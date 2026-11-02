// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

//! Sharing a tunnel: export as a `.conf` or `.ovpn` file, or show a WireGuard®
//! tunnel as a QR code that phone apps can scan (like web dashboards for
//! self-hosted servers do).
//!
//! Both contain the tunnel's private key: the file is written readable by its
//! owner only, and the QR dialog says plainly who must not see it. An OpenVPN
//! password is never exported.

use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;

use adw::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};
use qrcode::{Color, EcLevel, QrCode};

use crate::LOG_DOMAIN;
use crate::config::WgConfig;
use crate::profile::{Kind, Profile};

/// Asks where to save the tunnel and writes it. Returns the saved path, or the
/// error to show; `Ok(None)` when the user cancelled.
pub async fn export_file(
    parent: &gtk::Window,
    name: &str,
    profile: &Profile,
) -> Result<Option<String>, String> {
    let extension = profile.kind().extension();
    let filter = gtk::FileFilter::new();
    filter.set_name(Some(&match profile.kind() {
        Kind::WireGuard => gettext("Tunnel Configuration"),
        Kind::OpenVpn => gettext("OpenVPN Profile"),
    }));
    filter.add_pattern(&format!("*.{extension}"));
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);
    let dialog = gtk::FileDialog::builder()
        .title(gettext("Export Tunnel"))
        .initial_name(format!("{}.{extension}", file_stem(name)))
        .initial_folder(&start_folder())
        .filters(&filters)
        .modal(true)
        .build();
    let file = match dialog.save_future(Some(parent)).await {
        Ok(file) => file,
        Err(err) if err.matches(gtk::DialogError::Dismissed) => return Ok(None),
        Err(err) => return Err(err.to_string()),
    };
    let Some(path) = file.path() else {
        return Err(gettext("This location cannot be written to"));
    };
    // 0600: the file holds the private key.
    let written = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&path)
        .and_then(|mut out| out.write_all(profile.to_text().as_bytes()));
    match written {
        Ok(()) => {
            glib::g_info!(LOG_DOMAIN, "Tunnel “{name}” exported to {}", path.display());
            Ok(Some(path.display().to_string()))
        }
        Err(err) => Err(format!("{}: {err}", path.display())),
    }
}

/// Shows the tunnel as a QR code for a phone to scan.
pub fn show_qr(parent: &impl IsA<gtk::Widget>, name: &str, config: &WgConfig) {
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(18)
        .margin_top(12)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .build();

    let warning = gtk::Label::builder()
        .label(gettext(
            "This code contains your private key: anyone who scans it can use this tunnel. Show it only to your own devices.",
        ))
        .wrap(true)
        .justify(gtk::Justification::Center)
        .css_classes(["warning"])
        .build();
    content.append(&warning);

    match QrCode::with_error_correction_level(config.to_conf(), EcLevel::M) {
        Ok(code) => content.append(&qr_widget(&code, name)),
        Err(err) => {
            glib::g_warning!(LOG_DOMAIN, "Cannot build the QR code: {err}");
            content.append(
                &gtk::Label::builder()
                    .label(gettext(
                        "This tunnel is too large for a QR code. Export it as a file instead.",
                    ))
                    .wrap(true)
                    .css_classes(["error"])
                    .build(),
            );
        }
    }

    content.append(
        &gtk::Label::builder()
            .label(gettext(
                "On your phone, open your VPN app, add a tunnel and choose to scan a QR code.",
            ))
            .wrap(true)
            .justify(gtk::Justification::Center)
            .css_classes(["dim-label"])
            .build(),
    );

    let header = adw::HeaderBar::new();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&content));
    let dialog = adw::Dialog::builder()
        .title(gettext("Scan with Your Phone"))
        .content_width(420)
        .child(&toolbar)
        .build();
    dialog.present(Some(parent));
}

/// Black modules on a white square with the standard quiet zone, whatever the
/// light or dark style: phone cameras need that contrast.
fn qr_widget(code: &QrCode, name: &str) -> gtk::DrawingArea {
    const QUIET_ZONE: usize = 4;
    let width = code.width();
    let modules = code.to_colors();
    let area = gtk::DrawingArea::builder()
        .content_width(320)
        .content_height(320)
        .halign(gtk::Align::Center)
        .build();
    area.update_property(&[gtk::accessible::Property::Label(
        &gettext("QR code of “{name}”").replace("{name}", name),
    )]);
    area.set_draw_func(move |_, cr, w, h| {
        let side = f64::from(w.min(h));
        let cells = (width + 2 * QUIET_ZONE) as f64;
        let cell = (side / cells).floor().max(1.0);
        let offset_x = ((f64::from(w) - cell * cells) / 2.0).floor();
        let offset_y = ((f64::from(h) - cell * cells) / 2.0).floor();
        cr.set_source_rgb(1.0, 1.0, 1.0);
        cr.rectangle(offset_x, offset_y, cell * cells, cell * cells);
        if let Err(err) = cr.fill() {
            glib::g_warning!(LOG_DOMAIN, "Cannot draw the QR code: {err}");
            return;
        }
        cr.set_source_rgb(0.0, 0.0, 0.0);
        for (index, color) in modules.iter().enumerate() {
            if *color == Color::Dark {
                let x = (index % width + QUIET_ZONE) as f64;
                let y = (index / width + QUIET_ZONE) as f64;
                cr.rectangle(offset_x + x * cell, offset_y + y * cell, cell, cell);
            }
        }
        if let Err(err) = cr.fill() {
            glib::g_warning!(LOG_DOMAIN, "Cannot draw the QR code: {err}");
        }
    });
    area
}

/// Where file choosers open. Left alone, the desktop's chooser reopens the
/// last folder used by any app, and shows an error if it no longer exists.
pub fn start_folder() -> gio::File {
    gio::File::for_path(glib::home_dir())
}

/// A file name from the tunnel name, without characters file systems dislike.
pub fn file_stem(name: &str) -> String {
    let stem: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || "-_.".contains(c) {
                c
            } else {
                '-'
            }
        })
        .collect();
    if stem.trim_matches('-').is_empty() {
        "tunnel".to_owned()
    } else {
        stem
    }
}
