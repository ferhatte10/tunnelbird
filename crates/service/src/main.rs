// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

//! Tunnelbird's tunnel service for Windows and macOS (see docs/PROTOCOL.md).
//!
//! It runs with the rights tunnels need, stores the tunnels and drives the
//! VPN drivers; the native interface talks to it over a local channel. Linux
//! does not use it: NetworkManager plays this part.
//!
//! Status: request handling, storage and the Unix socket are tested with a
//! fake backend. The macOS part (`macos/`) has run on a Mac (docs/MACOS.md). The
//! Windows part (`windows/`: WireGuardNT, ovpn-dco-win, named pipe, DPAPI,
//! Windows service) is written but has not run there (docs/WINDOWS.md).
//!
//! Development run, anywhere Unix: `cargo run -p tunnelbird-service -- --fake`
//! then talk to it with `nc -U "$XDG_RUNTIME_DIR/tunnelbird-dev.sock"`.

#[cfg(target_os = "macos")]
mod macos;
mod service;
mod store;
mod transport;
#[cfg(windows)]
mod windows;

// The macOS counters parser is pure: its test runs everywhere.
#[cfg(all(test, not(target_os = "macos")))]
#[allow(dead_code)]
#[path = "macos/net.rs"]
mod macos_net;

use std::path::PathBuf;
use std::process::ExitCode;

use service::Service;
use service::fake::{FakeBackend, MemoryStore};
use store::FileStore;

struct Options {
    fake: bool,
    /// macOS: the real backend, as the launchd daemon.
    #[cfg(target_os = "macos")]
    daemon: bool,
    socket: PathBuf,
    store: Option<PathBuf>,
}

fn options() -> Result<Options, String> {
    let mut options = Options {
        fake: false,
        #[cfg(target_os = "macos")]
        daemon: false,
        socket: default_socket(),
        store: None,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--fake" => options.fake = true,
            #[cfg(target_os = "macos")]
            "--daemon" => options.daemon = true,
            #[cfg(target_os = "macos")]
            "--install-daemon" => return Err(windows_or_mac_action(macos::install)),
            #[cfg(target_os = "macos")]
            "--uninstall-daemon" => return Err(windows_or_mac_action(macos::uninstall)),
            #[cfg(windows)]
            "--service" => return Err(windows_or_mac_action(windows::run_service)),
            #[cfg(windows)]
            "--install-service" => return Err(windows_or_mac_action(windows::install)),
            #[cfg(windows)]
            "--uninstall-service" => return Err(windows_or_mac_action(windows::uninstall)),
            "--socket" => options.socket = args.next().ok_or("--socket needs a path")?.into(),
            "--store" => options.store = Some(args.next().ok_or("--store needs a folder")?.into()),
            "--help" => {
                return Err("usage: tunnelbird-service [--fake | --daemon | --install-daemon | --uninstall-daemon] [--socket PATH] [--store FOLDER]\n  --fake   scripted backend for development: nothing is connected for real\n  --daemon real tunnels, macOS only, as root\n  --install-daemon, --uninstall-daemon  the launchd daemon, macOS only, as root\n  --store  keep tunnels in FOLDER (default with --fake: in memory)".to_owned());
            }
            other => return Err(format!("Unknown option {other} (see --help)")),
        }
    }
    Ok(options)
}

/// Runs a Windows-only action given on the command line, and reports how it
/// went as the message `main` prints before exiting.
#[cfg(any(windows, target_os = "macos"))]
fn windows_or_mac_action(action: fn() -> Result<(), String>) -> String {
    match action() {
        Ok(()) => String::new(),
        Err(err) => err,
    }
}

fn default_socket() -> PathBuf {
    if cfg!(target_os = "macos") {
        PathBuf::from("/var/run/tunnelbird.sock")
    } else {
        std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("tunnelbird-dev.sock")
    }
}

fn main() -> ExitCode {
    let options = match options() {
        Ok(options) => options,
        // A Windows action that went well leaves no message.
        Err(message) if message.is_empty() => return ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };
    #[cfg(target_os = "macos")]
    if options.daemon {
        return match run_daemon(&options.socket) {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!("{message}");
                ExitCode::FAILURE
            }
        };
    }
    if !options.fake {
        eprintln!(
            "No tunnel backend for this system yet: run with --fake to try the protocol (see docs/PROTOCOL.md)"
        );
        return ExitCode::FAILURE;
    }
    let result = match options.store {
        Some(dir) => FileStore::open(dir)
            .and_then(|store| Service::new(FakeBackend::default(), store))
            .and_then(|service| serve(&options.socket, service)),
        None => Service::new(FakeBackend::default(), MemoryStore::default())
            .and_then(|service| serve(&options.socket, service)),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(unix)]
fn serve<B, S>(socket: &std::path::Path, service: Service<B, S>) -> Result<(), String>
where
    B: service::Backend + Send + 'static,
    S: service::Store + Send + 'static,
{
    eprintln!("Listening on {} (fake backend)", socket.display());
    transport::serve_unix(socket, service).map_err(|err| format!("{}: {err}", socket.display()))
}

/// `--fake` on Windows: the named pipe, a scripted backend.
#[cfg(windows)]
fn serve<B, S>(_: &std::path::Path, service: Service<B, S>) -> Result<(), String>
where
    B: service::Backend + Send + 'static,
    S: service::Store + Send + 'static,
{
    eprintln!("Listening on {} (fake backend)", windows::pipe::NAME);
    transport::serve(windows::pipe::incoming(), service).map_err(|err| err.to_string())
}

/// The macOS daemon: real tunnels, tunnels kept in the root-only folder.
#[cfg(target_os = "macos")]
fn run_daemon(socket: &std::path::Path) -> Result<(), String> {
    macos::restore_network_on_stop();
    let store = FileStore::open(macos::data_dir().join("tunnels"))?;
    let service = Service::new(macos::MacBackend::new()?, store)?;
    // The socket is ready to chown just after the listener binds: done from
    // a short-lived thread so serving starts at once.
    let path = socket.to_owned();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(300));
        if let Err(err) = macos::open_socket_to_admins(&path) {
            eprintln!("Cannot open the socket to administrators: {err}");
        }
    });
    eprintln!("Listening on {}", socket.display());
    transport::serve_unix(socket, service).map_err(|err| format!("{}: {err}", socket.display()))
}
