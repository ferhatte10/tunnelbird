// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

//! Preferences dialog, in two pages:
//! - General: style, language, background mode, start at login, update checks;
//! - Automatic Connection: trusted networks (see `trust.rs`).
//!
//! Every change is saved at once. The style applies immediately; the language
//! needs a restart, because labels already on screen keep the text they were
//! created with: the dialog offers a "Restart" button (see [`crate::request_restart`]).

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gettextrs::gettext;
use gtk::glib;

use crate::LOG_DOMAIN;
use crate::settings::{self, LANGUAGES, Settings, Style};
use crate::trust::{Network, NetworkKind, Pin, Policy, normalize_bssid};
use crate::update::Install;
use crate::window::clickable;

/// What the dialog needs to know about the app's current state.
pub struct Context {
    /// Saved tunnels as `(uuid, name)`, for the automatic connection choice.
    pub tunnels: Vec<(String, String)>,
    /// The network the computer is on, to offer "Trust this network".
    pub network: Option<Network>,
    /// Wi-Fi networks saved on this computer: trusting is picking from them,
    /// so a typo can never create a rule that never applies.
    pub saved_wifi: Vec<String>,
}

pub fn present(app: &adw::Application, context: Context) {
    let dialog = adw::PreferencesDialog::new();
    dialog.add(&general_page(app, &dialog));
    dialog.add(&connection_page(&dialog, context));
    dialog.present(app.active_window().as_ref());
}

fn general_page(app: &adw::Application, dialog: &adw::PreferencesDialog) -> adw::PreferencesPage {
    let current = Settings::load();
    let page = adw::PreferencesPage::builder()
        .title(gettext("General"))
        .icon_name("preferences-system-symbolic")
        .build();

    let appearance = adw::PreferencesGroup::builder()
        .title(gettext("Appearance"))
        .build();
    let style_names = [gettext("Follow System"), gettext("Light"), gettext("Dark")];
    let style_row = adw::ComboRow::builder()
        .title(gettext("Theme"))
        .model(&gtk::StringList::new(
            &style_names.each_ref().map(String::as_str),
        ))
        .selected(
            Style::ALL
                .iter()
                .position(|s| *s == current.style)
                .unwrap_or(0) as u32,
        )
        .build();
    clickable(&style_row);
    appearance.add(&style_row);
    page.add(&appearance);

    let behaviour = adw::PreferencesGroup::builder()
        .title(gettext("Behavior"))
        .build();
    let background_row = adw::SwitchRow::builder()
        .title(gettext("Run in Background"))
        .subtitle(gettext(
            "Keep Tunnelbird in the top bar when the window is closed",
        ))
        .active(current.background)
        .build();
    clickable(&background_row);
    behaviour.add(&background_row);
    let login_row = adw::SwitchRow::builder()
        .title(gettext("Start at Login"))
        .subtitle(gettext(
            "Starts hidden in the top bar, ready to protect you",
        ))
        .active(settings::autostart_enabled())
        .build();
    clickable(&login_row);
    behaviour.add(&login_row);
    // Flatpak and Snap installs are told about updates by their store.
    let updates_row = adw::SwitchRow::builder()
        .title(gettext("Check for Updates"))
        .subtitle(gettext(
            "Once a day, asks tunnelbird.ferhatsn.fr for the latest version number. Nothing about you is sent.",
        ))
        .active(current.check_updates)
        .visible(Install::current().checks_website())
        .build();
    clickable(&updates_row);
    behaviour.add(&updates_row);
    page.add(&behaviour);

    let language = adw::PreferencesGroup::builder()
        .title(gettext("Language"))
        .build();
    let system_language = gettext("System Language ({language})")
        .replace("{language}", settings::system_language_name());
    let mut language_names = vec![system_language.as_str()];
    language_names.extend(LANGUAGES.iter().map(|(_, name)| *name));
    let language_row = adw::ComboRow::builder()
        .title(gettext("Interface Language"))
        .subtitle(gettext("Tunnelbird restarts to apply a new language"))
        .model(&gtk::StringList::new(&language_names))
        .selected(
            LANGUAGES
                .iter()
                .position(|(code, _)| *code == current.language)
                .map_or(0, |i| i as u32 + 1),
        )
        .build();
    clickable(&language_row);
    language.add(&language_row);
    page.add(&language);

    let weak = dialog.downgrade();
    background_row.connect_active_notify(move |row| {
        update(weak.upgrade().as_ref(), |s| s.background = row.is_active());
    });

    let weak = dialog.downgrade();
    updates_row.connect_active_notify(move |row| {
        update(weak.upgrade().as_ref(), |s| {
            s.check_updates = row.is_active()
        });
    });

    let weak = dialog.downgrade();
    login_row.connect_active_notify(move |row| {
        let enabled = row.is_active();
        let weak = weak.clone();
        let failed = move |err: String| {
            glib::g_warning!(LOG_DOMAIN, "Cannot change start at login: {err}");
            if let Some(dialog) = weak.upgrade() {
                dialog.add_toast(adw::Toast::new(&gettext("Cannot change start at login")));
            }
        };
        if !crate::portal::in_flatpak() {
            if let Err(err) = settings::set_autostart(enabled) {
                failed(err);
            }
            return;
        }
        // In the sandbox the desktop handles it, through the Background portal.
        glib::spawn_future_local(async move {
            match crate::portal::request_autostart(enabled).await {
                Ok(()) => {
                    update(None, |s| s.start_at_login = enabled);
                }
                Err(err) => failed(err.to_string()),
            }
        });
    });

    let weak = dialog.downgrade();
    style_row.connect_selected_notify(move |row| {
        let style = Style::ALL
            .get(row.selected() as usize)
            .copied()
            .unwrap_or_default();
        adw::StyleManager::default().set_color_scheme(style.color_scheme());
        update(weak.upgrade().as_ref(), |s| s.style = style);
    });

    let weak = dialog.downgrade();
    let app_weak = app.downgrade();
    language_row.connect_selected_notify(move |row| {
        // Index 0 is "System Language", the others follow `LANGUAGES`.
        let code = match row.selected() {
            0 => String::new(),
            i => LANGUAGES
                .get(i as usize - 1)
                .map(|(code, _)| (*code).to_owned())
                .unwrap_or_default(),
        };
        let dialog = weak.upgrade();
        if !update(dialog.as_ref(), |s| s.language = code.clone())
            || !settings::language_needs_restart(&code)
        {
            return;
        }
        let Some(dialog) = dialog else { return };
        let toast = adw::Toast::builder()
            .title(gettext("Restart Tunnelbird to apply the new language"))
            .button_label(gettext("_Restart"))
            .timeout(0)
            .build();
        let app_weak = app_weak.clone();
        toast.connect_button_clicked(move |_| {
            if let Some(app) = app_weak.upgrade() {
                crate::request_restart(&app);
            }
        });
        dialog.add_toast(toast);
    });

    page
}

