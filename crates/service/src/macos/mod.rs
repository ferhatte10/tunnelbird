// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

//! The macOS side of the tunnel service: a launchd daemon running as root
//! (no paid Apple account, so no NetworkExtension), WireGuard® through
//! BoringTun on a `utun` interface, OpenVPN through the `openvpn` program,
//! the Unix socket `/var/run/tunnelbird.sock` for the menu bar app.
//! See docs/MACOS.md.
//!
//! WireGuard® has run on macOS; OpenVPN and launchd not yet (docs/MACOS.md).

pub mod net;
pub mod openvpn;

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, ToSocketAddrs};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use boringtun::device::{DeviceConfig, DeviceHandle};
use tunnelbird_core::plan::WgPlan;
use tunnelbird_core::profile::Profile;
use tunnelbird_core::protocol::{Counters, Login};

use tunnelbird_core::logs::Severity;

use crate::service::{Backend, BackendLine, Link};

const DATA_DIR: &str = "/Library/Application Support/Tunnelbird";
const LAUNCHD_LABEL: &str = "fr.ferhatsn.Tunnelbird.Service";
/// The `admin` group: members (the Mac's administrators) may use the socket.
const ADMIN_GID: u32 = 80;

pub fn data_dir() -> PathBuf {
    PathBuf::from(DATA_DIR)
}

enum Running {
    WireGuard {
        device: DeviceHandle,
        interface: String,
        logs: net::SharedLogs,
        handshake_seen: bool,
        // Dropped after the device: routes and DNS come back last.
        _journal: Arc<Mutex<net::Journal>>,
    },
    OpenVpn {
        process: openvpn::Process,
        // Dropped after the process: OpenVPN stops first.
        _journal: net::Journal,
    },
}

impl Running {
    fn interface(&self) -> &str {
        match self {
            Running::WireGuard { interface, .. } => interface,
            Running::OpenVpn { process, .. } => &process.interface,
        }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        if let Running::WireGuard { device, .. } = self {
            device.clean();
        }
    }
}

pub struct MacBackend {
    running: HashMap<String, Running>,
    run_dir: PathBuf,
}

impl MacBackend {
    pub fn new() -> Result<Self, String> {
        use std::os::unix::fs::DirBuilderExt;
        let run_dir = data_dir().join("run");
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&run_dir)
            .map_err(|err| format!("{}: {err}", run_dir.display()))?;
        // Before any new tunnel: a daemon that died may have left DNS and routes.
        restore_network(&run_dir);
        Ok(Self {
            running: HashMap::new(),
            run_dir,
        })
    }
}

/// Each server's address, IPv4 preferred.
fn resolve(plan: &WgPlan) -> Result<Vec<Option<SocketAddr>>, String> {
    plan.peers
        .iter()
        .map(|peer| {
            let Some(endpoint) = &peer.endpoint else {
                return Ok(None);
            };
            let addresses: Vec<SocketAddr> = endpoint
                .to_lookup()
                .to_socket_addrs()
                .map_err(|err| format!("Cannot find the server {}: {err}", endpoint.host))?
                .collect();
            Ok(addresses
                .iter()
                .find(|a| a.is_ipv4())
                .or(addresses.first())
                .copied())
        })
        .collect()
}

/// Sends the UAPI configuration to BoringTun's control socket.
fn configure(interface: &str, text: &str) -> Result<(), String> {
    let answer = query(interface, text)?;
    if answer.contains("errno=0") {
        Ok(())
    } else {
        Err(format!(
            "The tunnel refused its configuration ({})",
            answer.trim()
        ))
    }
}

fn query(interface: &str, text: &str) -> Result<String, String> {
    let path = format!("/var/run/wireguard/{interface}.sock");
    let mut socket = UnixStream::connect(&path).map_err(|err| format!("{path}: {err}"))?;
    socket
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|err| err.to_string())?;
    socket
        .write_all(text.as_bytes())
        .map_err(|err| err.to_string())?;
    let mut answer = String::new();
    let _ = socket.read_to_string(&mut answer);
    Ok(answer)
}

/// Seconds since 1970 of the latest handshake, if there has been one.
fn last_handshake(interface: &str) -> Option<u64> {
    let answer = query(interface, "get=1\n\n").ok()?;
    answer
        .lines()
        .filter_map(|line| line.strip_prefix("last_handshake_time_sec="))
        .filter_map(|value| value.parse().ok())
        .find(|seconds| *seconds > 0)
}

/// Keeps the tunnel's own packets on a physical interface, whatever the routes
/// say: this is what lets every other address, the server's included, go
/// through the tunnel. BoringTun takes the index as `fwmark` (our patch).
fn bind_to(interface: &str, index: u32) -> Result<(), String> {
    configure(interface, &format!("set=1\nfwmark={index}\n\n"))
}

