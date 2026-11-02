// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

//! Addresses, routes, DNS and counters of a `utun` interface on macOS, with
//! the system's own commands (`ifconfig`, `route`, `networksetup`,
//! `netstat`), the way `wg-quick` does it on macOS.
//!
//! DNS: macOS has no per-interface DNS for a utun without NetworkExtension,
//! so, like `wg-quick`, the tunnel's servers are set on every network service
//! and the previous ones are put back when the tunnel goes down.
//!
//! These changes outlive the process, so [`Journal`] writes them down first:
//! if the daemon dies, its next start undoes them.
//!
//! See docs/MACOS.md for what has run.

use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tunnelbird_core::config::IpNet;
use tunnelbird_core::logs::Severity;
use tunnelbird_core::protocol::Counters;

fn run(program: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|err| format!("{program}: {err}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(format!(
            "{program} {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

/// A `utunN` name nobody uses, from utun10 up (lower numbers belong to the
/// system and other VPN apps).
pub fn free_utun() -> Result<String, String> {
    let taken = run("/sbin/ifconfig", &["-l"])?;
    (10..256)
        .map(|n| format!("utun{n}"))
        .find(|name| !taken.split_whitespace().any(|t| t == name))
        .ok_or_else(|| "No free utun interface".to_owned())
}

pub fn add_addresses(interface: &str, addresses: &[IpNet], mtu: Option<u32>) -> Result<(), String> {
    for net in addresses {
        let address = format!("{}/{}", net.addr, net.prefix);
        match net.addr {
            IpAddr::V4(ip) => {
                let ip = ip.to_string();
                run(
                    "/sbin/ifconfig",
                    &[interface, "inet", &address, &ip, "alias"],
                )?;
            }
            IpAddr::V6(_) => {
                run("/sbin/ifconfig", &[interface, "inet6", &address, "alias"])?;
            }
        }
    }
    let mtu = mtu.unwrap_or(1420).to_string();
    run("/sbin/ifconfig", &[interface, "mtu", &mtu, "up"])?;
    Ok(())
}

pub fn add_tunnel_routes(interface: &str, nets: &[IpNet]) -> Result<(), String> {
    for net in nets {
        let family = if net.addr.is_ipv4() {
            "-inet"
        } else {
            "-inet6"
        };
        let destination = format!("{}/{}", net.addr, net.prefix);
        run(
            "/sbin/route",
            &[
                "-q",
                "-n",
                "add",
                family,
                &destination,
                "-interface",
                interface,
            ],
        )?;
    }
    Ok(())
}

unsafe extern "C" {
    fn if_nametoindex(name: *const std::ffi::c_char) -> std::ffi::c_uint;
}

/// The interface the system uses for internet.
pub struct PhysicalInterface {
    pub name: String,
    pub index: u32,
}

/// `scutil --nwi` and not `route get`: a utun without NetworkExtension is not
/// listed, so the answer stays the physical network while the tunnel is up.
pub fn default_interface() -> Result<PhysicalInterface, String> {
    let output = run("/usr/sbin/scutil", &["--nwi"])?;
    let name = primary_interface(&output).ok_or("No network")?;
    let c_name = std::ffi::CString::new(name).map_err(|err| err.to_string())?;
    // SAFETY: `c_name` is a valid C string for the duration of the call.
    match unsafe { if_nametoindex(c_name.as_ptr()) } {
        0 => Err("No network".to_owned()),
        index => Ok(PhysicalInterface {
            name: name.to_owned(),
            index,
        }),
    }
}

/// First interface listed, the most preferred, IPv4 before IPv6.
fn primary_interface(nwi: &str) -> Option<&str> {
    nwi.lines()
        .filter_map(|line| line.trim_start().split_once(" : flags"))
        .map(|(name, _)| name.trim())
        .find(|name| *name != "REACH")
}

/// The gateway of `interface`'s ordinary default route in `netstat -rn`.
/// Scoped routes (flag `I`) are left out: ours may hold an outdated gateway.
fn default_gateway(routes: &str, interface: &str) -> Option<String> {
    routes.lines().find_map(|line| {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let [destination, gateway, flags, netif, ..] = fields.as_slice() else {
            return None;
        };
        (*destination == "default"
            && *netif == interface
            && !gateway.starts_with("link#")
            && !flags.contains('I'))
        .then(|| (*gateway).to_owned())
    })
}

/// Log lines from a tunnel's threads, waiting to be read.
#[derive(Clone, Default)]
pub struct SharedLogs(Arc<Mutex<Vec<(Severity, String)>>>);

impl SharedLogs {
    pub fn push(&self, severity: Severity, text: String) {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push((severity, text));
    }

    pub fn take(&self) -> Vec<(Severity, String)> {
        std::mem::take(
            &mut *self
                .0
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        )
    }
}

/// True when the table has a default route scoped to `interface` (flag `I`).
fn has_scoped_default(routes: &str, interface: &str, gateway: &str) -> bool {
    routes.lines().any(|line| {
        let fields: Vec<&str> = line.split_whitespace().collect();
        matches!(fields.as_slice(), [destination, g, flags, netif, ..]
            if *destination == "default" && *g == gateway && *netif == interface && flags.contains('I'))
    })
}

/// Follows the physical network while a tunnel is up (Wi-Fi to Ethernet, or a
/// new gateway on the same Wi-Fi), until the tunnel drops its journal.
pub fn follow_network(
    journal: Weak<Mutex<Journal>>,
    mut bound: u32,
    logs: SharedLogs,
    rebind: impl Fn(u32) -> Result<(), String> + Send + 'static,
) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(2));
            let Some(journal) = journal.upgrade() else {
                return;
            };
            // No network for now: stay bound until another one comes.
            let Ok(physical) = default_interface() else {
                continue;
            };
            let scoped = journal
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .scope_to(&physical.name);
            let bound_now = scoped.and_then(|()| {
                if physical.index == bound {
                    Ok(())
                } else {
                    rebind(physical.index)
                }
            });
            match bound_now {
                Ok(()) => {
                    if physical.index != bound {
                        logs.push(
                            Severity::Info,
                            format!("Network changed: now on {}", physical.name),
                        );
                    }
                    bound = physical.index;
                }
                Err(err) => {
                    logs.push(
                        Severity::Warning,
                        format!("Cannot follow the network: {err}"),
                    );
                }
            }
        }
    });
}

