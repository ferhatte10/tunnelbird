// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

//! The Windows side of the tunnel service: a Windows service running as
//! SYSTEM, the WireGuardNT and ovpn-dco-win drivers, the named pipe and
//! DPAPI storage. See docs/WINDOWS.md.
//!
//! NOT YET RUN ON WINDOWS: written and compiled on Linux
//! (`cargo check --target x86_64-pc-windows-msvc`); the first run on a
//! Windows machine follows docs/WINDOWS.md, “First run”.

pub mod dpapi;
pub mod net;
pub mod openvpn;
pub mod pipe;
pub mod wireguard_nt;

use std::collections::HashMap;
use std::ffi::OsString;
use std::net::{SocketAddr, ToSocketAddrs};
use std::path::PathBuf;
use std::sync::{Arc, mpsc};
use std::time::Duration;

use tunnelbird_core::plan::WgPlan;
use tunnelbird_core::profile::Profile;
use tunnelbird_core::protocol::{Counters, Login};
use windows_service::service::{
    ServiceAccess, ServiceControl, ServiceControlAccept, ServiceErrorControl, ServiceExitCode,
    ServiceInfo, ServiceStartType, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};
use windows_service::{define_windows_service, service_dispatcher};

use crate::service::{Backend, Link, Service};
use crate::store::FileStore;

pub const SERVICE_NAME: &str = "Tunnelbird";
/// The adapter's name, as shown in Windows' network settings.
const ADAPTER: &str = "Tunnelbird";

/// UTF-16, NUL-terminated, for Win32 calls.
pub fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// `%ProgramData%\Tunnelbird`.
pub fn data_dir() -> PathBuf {
    std::env::var_os("ProgramData")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"))
        .join("Tunnelbird")
}

enum Running {
    WireGuard(wireguard_nt::Adapter),
    OpenVpn(openvpn::Process),
}

impl Running {
    fn counters(&self) -> Option<Counters> {
        match self {
            Running::WireGuard(adapter) => net::counters(adapter.luid()),
            Running::OpenVpn(process) => process.luid().and_then(net::counters),
        }
    }
}

pub struct WindowsBackend {
    driver: Option<Arc<wireguard_nt::Driver>>,
    running: HashMap<String, Running>,
    run_dir: PathBuf,
}

impl WindowsBackend {
    pub fn new() -> Result<Self, String> {
        let run_dir = data_dir().join("run");
        dpapi::create_private_dir(&run_dir)
            .map_err(|err| format!("{}: {err}", run_dir.display()))?;
        Ok(Self {
            driver: None,
            running: HashMap::new(),
            run_dir,
        })
    }

    /// WireGuardNT is loaded on first use: an OpenVPN-only user never needs it.
    fn driver(&mut self) -> Result<Arc<wireguard_nt::Driver>, String> {
        if let Some(driver) = &self.driver {
            return Ok(driver.clone());
        }
        let driver = Arc::new(wireguard_nt::Driver::load()?);
        self.driver = Some(driver.clone());
        Ok(driver)
    }
}

/// Each server's address, IPv4 preferred (most networks have it).
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

impl Backend for WindowsBackend {
    fn start(
        &mut self,
        id: &str,
        profile: &Profile,
        login: Option<&Login>,
    ) -> Result<Link, String> {
        let running = match profile {
            Profile::WireGuard(config) => {
                let plan = WgPlan::from_config(config)?;
                let resolved = resolve(&plan)?;
                let mut adapter = self.driver()?.start(ADAPTER, &plan, &resolved)?;
                let luid = adapter.luid();
                net::add_addresses(luid, &plan.addresses)?;
                net::add_tunnel_routes(luid, &plan.routes())?;
                adapter.routes.bypass(&plan.bypass(&resolved))?;
                net::set_dns(luid, &plan.dns_servers, &plan.dns_search)?;
                Running::WireGuard(adapter)
            }
            Profile::OpenVpn(config) => Running::OpenVpn(openvpn::Process::start(
                &self.run_dir,
                id,
                ADAPTER,
                &config.to_ovpn(),
                login,
            )?),
        };
        let addresses = match (&running, profile) {
            (Running::WireGuard(_), Profile::WireGuard(config)) => config
                .interface
                .addresses
                .iter()
                .map(|net| format!("{}/{}", net.addr, net.prefix))
                .collect(),
            _ => Vec::new(),
        };
        self.running.insert(id.to_owned(), running);
        Ok(Link {
            addresses,
            ..Link::default()
        })
    }