impl Backend for MacBackend {
    fn start(
        &mut self,
        id: &str,
        profile: &Profile,
        login: Option<&Login>,
    ) -> Result<Link, String> {
        let interface = net::free_utun()?;
        let (running, addresses, full_tunnel) = match profile {
            Profile::WireGuard(config) => {
                let plan = WgPlan::from_config(config)?;
                let resolved = resolve(&plan)?;
                let mut journal = net::Journal::new(&self.run_dir);
                let device = DeviceHandle::new(
                    &interface,
                    DeviceConfig {
                        n_threads: 2,
                        // A connected socket would keep the previous network's address.
                        use_connected_socket: false,
                    },
                )
                .map_err(|err| format!("Cannot create the tunnel: {err:?}"))?;
                configure(&interface, &plan.uapi(&resolved))?;
                let logs = net::SharedLogs::default();
                logs.push(Severity::Info, format!("Interface {interface} created"));
                let physical = net::default_interface()?;
                journal.scope_to(&physical.name)?;
                bind_to(&interface, physical.index)?;
                logs.push(
                    Severity::Info,
                    format!("Tunnel packets leave through {}", physical.name),
                );
                net::add_addresses(&interface, &plan.addresses, plan.mtu)?;
                let routes = plan.routes();
                net::add_tunnel_routes(&interface, &routes)?;
                logs.push(Severity::Info, format!("{} routes added", routes.len()));
                if plan.leaves_ipv6_outside() {
                    journal.block_ipv6()?;
                    logs.push(
                        Severity::Info,
                        "IPv6 to the internet blocked: this tunnel carries only IPv4".to_owned(),
                    );
                }
                journal.set_dns(&plan.dns_servers, &plan.dns_search)?;
                if !plan.dns_servers.is_empty() {
                    logs.push(Severity::Info, "DNS servers set".to_owned());
                }
                let addresses = plan
                    .addresses
                    .iter()
                    .map(|n| format!("{}/{}", n.addr, n.prefix))
                    .collect();
                let journal = Arc::new(Mutex::new(journal));
                net::follow_network(Arc::downgrade(&journal), physical.index, logs.clone(), {
                    let interface = interface.clone();
                    move |index| bind_to(&interface, index)
                });
                (
                    Running::WireGuard {
                        device,
                        interface,
                        logs,
                        handshake_seen: false,
                        _journal: journal,
                    },
                    addresses,
                    None,
                )
            }
            Profile::OpenVpn(config) => {
                let process = openvpn::Process::start(
                    &self.run_dir,
                    id,
                    &interface,
                    &config.to_ovpn(),
                    login,
                )?;
                let mut journal = net::Journal::new(&self.run_dir);
                if net::leaves_ipv6_outside(&process.interface)? {
                    journal.block_ipv6()?;
                }
                let addresses = net::interface_addresses(&process.interface);
                let full_tunnel = Some(net::carries_all_ipv4(&process.interface)?);
                (
                    Running::OpenVpn {
                        process,
                        _journal: journal,
                    },
                    addresses,
                    full_tunnel,
                )
            }
        };
        self.running.insert(id.to_owned(), running);
        Ok(Link {
            addresses,
            full_tunnel,
        })
    }

    fn stop(&mut self, id: &str) -> Result<(), String> {
        self.running.remove(id);
        Ok(())
    }

    fn counters(&self, id: &str) -> Option<Counters> {
        net::counters(self.running.get(id)?.interface())
    }

    fn take_logs(&mut self, id: &str) -> Vec<BackendLine> {
        match self.running.get_mut(id) {
            Some(Running::OpenVpn { process, .. }) => process
                .take_logs()
                .into_iter()
                .map(BackendLine::Openvpn)
                .collect(),
            Some(Running::WireGuard {
                interface,
                logs,
                handshake_seen,
                ..
            }) => {
                if !*handshake_seen && last_handshake(interface).is_some() {
                    *handshake_seen = true;
                    logs.push(Severity::Info, "Handshake with the server done".to_owned());
                }
                logs.take()
                    .into_iter()
                    .map(|(severity, text)| BackendLine::Wireguard(severity, text))
                    .collect()
            }
            None => Vec::new(),
        }
    }

    fn stopped(&mut self, id: &str) -> Option<String> {
        match self.running.get_mut(id) {
            Some(Running::OpenVpn { process, .. }) => process.stopped(),
            _ => None,
        }
    }
}

/// Undoes what a tunnel left behind outside the process.
fn restore_network(run_dir: &std::path::Path) {
    openvpn::stop_leftover(run_dir);
    if let Err(err) = net::Journal::recover(run_dir) {
        eprintln!("Cannot restore the network: {err}");
    }
}