/// A network service's DNS before the tunnel: name, servers, search domains.
type SavedDns = (String, Vec<String>, Vec<String>);

/// A default route of ours scoped to an interface: family, interface, gateway.
type ScopedRoute = (String, String, String);

/// What the running tunnel changed outside the process.
#[derive(Default, Debug, PartialEq, Serialize, Deserialize)]
struct Changes {
    dns: Vec<SavedDns>,
    #[serde(default)]
    scoped: Vec<ScopedRoute>,
    #[serde(default)]
    ipv6_blocked: bool,
}

/// The tunnel's changes outside the process: on disk before each is made, so a
/// daemon that dies can undo them at its next start, and undone on drop.
pub struct Journal {
    path: PathBuf,
    changes: Changes,
}

impl Journal {
    pub fn new(run_dir: &Path) -> Self {
        Self {
            path: journal_path(run_dir),
            changes: Changes::default(),
        }
    }

    fn save(&self) -> Result<(), String> {
        save_changes(&self.path, &self.changes)
    }

    /// Gives a socket bound to `interface` a way out. Without a default route
    /// scoped to it, the tunnel's `0/1` and `128.0/1` routes make such a socket
    /// fail with “Network is unreachable”.
    pub fn scope_to(&mut self, interface: &str) -> Result<(), String> {
        let mut found = false;
        for (family, table) in [("-inet", "inet"), ("-inet6", "inet6")] {
            let routes = run("/usr/sbin/netstat", &["-rn", "-f", table])?;
            let Some(gateway) = default_gateway(&routes, interface) else {
                continue;
            };
            found = true;
            let route = (family.to_owned(), interface.to_owned(), gateway);
            // The system removes it when the interface goes down: the journal
            // alone cannot say it is still there.
            if self.changes.scoped.contains(&route)
                && has_scoped_default(&routes, interface, &route.2)
            {
                continue;
            }
            // The same network with a new gateway: ours must go first.
            self.changes.scoped.retain(|(f, i, g)| {
                let outdated = f == family && i == interface;
                if outdated {
                    delete_scoped(&(f.clone(), i.clone(), g.clone()));
                }
                !outdated
            });
            self.changes.scoped.push(route.clone());
            self.save()?;
            let added = run(
                "/sbin/route",
                &[
                    "-q", "-n", "add", family, "-ifscope", interface, "default", &route.2,
                ],
            );
            if let Err(err) = added {
                self.changes.scoped.retain(|r| *r != route);
                self.save()?;
                // Already there: it is the system's own, and it stays.
                if err.contains("File exists") {
                    continue;
                }
                // Most servers are reached over IPv4: IPv6 alone is not fatal.
                if family == "-inet" {
                    return Err(err);
                }
                eprintln!("{err}");
            }
        }
        if found {
            Ok(())
        } else {
            Err("No network".to_owned())
        }
    }

