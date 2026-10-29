// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

//! The macOS side of the tunnel service: a launchd daemon running as root
//! (no paid Apple account, so no NetworkExtension), WireGuard® through
//! BoringTun on a `utun` interface, OpenVPN through the `openvpn` program,
//! the Unix socket `/var/run/tunnelbird.sock` for the menu bar app.
//! See docs/MACOS.md.
//!
//! NOT YET RUN ON macOS: written and compiled on Linux; the first run
//! follows docs/MACOS.md, “First run”.

pub mod net;
pub mod openvpn;

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, ToSocketAddrs};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use boringtun::device::{DeviceConfig, DeviceHandle};
use tunnelbird_core::plan::WgPlan;
use tunnelbird_core::profile::Profile;
use tunnelbird_core::protocol::{Counters, Login};

use crate::service::{Backend, Link};

pub const SOCKET: &str = "/var/run/tunnelbird.sock";
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
        // Dropped after the device: routes and DNS come back first.
        _bypass: net::Bypass,
        _dns: net::Dns,
    },
    OpenVpn(openvpn::Process),
}

impl Running {
    fn interface(&self) -> &str {
        match self {
            Running::WireGuard { interface, .. } => interface,
            Running::OpenVpn(process) => &process.interface,
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
    if answer.contains("errno=0") {
        Ok(())
    } else {
        Err(format!(
            "The tunnel refused its configuration ({})",
            answer.trim()
        ))
    }
}

impl Backend for MacBackend {
    fn start(
        &mut self,
        id: &str,
        profile: &Profile,
        login: Option<&Login>,
    ) -> Result<Link, String> {
        let interface = net::free_utun()?;
        let (running, addresses) = match profile {
            Profile::WireGuard(config) => {
                let plan = WgPlan::from_config(config)?;
                let resolved = resolve(&plan)?;
                let device = DeviceHandle::new(
                    &interface,
                    DeviceConfig {
                        n_threads: 2,
                        use_connected_socket: true,
                    },
                )
                .map_err(|err| format!("Cannot create the tunnel: {err:?}"))?;
                configure(&interface, &plan.uapi(&resolved))?;
                net::add_addresses(&interface, &plan.addresses, plan.mtu)?;
                let mut bypass = net::Bypass::default();
                bypass.add(&plan.bypass(&resolved))?;
                net::add_tunnel_routes(&interface, &plan.routes())?;
                let dns = net::Dns::set(&plan.dns_servers, &plan.dns_search)?;
                let addresses = plan
                    .addresses
                    .iter()
                    .map(|n| format!("{}/{}", n.addr, n.prefix))
                    .collect();
                (
                    Running::WireGuard {
                        device,
                        interface,
                        _bypass: bypass,
                        _dns: dns,
                    },
                    addresses,
                )
            }
            Profile::OpenVpn(config) => (
                Running::OpenVpn(openvpn::Process::start(
                    &self.run_dir,
                    id,
                    &interface,
                    &config.to_ovpn(),
                    login,
                )?),
                Vec::new(),
            ),
        };
        self.running.insert(id.to_owned(), running);
        Ok(Link { addresses })
    }

    fn stop(&mut self, id: &str) -> Result<(), String> {
        self.running.remove(id);
        Ok(())
    }

    fn counters(&self, id: &str) -> Option<Counters> {
        net::counters(self.running.get(id)?.interface())
    }
}

/// Lets the Mac's administrators (group `admin`) use the socket.
pub fn open_socket_to_admins() -> std::io::Result<()> {
    std::os::unix::fs::chown(SOCKET, Some(0), Some(ADMIN_GID))
}

/// Installs the launchd daemon (run once by the app's installer, as root).
pub fn install() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|err| err.to_string())?;
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{LAUNCHD_LABEL}</string>
  <key>ProgramArguments</key><array><string>{}</string><string>--daemon</string></array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>StandardErrorPath</key><string>{DATA_DIR}/service.log</string>
</dict>
</plist>
"#,
        exe.display()
    );
    let path = format!("/Library/LaunchDaemons/{LAUNCHD_LABEL}.plist");
    std::fs::write(&path, plist).map_err(|err| format!("{path}: {err}"))?;
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
    std::fs::remove_file(&path).map_err(|err| format!("{path}: {err}"))
}
