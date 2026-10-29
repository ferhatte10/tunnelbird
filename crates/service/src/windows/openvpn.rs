// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

//! OpenVPN on Windows: the community `openvpn.exe` (shipped in the service's
//! `openvpn` folder) with the ovpn-dco-win kernel driver. OpenVPN sets the
//! addresses, routes and DNS the server pushes; the service starts it, waits
//! for “Initialization Sequence Completed” and reads the adapter's counters.
//!
//! The profile and the login go to a folder only SYSTEM and administrators
//! can read, and the login file is deleted as soon as OpenVPN has read it.
//!
//! NOT YET RUN ON WINDOWS (docs/WINDOWS.md).

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use windows_sys::Win32::NetworkManagement::IpHelper::ConvertInterfaceAliasToLuid;
use windows_sys::Win32::NetworkManagement::Ndis::NET_LUID_LH;

use tunnelbird_core::protocol::Login;

use super::wide;

const STARTUP_TIMEOUT: Duration = Duration::from_secs(45);

pub struct Process {
    child: Child,
    adapter: String,
    files: Vec<PathBuf>,
}

impl Process {
    /// Starts OpenVPN with `profile` (certificates inline) on an adapter
    /// named `adapter`, and waits until the tunnel is up.
    pub fn start(
        run_dir: &Path,
        id: &str,
        adapter: &str,
        profile: &str,
        login: Option<&Login>,
    ) -> Result<Self, String> {
        let exe = std::env::current_exe()
            .map_err(|err| err.to_string())?
            .with_file_name("openvpn")
            .join("openvpn.exe");
        if !exe.exists() {
            return Err(
                "OpenVPN is missing from Tunnelbird's installation (openvpn\\openvpn.exe)"
                    .to_owned(),
            );
        }
        let config = run_dir.join(format!("{id}.ovpn"));
        std::fs::write(&config, profile).map_err(|err| format!("{}: {err}", config.display()))?;
        let mut files = vec![config.clone()];
        let mut command = Command::new(exe);
        command
            .arg("--config")
            .arg(&config)
            .args([
                "--windows-driver",
                "ovpn-dco",
                "--dev-node",
                adapter,
                "--verb",
                "3",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if let Some(login) = login {
            let auth = run_dir.join(format!("{id}.auth"));
            std::fs::write(&auth, format!("{}\n{}\n", login.username, login.password))
                .map_err(|err| format!("{}: {err}", auth.display()))?;
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

        // OpenVPN's log, read on a thread: its lines decide when it is up.
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
            adapter: adapter.to_owned(),
            files,
        };
        let deadline = std::time::Instant::now() + STARTUP_TIMEOUT;
        loop {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            match lines.recv_timeout(left) {
                Ok(line) if line.contains("Initialization Sequence Completed") => {
                    process.forget_login();
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

    /// The login file is only needed until OpenVPN has read it.
    fn forget_login(&mut self) {
        self.files.retain(|file| {
            if file.extension().is_some_and(|ext| ext == "auth") {
                let _ = std::fs::remove_file(file);
                false
            } else {
                true
            }
        });
    }

    pub fn luid(&self) -> Option<NET_LUID_LH> {
        let alias = wide(&self.adapter);
        // SAFETY: output written by the API.
        let mut luid: NET_LUID_LH = unsafe { std::mem::zeroed() };
        (unsafe { ConvertInterfaceAliasToLuid(alias.as_ptr(), &mut luid) } == 0).then_some(luid)
    }
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
