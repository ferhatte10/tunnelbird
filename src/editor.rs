// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

//! Tunnel editor: shown before an import, to check and adjust what was read
//! from the file, and to change an existing tunnel.
//!
//! Fields are plain text. On save they are written back in the `.conf` format
//! and parsed with [`config::parse`], so validation and messages are exactly
//! those of an imported file; each error is pinned to the field it comes from.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use futures_channel::oneshot;
use gettextrs::gettext;
use gtk::glib;

use crate::config::{self, ErrorKind, WgConfig};
use crate::share;
use crate::window::clickable;

pub enum Mode {
    Import,
    Edit,
}

pub struct Edited {
    pub name: String,
    pub config: WgConfig,
}

/// Every editable row. Rows keep their text; [`Fields::to_conf`] turns them back into a file.
struct Fields {
    name: adw::EntryRow,
    endpoint: adw::EntryRow,
    public_key: adw::EntryRow,
    preshared_key: adw::PasswordEntryRow,
    allowed_ips: adw::EntryRow,
    keepalive: adw::EntryRow,
    addresses: adw::EntryRow,
    dns: adw::EntryRow,
    private_key: adw::PasswordEntryRow,
    mtu: adw::EntryRow,
}

/// Opens the editor and waits for it to close. `None` when the user cancels.
pub async fn run(
    parent: &impl IsA<gtk::Widget>,
    mode: Mode,
    name: &str,
    original: &WgConfig,
    provider: Option<&str>,
) -> Option<Edited> {
    let editing = matches!(mode, Mode::Edit);
    let (title, action) = match mode {
        Mode::Import => (gettext("Import Tunnel"), gettext("_Import")),
        Mode::Edit => (gettext("Edit Tunnel"), gettext("_Save")),
    };
    let dialog = adw::Dialog::builder()
        .title(title)
        .content_width(560)
        .content_height(720)
        .build();

    let cancel = gtk::Button::with_mnemonic(&gettext("_Cancel"));
    let save = gtk::Button::builder()
        .label(action)
        .use_underline(true)
        .css_classes(["suggested-action"])
        .build();
    clickable(&cancel);
    clickable(&save);
    let header = adw::HeaderBar::builder()
        .show_start_title_buttons(false)
        .show_end_title_buttons(false)
        .build();
    header.pack_start(&cancel);
    header.pack_end(&save);

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    if !original.ignored.is_empty() {
        toolbar.add_top_bar(
            &adw::Banner::builder()
                .title(
                    gettext("Not supported, will be ignored: {keys}")
                        .replace("{keys}", &original.ignored.join(", ")),
                )
                .revealed(true)
                .build(),
        );
    }

    let first = original.peers.first();
    let entry = |title: String, text: String| {
        let row = adw::EntryRow::builder().title(title).text(text).build();
        // A field flagged by validation goes back to normal as soon as it is edited.
        row.connect_changed(|row| row.remove_css_class("error"));
        row
    };
    let secret = |title: String, text: String| {
        let row = adw::PasswordEntryRow::builder()
            .title(title)
            .text(text)
            .build();
        row.connect_changed(|row| row.remove_css_class("error"));
        row
    };
    let interface = &original.interface;
    let dns: Vec<String> = interface
        .dns_servers
        .iter()
        .map(ToString::to_string)
        .chain(interface.dns_search.iter().cloned())
        .collect();
    let fields = Rc::new(Fields {
        name: entry(gettext("Name"), name.to_owned()),
        endpoint: entry(
            gettext("Server Address"),
            first.and_then(|p| p.endpoint.clone()).unwrap_or_default(),
        ),
        public_key: entry(
            gettext("Server Public Key"),
            first.map(|p| p.public_key.clone()).unwrap_or_default(),
        ),
        preshared_key: secret(
            gettext("Preshared Key (optional)"),
            first
                .and_then(|p| p.preshared_key.clone())
                .unwrap_or_default(),
        ),
        allowed_ips: entry(
            gettext("Routed Networks"),
            first
                .map(|p| config::join_nets(&p.allowed_ips))
                .unwrap_or_default(),
        ),
        keepalive: entry(
            gettext("Keepalive in Seconds (optional)"),
            first
                .and_then(|p| p.persistent_keepalive)
                .map(|s| s.to_string())
                .unwrap_or_default(),
        ),
        addresses: entry(
            gettext("Addresses"),
            config::join_nets(&interface.addresses),
        ),
        dns: entry(gettext("DNS Servers"), dns.join(", ")),
        private_key: secret(gettext("Private Key"), interface.private_key.clone()),
        mtu: entry(
            gettext("MTU (optional)"),
            interface.mtu.map(|m| m.to_string()).unwrap_or_default(),
        ),
    });

    let page = adw::PreferencesPage::new();

    let general = adw::PreferencesGroup::new();
    general.add(&fields.name);
    general.add(
        &adw::ActionRow::builder()
            .title(gettext("Type"))
            .subtitle(config::TUNNEL_TYPE)
            .css_classes(["property"])
            .build(),
    );
    general.add(
        &adw::ActionRow::builder()
            .title(gettext("Provider"))
            .subtitle(
                provider
                    .map(str::to_owned)
                    .unwrap_or_else(|| gettext("Custom server")),
            )
            .css_classes(["property"])
            .build(),
    );
    let traffic = adw::ActionRow::builder()
        .title(gettext("Traffic"))
        .css_classes(["property"])
        .build();
    update_traffic(&traffic, &fields.allowed_ips.text());
    let traffic_row = traffic.clone();
    fields
        .allowed_ips
        .connect_changed(move |row| update_traffic(&traffic_row, &row.text()));
    general.add(&traffic);
    page.add(&general);

    let server = adw::PreferencesGroup::builder()
        .title(gettext("Server"))
        .build();
    server.add(&fields.endpoint);
    server.add(&fields.public_key);
    server.add(&fields.preshared_key);
    server.add(&fields.allowed_ips);
    server.add(&fields.keepalive);
    if original.peers.len() > 1 {
        server.set_description(Some(
            &gettext(
                "This tunnel has {count} servers. Only the first one is shown; the others are kept as they are.",
            )
            .replace("{count}", &original.peers.len().to_string()),
        ));
    }
    page.add(&server);

    let device = adw::PreferencesGroup::builder()
        .title(gettext("This Computer"))
        .description(gettext(
            "Addresses and keys of this computer inside the VPN",
        ))
        .build();
    device.add(&fields.addresses);
    device.add(&fields.dns);
    device.add(&fields.private_key);
    device.add(&fields.mtu);
    page.add(&device);

    // Validation errors show right under the header, where they cannot be missed.
    let error = adw::Banner::builder()
        .use_markup(false)
        .css_classes(["tunnel-error"])
        .build();
    toolbar.add_top_bar(&error);
    if editing {
        page.add(&share_group(parent, &dialog, &fields, original, &error));
    }
    toolbar.set_content(Some(&page));
    dialog.set_child(Some(&toolbar));
    dialog.set_default_widget(Some(&save));

    let (sender, receiver) = oneshot::channel::<Edited>();
    let sender = Rc::new(RefCell::new(Some(sender)));

    let weak = dialog.downgrade();
    cancel.connect_clicked(move |_| {
        if let Some(dialog) = weak.upgrade() {
            dialog.close();
        }
    });

    let weak = dialog.downgrade();
    let original = original.clone();
    save.connect_clicked(move |_| {
        let Some(dialog) = weak.upgrade() else { return };
        match fields.validate(&original) {
            Ok(edited) => {
                if let Some(sender) = sender.borrow_mut().take() {
                    // The receiver only disappears if the caller gave up waiting.
                    let _ = sender.send(edited);
                }
                dialog.close();
            }
            Err(message) => show_error(&error, &message),
        }
    });

    dialog.present(Some(parent));
    // Closing without saving drops the sender, which resolves the wait with `None`.
    receiver.await.ok()
}

