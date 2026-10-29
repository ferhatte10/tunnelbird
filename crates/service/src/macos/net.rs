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
//! NOT YET RUN ON macOS (docs/MACOS.md).

use std::net::IpAddr;
use std::process::Command;

use tunnelbird_core::config::IpNet;
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

/// Host routes keeping the servers on the current gateway; removed by `Drop`.
#[derive(Default)]
pub struct Bypass(Vec<String>);

impl Bypass {
    pub fn add(&mut self, hosts: &[IpNet]) -> Result<(), String> {
        for host in hosts {
            let family = if host.addr.is_ipv4() {
                "-inet"
            } else {
                "-inet6"
            };
            let default = if host.addr.is_ipv4() {
                "default"
            } else {
                "-inet6 default"
            };
            let output = run("/sbin/route", &["-n", "get", default])?;
            let gateway = output
                .lines()
                .find_map(|line| line.trim().strip_prefix("gateway:"))
                .map(str::trim)
                .ok_or("No default gateway")?
                .to_owned();
            let address = host.addr.to_string();
            run(
                "/sbin/route",
                &["-q", "-n", "add", family, &address, &gateway],
            )?;
            self.0.push(address);
        }
        Ok(())
    }
}

impl Drop for Bypass {
    fn drop(&mut self) {
        for address in &self.0 {
            let _ = run("/sbin/route", &["-q", "-n", "delete", address]);
        }
    }
}

/// DNS servers set on every network service, the old ones put back on drop.
pub struct Dns(Vec<(String, Vec<String>, Vec<String>)>);

impl Dns {
    pub fn set(servers: &[IpAddr], search: &[String]) -> Result<Self, String> {
        let mut saved = Vec::new();
        if servers.is_empty() {
            return Ok(Self(saved));
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
            saved.push((
                service.to_owned(),
                read("-getdnsservers"),
                read("-getsearchdomains"),
            ));
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
        Ok(Self(saved))
    }
}

impl Drop for Dns {
    fn drop(&mut self) {
        for (service, servers, search) in &self.0 {
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
    }
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
