// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

//! OpenVPN on macOS: the community `openvpn` program on a `utun` interface.
//! It sets the addresses and routes the server pushes. The helper looks for
//! it inside the app bundle first, then where Homebrew installs it.
//!
//! The profile and login go to a folder only root can read; the login file
//! is deleted as soon as OpenVPN has read it.
//!
//! NOT YET RUN ON macOS (docs/MACOS.md).

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use tunnelbird_core::protocol::Login;

const STARTUP_TIMEOUT: Duration = Duration::from_secs(45);
const CANDIDATES: [&str; 3] = [
    "/Applications/Tunnelbird.app/Contents/Resources/openvpn",
    "/opt/homebrew/sbin/openvpn",
    "/usr/local/sbin/openvpn",
];

pub struct Process {
    child: Child,
    pub interface: String,
    files: Vec<PathBuf>,
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
        let stdout = child.stdout.take().ok_or("OpenVPN gave no output")?;
        let (sender, lines) = mpsc::channel::<String>();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        let mut process = Self {
            child,
            interface: interface.to_owned(),
            files,
        };
        let deadline = Instant::now() + STARTUP_TIMEOUT;
        loop {
            match lines.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
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
        let _ = self.child.kill();
        let _ = self.child.wait();
        for file in &self.files {
            let _ = std::fs::remove_file(file);
        }
    }
}