/// Adds `key = value` to the file being written, unless the field is empty.
fn push_field(
    lines: &mut Vec<(String, Option<adw::PreferencesRow>)>,
    key: &str,
    row: &adw::PreferencesRow,
    value: String,
) {
    let value = value.trim();
    if !value.is_empty() {
        lines.push((format!("{key} = {value}"), Some(row.clone())));
    }
}

/// "Export as File…" and "Show QR Code", from the fields as currently edited.
fn share_group(
    parent: &impl IsA<gtk::Widget>,
    dialog: &adw::Dialog,
    fields: &Rc<Fields>,
    original: &WgConfig,
    error: &adw::Banner,
) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title(gettext("Share"))
        .description(gettext(
            "Both include the private key of this tunnel: keep them to yourself.",
        ))
        .build();
    let row = |title: String, icon: &str| {
        let row = adw::ActionRow::builder()
            .title(title)
            .activatable(true)
            .build();
        row.add_prefix(&gtk::Image::from_icon_name(icon));
        row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
        clickable(&row);
        group.add(&row);
        row
    };
    let export = row(gettext("Export as File…"), "document-save-symbolic");
    let qr = row(gettext("Show QR Code"), "phone-symbolic");

    let window = parent.root().and_downcast::<gtk::Window>();
    let (fields_c, original_c, error_c) = (fields.clone(), original.clone(), error.clone());
    export.connect_activated(move |row| {
        let edited = match fields_c.validate(&original_c) {
            Ok(edited) => edited,
            Err(message) => return show_error(&error_c, &message),
        };
        let Some(window) = window.clone() else { return };
        let (row, error) = (row.clone(), error_c.clone());
        glib::spawn_future_local(async move {
            match share::export_file(&window, &edited.name, &edited.config).await {
                Ok(Some(path)) => {
                    row.set_subtitle(&gettext("Saved to {path}").replace("{path}", &path))
                }
                Ok(None) => {}
                Err(message) => show_error(&error, &message),
            }
        });
    });

    let (fields_c, original_c, error_c) = (fields.clone(), original.clone(), error.clone());
    let dialog = dialog.downgrade();
    qr.connect_activated(move |_| {
        let edited = match fields_c.validate(&original_c) {
            Ok(edited) => edited,
            Err(message) => return show_error(&error_c, &message),
        };
        if let Some(dialog) = dialog.upgrade() {
            share::show_qr(&dialog, &edited.name, &edited.config);
        }
    });
    group
}