    /// Makes IPv6 to the internet fail at once, so applications fall back to
    /// IPv4, which goes through the tunnel. `::/1` and `8000::/1` are more
    /// specific than any default route; the local network (its own `/64`,
    /// `fe80::/10`) is more specific still and keeps working.
    pub fn block_ipv6(&mut self) -> Result<(), String> {
        self.changes.ipv6_blocked = true;
        self.save()?;
        for destination in IPV6_BLOCKED {
            run(
                "/sbin/route",
                &["-q", "-n", "add", "-inet6", destination, "::1", "-reject"],
            )?;
        }
        Ok(())
    }

    /// Sets the DNS servers on every network service.
    pub fn set_dns(&mut self, servers: &[IpAddr], search: &[String]) -> Result<(), String> {
        if servers.is_empty() {
            return Ok(());
        }
        let list = run("/usr/sbin/networksetup", &["-listallnetworkservices"])?;
        // First line is a notice; disabled services start with “*”.
        for service in list
            .lines()
            .skip(1)
            .filter(|s| !s.starts_with('*') && !s.is_empty())
        {
            let read = |what: &str| -> Vec<String> {
                run("/usr/sbin/networksetup", &[what, service])
                    .unwrap_or_default()
                    .lines()
                    .filter(|line| !line.contains(' '))
                    .map(str::to_owned)
                    .collect()
            };
            self.changes.dns.push((
                service.to_owned(),
                read("-getdnsservers"),
                read("-getsearchdomains"),
            ));
            self.save()?;
            let mut args = vec!["-setdnsservers", service];
            let servers: Vec<String> = servers.iter().map(ToString::to_string).collect();
            args.extend(servers.iter().map(String::as_str));
            run("/usr/sbin/networksetup", &args)?;
            if !search.is_empty() {
                let mut args = vec!["-setsearchdomains", service];
                args.extend(search.iter().map(String::as_str));
                run("/usr/sbin/networksetup", &args)?;
            }
        }
        Ok(())
    }

    /// Undoes what a daemon that died left behind, if anything.
    pub fn recover(run_dir: &Path) -> Result<(), String> {
        let path = journal_path(run_dir);
        let Some(changes) = load(&path)? else {
            return Ok(());
        };
        undo(&changes);
        std::fs::remove_file(&path).map_err(|err| format!("{}: {err}", path.display()))
    }
}

impl Drop for Journal {
    fn drop(&mut self) {
        undo(&self.changes);
        let _ = std::fs::remove_file(&self.path);
    }
}

const IPV6_BLOCKED: [&str; 2] = ["::/1", "8000::/1"];

/// True when `interface` carries all IPv4. For OpenVPN: the server decides.
pub fn carries_all_ipv4(interface: &str) -> Result<bool, String> {
    let ipv4 = run("/usr/sbin/netstat", &["-rn", "-f", "inet"])?;
    Ok(has_route(&ipv4, "0/1", interface))
}

/// True when `interface` carries all IPv4 but not IPv6.
pub fn leaves_ipv6_outside(interface: &str) -> Result<bool, String> {
    let ipv6 = run("/usr/sbin/netstat", &["-rn", "-f", "inet6"])?;
    Ok(carries_all_ipv4(interface)? && !has_route(&ipv6, "::/1", interface))
}