    fn stop(&mut self, id: &str) -> Result<(), String> {
        // Dropping the adapter or the process tears everything down.
        self.running.remove(id);
        Ok(())
    }

    fn counters(&self, id: &str) -> Option<Counters> {
        self.running.get(id)?.counters()
    }
}

define_windows_service!(ffi_service_main, service_main);

/// Called by the service control manager (`tunnelbird-service --service`).
pub fn run_service() -> Result<(), String> {
    service_dispatcher::start(SERVICE_NAME, ffi_service_main).map_err(|err| err.to_string())
}

fn service_main(_arguments: Vec<OsString>) {
    if let Err(err) = run() {
        log(&format!("The service stopped: {err}"));
    }
}

fn run() -> Result<(), String> {
    let (stop, stopped) = mpsc::channel();
    let handler = move |control| match control {
        ServiceControl::Stop | ServiceControl::Shutdown => {
            let _ = stop.send(());
            ServiceControlHandlerResult::NoError
        }
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        _ => ServiceControlHandlerResult::NotImplemented,
    };
    let status =
        service_control_handler::register(SERVICE_NAME, handler).map_err(|err| err.to_string())?;
    let set = |state, accepted| {
        status.set_service_status(ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: state,
            controls_accepted: accepted,
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: Duration::from_secs(10),
            process_id: None,
        })
    };
    set(
        ServiceState::Running,
        ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
    )
    .map_err(|err| err.to_string())?;

    let store = FileStore::open(data_dir().join("tunnels"))?;
    let service = Service::new(WindowsBackend::new()?, store)?;
    std::thread::spawn(move || {
        if let Err(err) = crate::transport::serve(pipe::incoming(), service) {
            log(&format!("The pipe stopped: {err}"));
        }
    });
    let _ = stopped.recv();
    // Tunnels go down with the process: adapters and OpenVPN are dropped.
    set(ServiceState::Stopped, ServiceControlAccept::empty()).map_err(|err| err.to_string())?;
    Ok(())
}

/// Registers the service to start with Windows (run by the installer).
pub fn install() -> Result<(), String> {
    let manager =
        ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CREATE_SERVICE)
            .map_err(|err| err.to_string())?;
    let executable = std::env::current_exe().map_err(|err| err.to_string())?;
    let info = ServiceInfo {
        name: SERVICE_NAME.into(),
        display_name: "Tunnelbird".into(),
        service_type: ServiceType::OWN_PROCESS,
        start_type: ServiceStartType::AutoStart,
        error_control: ServiceErrorControl::Normal,
        executable_path: executable,
        launch_arguments: vec!["--service".into()],
        dependencies: vec![],
        account_name: None, // LocalSystem
        account_password: None,
    };
    let service = manager
        .create_service(&info, ServiceAccess::CHANGE_CONFIG | ServiceAccess::START)
        .map_err(|err| err.to_string())?;
    service
        .set_description("Connects Tunnelbird's VPN tunnels")
        .map_err(|err| err.to_string())?;
    service.start::<&str>(&[]).map_err(|err| err.to_string())
}

/// Removes the service (run by the uninstaller).
pub fn uninstall() -> Result<(), String> {
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
        .map_err(|err| err.to_string())?;
    let service = manager
        .open_service(SERVICE_NAME, ServiceAccess::STOP | ServiceAccess::DELETE)
        .map_err(|err| err.to_string())?;
    let _ = service.stop();
    service.delete().map_err(|err| err.to_string())
}

/// `%ProgramData%\Tunnelbird\service.log`: a service has no console.
pub fn log(message: &str) {
    use std::io::Write;
    let path = data_dir().join("service.log");
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(file, "{message}");
    }
}
