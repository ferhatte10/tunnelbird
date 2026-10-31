// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

//! OpenVPN on macOS: the community `openvpn` program on a `utun` interface.
//! It sets the addresses and routes the server pushes. The helper looks for
//! it inside the app bundle first, then where Homebrew installs it.
//!
//! The profile and login go to a folder only root can read; the login file
//! is deleted as soon as OpenVPN has read it.
//!
//! See docs/MACOS.md for what has run.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use tunnelbird_core::protocol::Login;

const STARTUP_TIMEOUT: Duration = Duration::from_secs(45);
/// OpenVPN takes down its routes and DNS itself, if it is given time to.
const STOP_TIMEOUT: Duration = Duration::from_secs(5);
const PID_FILE: &str = "openvpn.pid";
const SIGTERM: std::ffi::c_int = 15;

unsafe extern "C" {
    fn kill(pid: std::ffi::c_int, signal: std::ffi::c_int) -> std::ffi::c_int;
}

/// Asks a process to stop cleanly.
fn terminate(pid: u32) {
    // SAFETY: plain system call; a process already gone only returns an error.
    unsafe { kill(pid as std::ffi::c_int, SIGTERM) };
}

/// Stops an OpenVPN left by a daemon that died, and removes its files: the
/// profile, and the login if it had not started yet.
pub fn stop_leftover(run_dir: &Path) {
    let pid_file = run_dir.join(PID_FILE);
    if let Some(pid) = std::fs::read_to_string(&pid_file)
        .ok()
        .and_then(|text| text.trim().parse::<u32>().ok())
    {
        // The number may belong to another program by now.
        let name = Command::new("/bin/ps")
            .args(["-o", "comm=", "-p", &pid.to_string()])
            .output()
            .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
            .unwrap_or_default();
        if name.trim().ends_with("openvpn") {
            terminate(pid);
            std::thread::sleep(STOP_TIMEOUT);
        }
    }
    let _ = std::fs::remove_file(pid_file);
    let Ok(files) = std::fs::read_dir(run_dir) else {
        return;
    };
    for file in files.flatten().map(|entry| entry.path()) {
        if file
            .extension()
            .is_some_and(|ext| ext == "ovpn" || ext == "auth")
        {
            let _ = std::fs::remove_file(file);
        }
    }
}

/// The interface in OpenVPN's log line `Opened utun device utun4`: it may
/// choose another one than asked.
fn opened_device(line: &str) -> Option<&str> {
    line.split_once("Opened utun device ")
        .map(|(_, device)| device.trim())
}

const CANDIDATES: [&str; 3] = [
    "/Applications/Tunnelbird.app/Contents/Resources/openvpn",
    "/opt/homebrew/sbin/openvpn",
    "/usr/local/sbin/openvpn",
];

pub struct Process {
    child: Child,
    pub interface: String,
    files: Vec<PathBuf>,
    /// Log lines nobody has read yet.
    logs: Arc<Mutex<Vec<String>>>,
}

impl Process {
    pub fn start(
        run_dir: &Path,
        id: &str,
        interface: &str,
        profile: &str,
        login: Option<&Login>,
    ) -> Result<Self, String> {
        let exe = CANDIDATES
            .iter()
            .map(Path::new)
            .find(|path| path.exists())
            .ok_or("OpenVPN is not installed (brew install openvpn)")?;
        let config = run_dir.join(format!("{id}.ovpn"));
        write_private(&config, profile)?;
        let mut files = vec![config.clone()];
        let mut command = Command::new(exe);
        command
            .arg("--config")
            .arg(&config)
            .args(["--dev", interface, "--dev-type", "tun", "--verb", "3"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if let Some(login) = login {
            let auth = run_dir.join(format!("{id}.auth"));
            write_private(&auth, &format!("{}\n{}\n", login.username, login.password))?;
            command
                .arg("--auth-user-pass")
                .arg(&auth)
                .arg("--auth-nocache");
            files.push(auth);
        }
        let mut child = command
            .spawn()
            .map_err(|err| format!("Cannot start OpenVPN: {err}"))?;
        write_private(&run_dir.join(PID_FILE), &child.id().to_string())?;
        files.push(run_dir.join(PID_FILE));
        let stdout = child.stdout.take().ok_or("OpenVPN gave no output")?;
        let (sender, lines) = mpsc::channel::<String>();
        let logs = Arc::new(Mutex::new(Vec::new()));
        let kept = logs.clone();
        std::thread::spawn(move || {
            // Keeps reading after startup, when nobody listens any more: a closed
            // pipe would kill OpenVPN at its next log line.
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                kept.lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .push(line.clone());
                let _ = sender.send(line);
            }
        });
        let mut process = Self {
            child,
            interface: interface.to_owned(),
            files,
            logs,
        };
        let deadline = Instant::now() + STARTUP_TIMEOUT;
        loop {
            match lines.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(line) if opened_device(&line).is_some() => {
                    if let Some(device) = opened_device(&line) {
                        process.interface = device.to_owned();
                    }
                }
                Ok(line) if line.contains("Initialization Sequence Completed") => {
                    process.files.retain(|file| {
                        let login = file.extension().is_some_and(|ext| ext == "auth");
                        if login {
                            let _ = std::fs::remove_file(file);
                        }
                        !login
                    });
                    return Ok(process);
                }
                Ok(line) if line.contains("AUTH_FAILED") => {
                    return Err("The server refused the user name or password".to_owned());
                }
                Ok(_) => {}
                Err(_) => {
                    return Err("OpenVPN did not connect in time (wrong server address, or blocked network?)".to_owned());
                }
            }
        }
    }
}

impl Process {
    pub fn take_logs(&self) -> Vec<String> {
        std::mem::take(
            &mut *self
                .logs
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        )
    }

    /// Why OpenVPN ended, if it did while it should still be running.
    pub fn stopped(&mut self) -> Option<String> {
        match self.child.try_wait() {
            Ok(Some(status)) => Some(format!("OpenVPN ended ({status})")),
            _ => None,
        }
    }
}

fn write_private(path: &Path, text: &str) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .and_then(|mut file| file.write_all(text.as_bytes()))
        .map_err(|err| format!("{}: {err}", path.display()))
}

impl Drop for Process {
    fn drop(&mut self) {
        terminate(self.child.id());
        let deadline = Instant::now() + STOP_TIMEOUT;
        while Instant::now() < deadline && matches!(self.child.try_wait(), Ok(None)) {
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        for file in &self.files {
            let _ = std::fs::remove_file(file);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opened_device_is_read_from_the_log() {
        assert_eq!(
            opened_device("2026-10-31 01:30:00 Opened utun device utun4"),
            Some("utun4")
        );
        assert_eq!(opened_device("Initialization Sequence Completed"), None);
    }
}