fn connection_page(dialog: &adw::PreferencesDialog, context: Context) -> adw::PreferencesPage {
    let current = Settings::load().trust;
    let page = adw::PreferencesPage::builder()
        .title(gettext("Automatic Connection"))
        .name("connection")
        .icon_name("network-wireless-symbolic")
        .build();

    let rules = adw::PreferencesGroup::builder()
        .description(gettext(
            "Tunnelbird connects a tunnel by itself when you join a network you do not trust, such as café or hotel Wi-Fi. It never disconnects anything on its own.",
        ))
        .build();
    let enabled_row = adw::SwitchRow::builder()
        .title(gettext("Connect on Untrusted Networks"))
        .active(current.enabled)
        .build();
    clickable(&enabled_row);
    rules.add(&enabled_row);

    let names: Vec<&str> = context
        .tunnels
        .iter()
        .map(|(_, name)| name.as_str())
        .collect();
    let tunnel_row = adw::ComboRow::builder()
        .title(gettext("Tunnel to Connect"))
        .model(&gtk::StringList::new(&names))
        .sensitive(!names.is_empty())
        .build();
    if names.is_empty() {
        tunnel_row.set_subtitle(&gettext("Import a tunnel first"));
    }
    let index = context
        .tunnels
        .iter()
        .position(|(uuid, _)| *uuid == current.tunnel)
        .unwrap_or(0);
    tunnel_row.set_selected(index as u32);
    clickable(&tunnel_row);
    rules.add(&tunnel_row);

    let wired_row = adw::SwitchRow::builder()
        .title(gettext("Trust Wired Networks"))
        .subtitle(gettext("A cable usually means home or office"))
        .active(current.trust_wired)
        .build();
    clickable(&wired_row);
    rules.add(&wired_row);
    let strict_row = adw::SwitchRow::builder()
        .title(gettext("Strict Verification"))
        .subtitle(gettext(
            "Also check the hardware of the Wi-Fi router, so a hotspot with the same name is not trusted. Best for networks without repeaters.",
        ))
        .active(current.strict)
        .build();
    clickable(&strict_row);
    rules.add(&strict_row);
    page.add(&rules);

    let trusted = adw::PreferencesGroup::builder()
        .title(gettext("Trusted Wi-Fi Networks"))
        .build();
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    trusted.add(&list);
    page.add(&trusted);

    let tunnels = Rc::new(context.tunnels);
    let wifi = context.network.filter(|n| n.kind == NetworkKind::Wifi);
    let list_state = Rc::new(TrustedList {
        list,
        dialog: dialog.downgrade(),
        current_ssid: wifi.as_ref().and_then(|n| n.ssid.clone()),
        current_bssid: wifi.and_then(|n| n.bssid),
        saved_wifi: context.saved_wifi,
    });
    list_state.fill();

    let (weak, list) = (dialog.downgrade(), list_state.clone());
    strict_row.connect_active_notify(move |row| {
        update(weak.upgrade().as_ref(), |s| {
            s.trust.strict = row.is_active()
        });
        list.fill();
    });

    // The tunnel choice defaults to the first one: save it as soon as the
    // feature is turned on, so "enabled" never points to no tunnel.
    let weak = dialog.downgrade();
    let (tunnels_c, tunnel_row_c) = (tunnels.clone(), tunnel_row.clone());
    enabled_row.connect_active_notify(move |row| {
        let tunnel = selected_uuid(&tunnels_c, &tunnel_row_c);
        update(weak.upgrade().as_ref(), |s| {
            s.trust.enabled = row.is_active();
            if s.trust.tunnel.is_empty() {
                s.trust.tunnel = tunnel.clone();
            }
        });
    });
    let weak = dialog.downgrade();
    tunnel_row.connect_selected_notify(move |row| {
        let tunnel = selected_uuid(&tunnels, row);
        update(weak.upgrade().as_ref(), |s| s.trust.tunnel = tunnel.clone());
    });
    let weak = dialog.downgrade();
    wired_row.connect_active_notify(move |row| {
        update(weak.upgrade().as_ref(), |s| {
            s.trust.trust_wired = row.is_active()
        });
    });

    page
}

