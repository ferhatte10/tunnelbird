// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

//! Desktop portal calls, used when Tunnelbird runs inside a Flatpak sandbox,
//! where it cannot write the user's autostart folder or restart itself.

use std::collections::HashMap;

use gettextrs::gettext;
use gtk::glib;
use zbus::Variant;

/// True inside a Flatpak sandbox.
pub fn in_flatpak() -> bool {
    std::path::Path::new("/.flatpak-info").exists()
}

/// Asks the Background portal to start Tunnelbird at login (hidden in the top
/// bar), or to stop doing so. The desktop may ask the user to confirm.
pub async fn request_autostart(enabled: bool) -> zbus::Result<()> {
    let conn = zbus::Connection::session().await?;
    let options: HashMap<&str, Variant<'_>> = HashMap::from([
        (
            "reason",
            Variant::from(gettext(
                "Start Tunnelbird hidden in the top bar when you log in",
            )),
        ),
        ("autostart", Variant::from(enabled)),
        (
            "commandline",
            Variant::from(vec!["tunnelbird", "--background"]),
        ),
        ("dnd-activatable", Variant::from(false)),
    ]);
    conn.call_method(
        Some("org.freedesktop.portal.Desktop"),
        "/org/freedesktop/portal/desktop",
        Some("org.freedesktop.portal.Background"),
        "RequestBackground",
        &("", options),
    )
    .await?;
    Ok(())
}

/// Starts the newest installed version of Tunnelbird in a new sandbox (after
/// an update): restarting inside this sandbox would run the old files again.
pub async fn spawn_latest_version() -> zbus::Result<()> {
    /// `FLATPAK_SPAWN_FLAGS_LATEST_VERSION`.
    const LATEST_VERSION: u32 = 2;
    let conn = zbus::Connection::session().await?;
    let home = glib::home_dir();
    let mut cwd = home.as_os_str().as_encoded_bytes().to_vec();
    cwd.push(0);
    let argv = vec![b"tunnelbird\0".to_vec()];
    let fds: HashMap<u32, zbus::OwnedFd> = HashMap::new();
    let envs: HashMap<&str, &str> = HashMap::new();
    let options: HashMap<&str, Variant<'_>> = HashMap::new();
    conn.call_method(
        Some("org.freedesktop.portal.Flatpak"),
        "/org/freedesktop/portal/Flatpak",
        Some("org.freedesktop.portal.Flatpak"),
        "Spawn",
        &(cwd, argv, fds, envs, LATEST_VERSION, options),
    )
    .await?;
    Ok(())
}