/// The addresses of an interface, from `ifconfig`, link-local ones left out.
pub fn interface_addresses(interface: &str) -> Vec<String> {
    run("/sbin/ifconfig", &[interface])
        .map(|output| parse_addresses(&output))
        .unwrap_or_default()
}

fn parse_addresses(ifconfig: &str) -> Vec<String> {
    ifconfig
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            match (fields.next(), fields.next()) {
                (Some("inet" | "inet6"), Some(address)) if !address.starts_with("fe80") => {
                    Some(address.to_owned())
                }
                _ => None,
            }
        })
        .collect()
}

fn has_route(routes: &str, destination: &str, interface: &str) -> bool {
    routes.lines().any(|line| {
        let fields: Vec<&str> = line.split_whitespace().collect();
        matches!(fields.as_slice(), [d, _, _, netif, ..] if *d == destination && *netif == interface)
    })
}

fn undo(changes: &Changes) {
    if changes.ipv6_blocked {
        for destination in IPV6_BLOCKED {
            let _ = run(
                "/sbin/route",
                &["-q", "-n", "delete", "-inet6", destination],
            );
        }
    }
    for route in &changes.scoped {
        delete_scoped(route);
    }
    for saved in &changes.dns {
        restore_dns(saved);
    }
}

fn delete_scoped((family, interface, _): &ScopedRoute) {
    let _ = run(
        "/sbin/route",
        &[
            "-q", "-n", "delete", family, "-ifscope", interface, "default",
        ],
    );
}

fn save_changes(path: &Path, changes: &Changes) -> Result<(), String> {
    use std::io::Write;
    let text = serde_json::to_string(changes).map_err(|err| err.to_string())?;
    // Through a second file: a crash while writing leaves the old journal whole.
    let temporary = path.with_extension("tmp");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    // The tests of other systems build this file too, and have no mode.
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(&temporary)
        .and_then(|mut file| file.write_all(text.as_bytes()))
        .and_then(|()| std::fs::rename(&temporary, path))
        .map_err(|err| format!("{}: {err}", path.display()))
}

fn journal_path(run_dir: &Path) -> PathBuf {
    run_dir.join("changes.json")
}

fn load(path: &Path) -> Result<Option<Changes>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text)
            .map(Some)
            .map_err(|err| format!("{}: {err}", path.display())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(format!("{}: {err}", path.display())),
    }
}

fn restore_dns((service, servers, search): &SavedDns) {
    // “Empty” means: back to what DHCP gives.
    let mut args = vec!["-setdnsservers", service.as_str()];
    if servers.is_empty() {
        args.push("Empty");
    } else {
        args.extend(servers.iter().map(String::as_str));
    }
    let _ = run("/usr/sbin/networksetup", &args);
    let mut args = vec!["-setsearchdomains", service.as_str()];
    if search.is_empty() {
        args.push("Empty");
    } else {
        args.extend(search.iter().map(String::as_str));
    }
    let _ = run("/usr/sbin/networksetup", &args);
}

/// Counters of an interface, from `netstat -I <name> -b -n` (its link line).
pub fn counters(interface: &str) -> Option<Counters> {
    let output = run("/usr/sbin/netstat", &["-I", interface, "-b", "-n"]).ok()?;
    parse_netstat(&output)
}