fn show_error(banner: &adw::Banner, message: &str) {
    banner.set_title(message);
    banner.set_revealed(true);
}

fn update_traffic(row: &adw::ActionRow, allowed_ips: &str) {
    let full = allowed_ips
        .split([',', ' '])
        .any(|net| net.trim().ends_with("/0"));
    row.set_subtitle(&if full {
        gettext("All internet traffic goes through the VPN")
    } else {
        gettext("Only the routed networks go through the VPN")
    });
}

impl Fields {
    /// Writes the fields as a `.conf` file, remembering which row each line comes from.
    fn to_conf(&self, original: &WgConfig) -> (String, Vec<Option<adw::PreferencesRow>>) {
        let mut lines: Vec<(String, Option<adw::PreferencesRow>)> = Vec::new();
        let text = |row: &adw::EntryRow| row.text().to_string();
        let secret = |row: &adw::PasswordEntryRow| row.text().to_string();

        lines.push(("[Interface]".to_owned(), None));
        push_field(
            &mut lines,
            "PrivateKey",
            self.private_key.upcast_ref(),
            secret(&self.private_key),
        );
        push_field(
            &mut lines,
            "Address",
            self.addresses.upcast_ref(),
            text(&self.addresses),
        );
        push_field(&mut lines, "DNS", self.dns.upcast_ref(), text(&self.dns));
        push_field(&mut lines, "MTU", self.mtu.upcast_ref(), text(&self.mtu));
        let interface = &original.interface;
        if let Some(port) = interface.listen_port {
            lines.push((format!("ListenPort = {port}"), None));
        }
        if let Some(mark) = interface.fwmark {
            lines.push((format!("FwMark = {mark}"), None));
        }
        lines.push(("[Peer]".to_owned(), None));
        push_field(
            &mut lines,
            "PublicKey",
            self.public_key.upcast_ref(),
            text(&self.public_key),
        );
        push_field(
            &mut lines,
            "PresharedKey",
            self.preshared_key.upcast_ref(),
            secret(&self.preshared_key),
        );
        push_field(
            &mut lines,
            "Endpoint",
            self.endpoint.upcast_ref(),
            text(&self.endpoint),
        );
        push_field(
            &mut lines,
            "AllowedIPs",
            self.allowed_ips.upcast_ref(),
            text(&self.allowed_ips),
        );
        push_field(
            &mut lines,
            "PersistentKeepalive",
            self.keepalive.upcast_ref(),
            text(&self.keepalive),
        );
        for peer in original.peers.iter().skip(1) {
            for line in peer.to_conf().lines() {
                lines.push((line.to_owned(), None));
            }
        }
        let rows = lines.iter().map(|(_, row)| row.clone()).collect();
        let text = lines
            .into_iter()
            .map(|(line, _)| line)
            .collect::<Vec<_>>()
            .join("\n");
        (text, rows)
    }

    /// Checks the fields; on error, flags the faulty row and returns the message.
    fn validate(&self, original: &WgConfig) -> Result<Edited, String> {
        let name = self.name.text().trim().to_owned();
        if name.is_empty() {
            return Err(self.flag(
                self.name.upcast_ref(),
                gettext("Enter a name for this tunnel"),
            ));
        }
        if self.endpoint.text().trim().is_empty() {
            return Err(self.flag(
                self.endpoint.upcast_ref(),
                gettext("Enter the server address, for example vpn.example.com:51820"),
            ));
        }
        let (text, rows) = self.to_conf(original);
        match config::parse(&text) {
            Ok(config) => Ok(Edited { name, config }),
            Err(err) => {
                let row = match (&err.kind, err.line) {
                    (ErrorKind::NoPrivateKey, _) => Some(self.private_key.clone().upcast()),
                    (ErrorKind::PeerWithoutPublicKey, _) => Some(self.public_key.clone().upcast()),
                    (_, Some(line)) => rows.get(line - 1).cloned().flatten(),
                    _ => None,
                };
                // Line numbers mean nothing here: the faulty field is highlighted instead.
                let message = err.message();
                Err(match row {
                    Some(row) => self.flag(&row, message),
                    None => message,
                })
            }
        }
    }

    fn flag(&self, row: &adw::PreferencesRow, message: String) -> String {
        row.add_css_class("error");
        row.grab_focus();
        message
    }
}
