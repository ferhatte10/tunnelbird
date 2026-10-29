// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

//! Desktop portal calls, used when Tunnelbird runs inside a Flatpak sandbox,
//! where it cannot write the user's autostart folder itself.

use std::collections::HashMap;

use gettextrs::gettext;
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
