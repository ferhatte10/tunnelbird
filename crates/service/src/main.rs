// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

//! Tunnelbird's tunnel service for Windows and macOS (see docs/PROTOCOL.md).
//!
//! It runs with the rights tunnels need, stores the tunnels and drives the
//! VPN drivers; the native interface talks to it over a local channel. Linux
//! does not use it: NetworkManager plays this part.
//!
//! Status: the request handling, storage and Unix socket are done and tested
//! with a fake backend. Still to write, on the target machines: the Windows
//! backend (WireGuardNT, ovpn-dco-win), its named pipe and DPAPI storage, and
//! the macOS backend (BoringTun, `openvpn`).
//!
//! Development run, anywhere Unix: `cargo run -p tunnelbird-service -- --fake`
//! then talk to it with `nc -U "$XDG_RUNTIME_DIR/tunnelbird-dev.sock"`.

// Until the Windows named pipe and backend exist, nothing calls the service
// there; the code is still compiled to keep it Windows-ready.
#![cfg_attr(not(unix), allow(dead_code))]

mod service;
mod store;
mod transport;

use std::path::PathBuf;
use std::process::ExitCode;

use service::Service;
use service::fake::{FakeBackend, MemoryStore};
use store::FileStore;

struct Options {
    fake: bool,
    socket: PathBuf,
    store: Option<PathBuf>,
}

fn options() -> Result<Options, String> {
    let mut options = Options {
        fake: false,
        socket: default_socket(),
        store: None,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--fake" => options.fake = true,
            "--socket" => options.socket = args.next().ok_or("--socket needs a path")?.into(),
            "--store" => options.store = Some(args.next().ok_or("--store needs a folder")?.into()),
            "--help" => {
                return Err("usage: tunnelbird-service [--fake] [--socket PATH] [--store FOLDER]\n  --fake   scripted backend for development: nothing is connected for real\n  --store  keep tunnels in FOLDER (default with --fake: in memory)".to_owned());
            }
            other => return Err(format!("Unknown option {other} (see --help)")),
        }
    }
    Ok(options)
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
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };
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

#[cfg(not(unix))]
fn serve<B, S>(_: &std::path::Path, _: Service<B, S>) -> Result<(), String> {
    Err("The Windows named pipe is not written yet".to_owned())
}