fn parse_netstat(output: &str) -> Option<Counters> {
    // Name Mtu Network Address Ipkts Ierrs Ibytes Opkts Oerrs Obytes Coll
    let line = output.lines().find(|line| line.contains("<Link#"))?;
    let fields: Vec<&str> = line.split_whitespace().collect();
    // A utun has no hardware address: the Address column is missing.
    let numbers: Vec<u64> = fields
        .iter()
        .skip(3)
        .filter_map(|f| f.parse().ok())
        .collect();
    let [rx_packets, _, rx_bytes, tx_packets, _, tx_bytes, ..] = numbers.as_slice() else {
        return None;
    };
    Some(Counters {
        rx_bytes: *rx_bytes,
        tx_bytes: *tx_bytes,
        rx_packets: *rx_packets,
        tx_packets: *tx_packets,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primary_interface_comes_first() {
        let nwi = "Network information\n\nIPv4 network interface information\n     en7 : flags      : 0x7 (IPv4,IPv6,DNS)\n           address    : 192.0.2.2\n     en0 : flags      : 0x7 (IPv4,IPv6,DNS)\n\n   REACH : flags 0x00000002 (Reachable)\n\nIPv6 network interface information\n   No IPv6 states found\n";
        assert_eq!(primary_interface(nwi), Some("en7"));
        let none = "IPv4 network interface information\n   No IPv4 states found\n\n   REACH : flags 0x00000000 (Not Reachable)\n";
        assert_eq!(primary_interface(none), None);
    }

    #[test]
    fn routes_are_matched_by_destination_and_interface() {
        let routes = "0/1  192.0.2.1  UGScg  utun4\n::/1  fe80::1  UGSc  utun10\n";
        assert!(has_route(routes, "0/1", "utun4"));
        assert!(!has_route(routes, "0/1", "utun10"));
        assert!(!has_route(routes, "128.0/1", "utun4"));
    }

    #[test]
    fn addresses_are_read_from_ifconfig() {
        let output = "utun4: flags=8051<UP,POINTOPOINT,RUNNING> mtu 1500\n\
                      \tinet 192.0.2.2 --> 192.0.2.1 netmask 0xffffff00\n\
                      \tinet6 fe80::1%utun4 prefixlen 64 scopeid 0x1b\n\
                      \tinet6 2001:db8::2 prefixlen 128\n";
        assert_eq!(parse_addresses(output), ["192.0.2.2", "2001:db8::2"]);
    }

    #[test]
    fn scoped_routes_are_told_from_ordinary_ones() {
        let ordinary = "default  192.0.2.1  UGScg  en0\n";
        let scoped = "default  192.0.2.1  UGScg  en0\ndefault  192.0.2.1  UGScIg  en0\n";
        assert!(!has_scoped_default(ordinary, "en0", "192.0.2.1"));
        assert!(has_scoped_default(scoped, "en0", "192.0.2.1"));
        assert!(!has_scoped_default(scoped, "en0", "192.0.2.9"));
        assert!(!has_scoped_default(scoped, "en7", "192.0.2.1"));
    }

    #[test]
    fn gateway_comes_from_the_ordinary_default_route() {
        let routes = "Destination  Gateway  Flags  Netif Expire\n\
                      0/1          link#27  UScg   utun10\n\
                      default      192.0.2.1  UGScg  en0\n\
                      default      192.0.2.9  UGScIg en0\n\
                      default      link#9   UCSIg  bridge100  !\n";
        assert_eq!(default_gateway(routes, "en0").as_deref(), Some("192.0.2.1"));
        assert_eq!(default_gateway(routes, "bridge100"), None);
        assert_eq!(default_gateway(routes, "en7"), None);
    }

    #[test]
    fn journal_is_written_then_removed() {
        let dir = std::env::temp_dir().join(format!("tb-journal-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let path = journal_path(&dir);
        assert_eq!(load(&path), Ok(None));

        // Saved directly: dropping a journal with content would touch the network.
        let changes = Changes {
            dns: vec![(
                "Wi-Fi".to_owned(),
                vec!["198.51.100.53".to_owned()],
                Vec::new(),
            )],
            scoped: vec![("-inet".to_owned(), "en0".to_owned(), "192.0.2.1".to_owned())],
            ipv6_blocked: true,
        };
        save_changes(&path, &changes).unwrap();
        assert_eq!(load(&path), Ok(Some(changes)));

        drop(Journal::new(&dir));
        assert_eq!(load(&path), Ok(None));
        std::fs::remove_dir(&dir).unwrap();
    }

    #[test]
    fn netstat_link_line_is_read() {
        let output = "Name       Mtu   Network       Address            Ipkts Ierrs     Ibytes    Opkts Oerrs     Obytes  Coll\n\
                      utun10    1420  <Link#21>                          1200     0    1048576      800     0      65536     0\n\
                      utun10    1420  10.8.0.2/32   10.8.0.2             1200     -    1048576      800     -      65536     -\n";
        assert_eq!(
            parse_netstat(output),
            Some(Counters {
                rx_bytes: 1_048_576,
                tx_bytes: 65_536,
                rx_packets: 1200,
                tx_packets: 800
            })
        );
    }
}