fn selected_uuid(tunnels: &[(String, String)], row: &adw::ComboRow) -> String {
    tunnels
        .get(row.selected() as usize)
        .map(|(uuid, _)| uuid.clone())
        .unwrap_or_default()
}

/// The trusted Wi-Fi list, rebuilt after each change.
struct TrustedList {
    list: gtk::ListBox,
    dialog: glib::WeakRef<adw::PreferencesDialog>,
    current_ssid: Option<String>,
    current_bssid: Option<String>,
    saved_wifi: Vec<String>,
}

impl TrustedList {
    fn fill(self: &Rc<Self>) {
        self.list.remove_all();
        let policy = Settings::load().trust;
        let trusted = policy.trusted_ssids.clone();

        if let Some(ssid) = self.current_ssid.clone() {
            // The access point is pinned too, so the strict check works right away.
            let pin = self.current_bssid.clone().map(|bssid| Pin {
                bssid: normalize_bssid(&bssid),
                ssid: ssid.clone(),
            });
            let unknown_access_point = policy.strict
                && trusted.contains(&ssid)
                && pin
                    .as_ref()
                    .is_some_and(|pin| !policy.is_pinned(&ssid, &pin.bssid));
            let label = if !trusted.contains(&ssid) {
                Some((
                    gettext("Trust “{name}”").replace("{name}", &ssid),
                    gettext("The Wi-Fi network you are on now"),
                ))
            } else if unknown_access_point {
                Some((
                    gettext("Trust This Access Point of “{name}”").replace("{name}", &ssid),
                    gettext("New router or repeater: trust it only if it is yours"),
                ))
            } else {
                None
            };
            if let Some((title, subtitle)) = label {
                let row = adw::ActionRow::builder()
                    .title(title)
                    .subtitle(subtitle)
                    .use_markup(false)
                    .activatable(true)
                    .build();
                row.add_prefix(&gtk::Image::from_icon_name("list-add-symbolic"));
                clickable(&row);
                let this = self.clone();
                row.connect_activated(move |_| {
                    this.change(|policy| {
                        if !policy.trusted_ssids.contains(&ssid) {
                            policy.trusted_ssids.push(ssid.clone());
                        }
                        if let Some(pin) = pin.clone().filter(|p| !policy.pins.contains(p)) {
                            policy.pins.push(pin);
                        }
                    })
                });
                self.list.append(&row);
            }
        }

        for ssid in &trusted {
            let row = adw::ActionRow::builder()
                .title(ssid.as_str())
                .use_markup(false)
                .build();
            if policy.strict {
                let pins = policy.pins_for(ssid);
                row.set_subtitle(&if pins == 0 {
                    gettext("Its access point will be remembered the next time you connect")
                } else {
                    gettext("Checked against {count} access point(s)")
                        .replace("{count}", &pins.to_string())
                });
            }
            row.add_prefix(&gtk::Image::from_icon_name("network-wireless-symbolic"));
            let remove = gtk::Button::builder()
                .icon_name("user-trash-symbolic")
                .tooltip_text(gettext("Stop Trusting"))
                .valign(gtk::Align::Center)
                .css_classes(["flat"])
                .build();
            clickable(&remove);
            let (this, ssid) = (self.clone(), ssid.clone());
            remove.connect_clicked(move |_| {
                this.change(|policy| {
                    policy.trusted_ssids.retain(|s| *s != ssid);
                    policy.pins.retain(|pin| pin.ssid != ssid);
                })
            });
            row.add_suffix(&remove);
            self.list.append(&row);
        }

        // The other saved networks, to pick from (the current one has its own row above).
        let candidates: Vec<&String> = self
            .saved_wifi
            .iter()
            .filter(|ssid| !trusted.contains(ssid) && self.current_ssid.as_ref() != Some(ssid))
            .collect();
        if candidates.is_empty() {
            return;
        }
        let saved = adw::ExpanderRow::builder()
            .title(gettext("Add a Saved Wi-Fi Network"))
            .subtitle(gettext("Networks this computer has already joined"))
            .build();
        saved.add_prefix(&gtk::Image::from_icon_name("list-add-symbolic"));
        clickable(&saved);
        for ssid in candidates {
            let row = adw::ActionRow::builder()
                .title(ssid.as_str())
                .use_markup(false)
                .activatable(true)
                .build();
            row.add_prefix(&gtk::Image::from_icon_name("network-wireless-symbolic"));
            row.add_suffix(&gtk::Image::from_icon_name("list-add-symbolic"));
            clickable(&row);
            let (this, ssid) = (self.clone(), ssid.clone());
            row.connect_activated(move |_| {
                this.change(|policy| {
                    if !policy.trusted_ssids.contains(&ssid) {
                        policy.trusted_ssids.push(ssid.clone());
                    }
                });
            });
            saved.add_row(&row);
        }
        self.list.append(&saved);
    }

    fn change(self: &Rc<Self>, edit: impl FnOnce(&mut Policy)) {
        let dialog = self.dialog.upgrade();
        let edit = RefCell::new(Some(edit));
        update(dialog.as_ref(), |s| {
            if let Some(edit) = edit.borrow_mut().take() {
                edit(&mut s.trust);
            }
        });
        // Rebuilding removes the row that triggered this; do it once GTK is done with it.
        let this = self.clone();
        glib::idle_add_local_once(move || this.fill());
    }
}

/// Loads the settings, applies `change` and saves; a failure is logged and
/// shown in the dialog. Returns whether the save worked.
fn update(dialog: Option<&adw::PreferencesDialog>, change: impl FnOnce(&mut Settings)) -> bool {
    let mut settings = Settings::load();
    change(&mut settings);
    match settings.save() {
        Ok(()) => true,
        Err(err) => {
            glib::g_warning!(LOG_DOMAIN, "Cannot save settings: {err}");
            if let Some(dialog) = dialog {
                dialog.add_toast(
                    adw::Toast::builder()
                        .title(gettext("Cannot save preferences"))
                        .use_markup(false)
                        .build(),
                );
            }
            false
        }
    }
}
