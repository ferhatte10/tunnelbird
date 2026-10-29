// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

//! Tunnel editor: shown before an import, to check and adjust what was read
//! from the file, and to change an existing tunnel.
//!
//! The dialog is shared; its form depends on the kind of tunnel:
//! - WireGuard®: fields are plain text. On save they are written back in the
//!   `.conf` format and parsed with [`config::parse`], so validation and
//!   messages are exactly those of an imported file; each error is pinned to the
//!   field it comes from.
//! - OpenVPN: name, server, protocol and login are editable; certificates and
//!   encryption are shown, kept as the provider set them.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use futures_channel::oneshot;
use gettextrs::gettext;
use gtk::glib;

use crate::config::{self, ErrorKind, WgConfig};
use crate::ovpn::{OvpnConfig, Proto, Remote, Source};
use crate::profile::{Kind, Profile};
use crate::share;
use crate::window::clickable;

pub enum Mode {
    Import,
    Edit,
}

pub struct Edited {
    pub name: String,
    pub profile: Profile,
}

/// Turns the form's fields into a profile, or says what is wrong.
type Validate = Rc<dyn Fn() -> Result<Edited, String>>;

/// What a kind of tunnel adds to the dialog.
struct Form {
    page: adw::PreferencesPage,
    validate: Validate,
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
    original: &Profile,
    provider: Option<&str>,
) -> Option<Edited> {
    let editing = matches!(mode, Mode::Edit);
    let (title, action) = match mode {
        Mode::Import => (gettext("Import Tunnel"), gettext("_Import")),
        Mode::Edit => (gettext("Edit Tunnel"), gettext("_Save")),
    };
    let form = match original {
        Profile::WireGuard(config) => wireguard_form(name, config, provider),
        Profile::OpenVpn(config) => openvpn_form(name, config, provider),
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
    if !original.ignored().is_empty() {
        toolbar.add_top_bar(
            &adw::Banner::builder()
                .title(
                    gettext("Not supported, will be ignored: {keys}")
                        .replace("{keys}", &original.ignored().join(", ")),
                )
                .revealed(true)
                .build(),
        );
    }

    // Validation errors show right under the header, where they cannot be missed.
    let error = adw::Banner::builder()
        .use_markup(false)
        .css_classes(["tunnel-error"])
        .build();
    toolbar.add_top_bar(&error);
    if editing {
        form.page.add(&share_group(
            parent,
            &dialog,
            original.kind(),
            &form.validate,
            &error,
        ));
    }
    toolbar.set_content(Some(&form.page));
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
    let validate = form.validate.clone();
    save.connect_clicked(move |_| {
        let Some(dialog) = weak.upgrade() else { return };
        match validate() {
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

fn entry(title: String, text: String) -> adw::EntryRow {
    let row = adw::EntryRow::builder().title(title).text(text).build();
    // A field flagged by validation goes back to normal as soon as it is edited.
    row.connect_changed(|row| row.remove_css_class("error"));
    row
}

fn secret(title: String, text: String) -> adw::PasswordEntryRow {
    let row = adw::PasswordEntryRow::builder()
        .title(title)
        .text(text)
        .build();
    row.connect_changed(|row| row.remove_css_class("error"));
    row
}

fn property(title: String, value: &str) -> adw::ActionRow {
    adw::ActionRow::builder()
        .title(title)
        .subtitle(value)
        .css_classes(["property"])
        .build()
}

fn provider_row(provider: Option<&str>) -> adw::ActionRow {
    property(
        gettext("Provider"),
        &provider
            .map(str::to_owned)
            .unwrap_or_else(|| gettext("Custom server")),
    )
}

fn wireguard_form(name: &str, original: &WgConfig, provider: Option<&str>) -> Form {
    let first = original.peers.first();
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
    general.add(&property(gettext("Type"), config::TUNNEL_TYPE));
    general.add(&provider_row(provider));
    let traffic = property(gettext("Traffic"), "");
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

    let original = original.clone();
    Form {
        page,
        validate: Rc::new(move || {
            fields.validate(&original).map(|(name, config)| Edited {
                name,
                profile: Profile::WireGuard(config),
            })
        }),
    }
}

fn openvpn_form(name: &str, original: &OvpnConfig, provider: Option<&str>) -> Form {
    let name_row = entry(gettext("Name"), name.to_owned());
    let server_row = entry(
        gettext("Server Address"),
        original.endpoint().unwrap_or_default(),
    );
    let first_proto = original
        .remotes
        .first()
        .and_then(|r| r.proto)
        .unwrap_or(original.proto);
    let proto_row = adw::ComboRow::builder()
        .title(gettext("Protocol"))
        .model(&gtk::StringList::new(&["UDP", "TCP"]))
        .selected(u32::from(first_proto == Proto::Tcp))
        .build();
    clickable(&proto_row);
    let username_row = entry(gettext("User Name"), original.username.clone());
    let password_row = secret(gettext("Password (optional)"), original.password.clone());

    let page = adw::PreferencesPage::new();
    let general = adw::PreferencesGroup::new();
    general.add(&name_row);
    general.add(&property(gettext("Type"), crate::ovpn::TUNNEL_TYPE));
    general.add(&provider_row(provider));
    general.add(&property(
        gettext("Traffic"),
        &if original.redirect_gateway {
            gettext("All internet traffic goes through the VPN")
        } else {
            gettext("Decided by the server when connecting")
        },
    ));
    page.add(&general);

    let server = adw::PreferencesGroup::builder()
        .title(gettext("Server"))
        .build();
    server.add(&server_row);
    server.add(&proto_row);
    if original.remotes.len() > 1 {
        server.set_description(Some(
            &gettext(
                "This profile has {count} servers. Only the first one is shown; the others are kept and tried in turn.",
            )
            .replace("{count}", &original.remotes.len().to_string()),
        ));
    }
    page.add(&server);

    if original.user_pass {
        let login = adw::PreferencesGroup::builder()
            .title(gettext("Login"))
            .description(gettext(
                "Leave the password empty to be asked at the first connection: your desktop's keyring then keeps it.",
            ))
            .build();
        login.add(&username_row);
        login.add(&password_row);
        page.add(&login);
    }

    let included = |source: &Option<Source>| match source {
        Some(_) => gettext("Included"),
        None => gettext("Not included"),
    };
    let security = adw::PreferencesGroup::builder()
        .title(gettext("Security"))
        .description(gettext("Set by your provider and kept as they are"))
        .build();
    security.add(&property(
        gettext("Server Certificate"),
        &included(&original.ca),
    ));
    security.add(&property(
        gettext("Client Certificate"),
        &if original.cert.is_some() {
            gettext("Included")
        } else {
            gettext("None: user name and password")
        },
    ));
    security.add(&property(
        gettext("Encryption"),
        &original
            .data_ciphers
            .clone()
            .or_else(|| original.cipher.clone())
            .unwrap_or_else(|| gettext("Agreed with the server")),
    ));
    security.add(&property(
        gettext("Extra Protection"),
        &if original.tls_crypt.is_some() || original.tls_crypt_v2.is_some() {
            gettext("Encrypted control channel (tls-crypt)")
        } else if original.tls_auth.is_some() {
            gettext("Signed control channel (tls-auth)")
        } else {
            gettext("None")
        },
    ));
    page.add(&security);

    let original = original.clone();
    let flag = |row: &adw::PreferencesRow, message: String| {
        row.add_css_class("error");
        row.grab_focus();
        message
    };
    Form {
        page,
        validate: Rc::new(move || {
            let name = name_row.text().trim().to_owned();
            if name.is_empty() {
                return Err(flag(
                    name_row.upcast_ref(),
                    gettext("Enter a name for this tunnel"),
                ));
            }
            let Some((host, port)) = parse_server(&server_row.text()) else {
                return Err(flag(
                    server_row.upcast_ref(),
                    gettext("Enter the server address, for example vpn.example.com:1194"),
                ));
            };
            let username = username_row.text().trim().to_owned();
            if original.user_pass && username.is_empty() {
                return Err(flag(
                    username_row.upcast_ref(),
                    gettext("Enter the user name given by your provider"),
                ));
            }
            let mut config = original.clone();
            let proto = if proto_row.selected() == 1 {
                Proto::Tcp
            } else {
                Proto::Udp
            };
            let first = Remote {
                host,
                port,
                proto: Some(proto),
            };
            match config.remotes.first_mut() {
                Some(remote) => *remote = first,
                None => config.remotes.push(first),
            }
            config.username = username;
            config.password = password_row.text().to_string();
            Ok(Edited {
                name,
                profile: Profile::OpenVpn(Box::new(config)),
            })
        }),
    }
}

/// `vpn.example.com`, `vpn.example.com:443` → host and port.
fn parse_server(text: &str) -> Option<(String, Option<u16>)> {
    let text = text.trim();
    let (host, port) = match text.rsplit_once(':') {
        Some((host, port)) if port.chars().all(|c| c.is_ascii_digit()) => {
            (host, Some(port.parse().ok()?))
        }
        _ => (text, None),
    };
    (!host.is_empty() && !host.contains(char::is_whitespace)).then(|| (host.to_owned(), port))
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

/// "Export as File…" and, for WireGuard®, "Show QR Code", from the fields as
/// currently edited.
fn share_group(
    parent: &impl IsA<gtk::Widget>,
    dialog: &adw::Dialog,
    kind: Kind,
    validate: &Validate,
    error: &adw::Banner,
) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title(gettext("Share"))
        .description(gettext(
            "Shared tunnels include the private key: keep them to yourself.",
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

    let window = parent.root().and_downcast::<gtk::Window>();
    let (validate_c, error_c) = (validate.clone(), error.clone());
    export.connect_activated(move |row| {
        let edited = match validate_c() {
            Ok(edited) => edited,
            Err(message) => return show_error(&error_c, &message),
        };
        let Some(window) = window.clone() else { return };
        let (row, error) = (row.clone(), error_c.clone());
        glib::spawn_future_local(async move {
            match share::export_file(&window, &edited.name, &edited.profile).await {
                Ok(Some(path)) => {
                    row.set_subtitle(&gettext("Saved to {path}").replace("{path}", &path))
                }
                Ok(None) => {}
                Err(message) => show_error(&error, &message),
            }
        });
    });

    if kind.has_qr_code() {
        let qr = row(gettext("Show QR Code"), "phone-symbolic");
        let (validate_c, error_c) = (validate.clone(), error.clone());
        let dialog = dialog.downgrade();
        qr.connect_activated(move |_| {
            let edited = match validate_c() {
                Ok(edited) => edited,
                Err(message) => return show_error(&error_c, &message),
            };
            if let (Some(dialog), Profile::WireGuard(config)) = (dialog.upgrade(), &edited.profile)
            {
                share::show_qr(&dialog, &edited.name, config);
            }
        });
    }
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
    fn validate(&self, original: &WgConfig) -> Result<(String, WgConfig), String> {
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
            Ok(config) => Ok((name, config)),
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