/// Puts the network back when the daemon is asked to stop (launchd, Ctrl+C):
/// without this, the tunnel's DNS would outlive the tunnel and names would stop
/// resolving. Call before any thread starts, so they all inherit the mask and
/// only this one receives the signals.
pub fn restore_network_on_stop() {
    use std::ffi::c_int;
    // `sigset_t` is a 32-bit mask on macOS.
    unsafe extern "C" {
        fn sigemptyset(set: *mut u32) -> c_int;
        fn sigaddset(set: *mut u32, signal: c_int) -> c_int;
        fn pthread_sigmask(how: c_int, set: *const u32, old: *mut u32) -> c_int;
        fn sigwait(set: *const u32, signal: *mut c_int) -> c_int;
    }
    const SIG_BLOCK: c_int = 1;
    const SIGINT: c_int = 2;
    const SIGTERM: c_int = 15;

    let mut set = 0u32;
    // SAFETY: `set` lives through the calls; a null `old` is allowed.
    unsafe {
        sigemptyset(&mut set);
        sigaddset(&mut set, SIGINT);
        sigaddset(&mut set, SIGTERM);
        pthread_sigmask(SIG_BLOCK, &set, std::ptr::null_mut());
    }
    std::thread::spawn(move || {
        let mut signal = 0;
        // SAFETY: both pointers are valid for the call.
        unsafe { sigwait(&set, &mut signal) };
        restore_network(&data_dir().join("run"));
        std::process::exit(0);
    });
}

/// Lets the Mac's administrators (group `admin`) use the socket.
pub fn open_socket_to_admins(socket: &std::path::Path) -> std::io::Result<()> {
    std::os::unix::fs::chown(socket, Some(0), Some(ADMIN_GID))
}

/// Where the daemon's program lives. Root runs it at every start, so it must
/// be a copy only root can change, never the file it was installed from. Not
/// `/Library/PrivilegedHelperTools`: macOS kills a program there that has only
/// an ad hoc signature (seen on macOS 27.2), and we have no paid one.
const INSTALLED_EXE: &str = "/Library/Application Support/Tunnelbird/tunnelbird-service";

fn launchd_plist(exe: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{LAUNCHD_LABEL}</string>
  <key>ProgramArguments</key><array><string>{exe}</string><string>--daemon</string></array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>StandardErrorPath</key><string>{DATA_DIR}/service.log</string>
</dict>
</plist>
"#
    )
}

/// Installs the launchd daemon (run once by the app's installer, as root), or
/// replaces the one already there.
pub fn install() -> Result<(), String> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    let exe = std::env::current_exe().map_err(|err| err.to_string())?;
    // Without its folder, launchd could not open the log and would not start it.
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(DATA_DIR)
        .map_err(|err| format!("{DATA_DIR}: {err}"))?;
    // Nothing to stop on a first installation: launchctl's complaint is noise.
    let _ = std::process::Command::new("/bin/launchctl")
        .args(["bootout", &format!("system/{LAUNCHD_LABEL}")])
        .stderr(std::process::Stdio::null())
        .status();
    // A new file, not written over the old one: macOS remembers the signature
    // of a program by its file, and kills it if the content changed in place.
    let _ = std::fs::remove_file(INSTALLED_EXE);
    std::fs::copy(&exe, INSTALLED_EXE).map_err(|err| format!("{INSTALLED_EXE}: {err}"))?;
    std::os::unix::fs::chown(INSTALLED_EXE, Some(0), Some(0))
        .map_err(|err| format!("{INSTALLED_EXE}: {err}"))?;
    std::fs::set_permissions(INSTALLED_EXE, std::fs::Permissions::from_mode(0o755))
        .map_err(|err| format!("{INSTALLED_EXE}: {err}"))?;
    let path = format!("/Library/LaunchDaemons/{LAUNCHD_LABEL}.plist");
    std::fs::write(&path, launchd_plist(INSTALLED_EXE)).map_err(|err| format!("{path}: {err}"))?;
    let status = std::process::Command::new("/bin/launchctl")
        .args(["bootstrap", "system", &path])
        .status()
        .map_err(|err| err.to_string())?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| "launchctl bootstrap failed".to_owned())
}

pub fn uninstall() -> Result<(), String> {
    let path = format!("/Library/LaunchDaemons/{LAUNCHD_LABEL}.plist");
    let _ = std::process::Command::new("/bin/launchctl")
        .args(["bootout", &format!("system/{LAUNCHD_LABEL}")])
        .status();
    let _ = std::fs::remove_file(INSTALLED_EXE);
    std::fs::remove_file(&path).map_err(|err| format!("{path}: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_daemon_runs_the_protected_copy() {
        let plist = launchd_plist(INSTALLED_EXE);
        assert!(plist.contains(&format!(
            "<string>{INSTALLED_EXE}</string><string>--daemon</string>"
        )));
        assert!(plist.contains(LAUNCHD_LABEL));
    }
}
