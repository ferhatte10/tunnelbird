// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

//! The local channel to the interfaces: one JSON message per line (see
//! docs/PROTOCOL.md): a Unix socket on macOS (and Linux, for development with
//! the fake backend), the named pipe on Windows (`windows/pipe.rs`).
//!
//! Each client gets a thread. Requests go through one shared [`Service`]
//! (one at a time); events are sent to every client that said `hello`.

use std::io::{BufRead, BufReader, Write};
use std::sync::{Arc, Mutex, MutexGuard};

use tunnelbird_core::protocol::{
    ErrorCode, Event, EventMessage, Request, RequestBody, Response, ResponseBody, ServiceMessage,
};

use crate::service::{Backend, Service, Store};

/// Clients that may receive events, by connection number.
type Clients<W> = Arc<Mutex<Vec<(u64, W)>>>;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // A thread that panicked holding the lock leaves usable data: keep going.
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Sends `events` to every client; a client that cannot be written to is dropped.
fn broadcast<W: Write>(clients: &Clients<W>, events: Vec<Event>) {
    if events.is_empty() {
        return;
    }
    let mut clients = lock(clients);
    for event in events {
        let line = ServiceMessage::Event(EventMessage { event }).to_line();
        clients.retain_mut(|(_, writer)| writer.write_all(line.as_bytes()).is_ok());
    }
}

/// Serves one client until it hangs up.
fn serve_client<R, W, B, S>(
    number: u64,
    reader: R,
    mut writer: W,
    service: &Arc<Mutex<Service<B, S>>>,
    clients: &Clients<W>,
) where
    R: std::io::Read,
    W: Write + TryClone,
    B: Backend,
    S: Store,
{
    let mut greeted = false;
    for line in BufReader::new(reader).lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let (id, body) = match Request::from_line(&line) {
            Ok(Request { id, body }) => (id, body),
            Err(err) => {
                let reply = ServiceMessage::Response(Response {
                    id: 0,
                    body: ResponseBody::Error {
                        code: ErrorCode::Internal,
                        message: format!("Unreadable request: {err}"),
                    },
                });
                if writer.write_all(reply.to_line().as_bytes()).is_err() {
                    break;
                }
                continue;
            }
        };
        let is_hello = matches!(body, RequestBody::Hello { .. });
        let (response, events) = if greeted || is_hello {
            lock(service).handle(body)
        } else {
            (
                ResponseBody::Error {
                    code: ErrorCode::VersionMismatch,
                    message: "Send hello first".to_owned(),
                },
                Vec::new(),
            )
        };
        if is_hello && matches!(response, ResponseBody::Hello { .. }) && !greeted {
            greeted = true;
            if let Ok(copy) = writer.try_clone_writer() {
                lock(clients).push((number, copy));
            }
        }
        let reply = ServiceMessage::Response(Response { id, body: response });
        if writer.write_all(reply.to_line().as_bytes()).is_err() {
            break;
        }
        broadcast(clients, events);
    }
    lock(clients).retain(|(n, _)| *n != number);
}

/// A writer that can be duplicated, so events reach a client while its own
/// thread waits for its next request.
pub trait TryClone: Sized {
    fn try_clone_writer(&self) -> std::io::Result<Self>;
}

#[cfg(unix)]
impl TryClone for std::os::unix::net::UnixStream {
    fn try_clone_writer(&self) -> std::io::Result<Self> {
        self.try_clone()
    }
}

/// Serves clients as they come, until the process ends. Requests go
/// through one shared [`Service`]; a thread sends the traffic every second.
pub fn serve<C, B, S>(
    incoming: impl Iterator<Item = std::io::Result<C>>,
    service: Service<B, S>,
) -> std::io::Result<()>
where
    C: std::io::Read + Write + TryClone + Send + 'static,
    B: Backend + Send + 'static,
    S: Store + Send + 'static,
{
    let service = Arc::new(Mutex::new(service));
    let clients: Clients<C> = Arc::new(Mutex::new(Vec::new()));

    // Traffic of the connected tunnel, once a second.
    {
        let (service, clients) = (service.clone(), clients.clone());
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(std::time::Duration::from_secs(1));
                let events = lock(&service).tick();
                broadcast(&clients, events);
            }
        });
    }

    for (number, stream) in (1u64..).zip(incoming) {
        let stream = match stream {
            Ok(stream) => stream,
            Err(err) => {
                eprintln!("Connection refused: {err}");
                continue;
            }
        };
        let reader = stream.try_clone_writer()?;
        let (service, clients) = (service.clone(), clients.clone());
        std::thread::spawn(move || serve_client(number, reader, stream, &service, &clients));
    }
    Ok(())
}

/// Listens on a Unix socket readable by its owner and group only (root and
/// `admin` on macOS).
#[cfg(unix)]
pub fn serve_unix<B, S>(path: &std::path::Path, service: Service<B, S>) -> std::io::Result<()>
where
    B: Backend + Send + 'static,
    S: Store + Send + 'static,
{
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixListener;

    // A socket left by a previous run would make bind fail.
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    let listener = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o660))?;
    serve(listener.incoming(), service)
}
