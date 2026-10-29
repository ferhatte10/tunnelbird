// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

//! User preferences (style and language), stored in
//! `$XDG_CONFIG_HOME/tunnelbird/settings.ini` (usually `~/.config`).
//!
//! A plain key file needs no GSettings schema to install and works the same
//! from `cargo run`, a system install or a Flatpak sandbox.

use std::path::PathBuf;
use std::sync::OnceLock;

use gtk::glib;

use crate::LOG_DOMAIN;
use crate::trust::{Pin, Policy};

const GROUP: &str = "preferences";
const KEY_STYLE: &str = "style";
const KEY_LANGUAGE: &str = "language";
const KEY_BACKGROUND: &str = "run-in-background";
const KEY_AUTO_CONNECT: &str = "auto-connect";
const KEY_AUTO_TUNNEL: &str = "auto-connect-tunnel";
const KEY_TRUSTED: &str = "trusted-wifi";
const KEY_TRUST_WIRED: &str = "trust-wired";
const KEY_START_AT_LOGIN: &str = "start-at-login";
const KEY_STRICT: &str = "strict-wifi-check";
/// Pinned access points, one `BSSID SSID` entry each (the BSSID has a fixed width).
const KEY_PINS: &str = "trusted-access-points";

/// Interface languages offered in Preferences, as (code, endonym).
/// Add a line here when a new `po/<code>.po` translation lands (see docs/TRANSLATING.md).
pub const LANGUAGES: &[(&str, &str)] = &[("en", "English"), ("fr", "Français")];

/// Language applied when the app started, to know whether a restart is needed.
static STARTUP_LANGUAGE: OnceLock<String> = OnceLock::new();
/// Interface language the system asks for, among [`LANGUAGES`] (English when
/// the system language is not translated). Read before `LANGUAGE` is changed.
static SYSTEM_LANGUAGE: OnceLock<&'static str> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Style {
    #[default]
    System,
    Light,
    Dark,
}

impl Style {
    pub const ALL: [Style; 3] = [Style::System, Style::Light, Style::Dark];

    fn key(self) -> &'static str {
        match self {
            Style::System => "system",
            Style::Light => "light",
            Style::Dark => "dark",
        }
    }

    fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|style| style.key() == key)
    }

    pub fn color_scheme(self) -> adw::ColorScheme {
        match self {
            Style::System => adw::ColorScheme::Default,
            Style::Light => adw::ColorScheme::ForceLight,
            Style::Dark => adw::ColorScheme::ForceDark,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Settings {
    pub style: Style,
    /// Language code from [`LANGUAGES`], or empty to follow the system.
    pub language: String,
    /// Keep running in the top bar when the window is closed.
    pub background: bool,
    /// Automatic connection on untrusted networks (see `trust.rs`).
    pub trust: Policy,
    /// Remembered only in Flatpak, where the host autostart folder is not visible.
    pub start_at_login: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            style: Style::default(),
            language: String::new(),
            background: true,
            start_at_login: false,
            trust: Policy {
                trust_wired: true,
                ..Policy::default()
            },
        }
    }
}

impl Settings {
    /// Reads the settings file. A missing or damaged file gives the defaults
    /// (damage is logged): preferences must never prevent the app from starting.
    pub fn load() -> Self {
        let file = glib::KeyFile::new();
        if let Err(err) = file.load_from_file(path(), glib::KeyFileFlags::NONE) {
            if !err.matches(glib::FileError::Noent) {
                glib::g_warning!(LOG_DOMAIN, "Cannot read settings, using defaults: {err}");
            }
            return Self::default();
        }
        let style = file
            .string(GROUP, KEY_STYLE)
            .ok()
            .and_then(|key| Style::from_key(&key))
            .unwrap_or_default();
        let language = file
            .string(GROUP, KEY_LANGUAGE)
            .map(|code| code.to_string())
            .ok()
            .filter(|code| LANGUAGES.iter().any(|(known, _)| known == code))
            .unwrap_or_default();
        let background = file.boolean(GROUP, KEY_BACKGROUND).unwrap_or(true);
        let trust = Policy {
            enabled: file.boolean(GROUP, KEY_AUTO_CONNECT).unwrap_or(false),
            tunnel: file
                .string(GROUP, KEY_AUTO_TUNNEL)
                .map(|s| s.to_string())
                .unwrap_or_default(),
            trusted_ssids: file
                .string_list(GROUP, KEY_TRUSTED)
                .map(|list| list.iter().map(|s| s.to_string()).collect())
                .unwrap_or_default(),
            trust_wired: file.boolean(GROUP, KEY_TRUST_WIRED).unwrap_or(true),
            strict: file.boolean(GROUP, KEY_STRICT).unwrap_or(false),
            pins: file
                .string_list(GROUP, KEY_PINS)
                .map(|list| list.iter().filter_map(|entry| parse_pin(entry)).collect())
                .unwrap_or_default(),
        };
        Self {
            style,
            language,
            background,
            trust,
            start_at_login: file.boolean(GROUP, KEY_START_AT_LOGIN).unwrap_or(false),
        }
    }

    pub fn save(&self) -> Result<(), String> {
        let path = path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|err| format!("{}: {err}", dir.display()))?;
        }
        let file = glib::KeyFile::new();
        file.set_string(GROUP, KEY_STYLE, self.style.key());
        file.set_string(GROUP, KEY_LANGUAGE, &self.language);
        file.set_boolean(GROUP, KEY_BACKGROUND, self.background);
        file.set_boolean(GROUP, KEY_AUTO_CONNECT, self.trust.enabled);
        file.set_string(GROUP, KEY_AUTO_TUNNEL, &self.trust.tunnel);
        file.set_value(
            GROUP,
            KEY_TRUSTED,
            &string_list_value(&self.trust.trusted_ssids),
        );
        file.set_boolean(GROUP, KEY_TRUST_WIRED, self.trust.trust_wired);
        file.set_boolean(GROUP, KEY_START_AT_LOGIN, self.start_at_login);
        file.set_boolean(GROUP, KEY_STRICT, self.trust.strict);
        let pins: Vec<String> = self
            .trust
            .pins
            .iter()
            .map(|pin| format!("{} {}", pin.bssid, pin.ssid))
            .collect();
        file.set_value(GROUP, KEY_PINS, &string_list_value(&pins));
        file.save_to_file(&path)
            .map_err(|err| format!("{}: {err}", path.display()))
    }
}

/// Sets the interface language before gettext starts. Must run first in `main`.
///
/// `code` is the user's choice, empty for "System Language". The system
/// language is used when Tunnelbird is translated in it; otherwise the whole
/// interface switches to English, GNOME's own labels included, rather than
/// mixing English texts with "Cancel" buttons in another language.
pub fn apply_language(code: &str) {
    let _ = STARTUP_LANGUAGE.set(code.to_owned());
    let system = *SYSTEM_LANGUAGE.get_or_init(detect_system_language);
    let effective = if code.is_empty() { system } else { code };
    // SAFETY: called at the very start of `main`, before any other thread exists.
    unsafe { std::env::set_var("LANGUAGE", effective) };
}

/// First translated language in the system preferences (`LANGUAGE`,
/// `LC_ALL`, `LC_MESSAGES`, `LANG`), or English.
fn detect_system_language() -> &'static str {
    let names = glib::language_names();
    pick_language(names.iter().map(|name| name.as_str()))
}

/// First of `names` (locale names such as `fr_FR.UTF-8`) that Tunnelbird is
/// translated in, else English.
fn pick_language<'a>(names: impl IntoIterator<Item = &'a str>) -> &'static str {
    names
        .into_iter()
        .filter_map(|name| name.split(['_', '.', '@']).next())
        .find_map(|base| {
            LANGUAGES
                .iter()
                .find(|(code, _)| *code == base)
                .map(|(code, _)| *code)
        })
        .unwrap_or("en")
}

/// Name of the language "System Language" resolves to, e.g. “English”.
pub fn system_language_name() -> &'static str {
    let code = SYSTEM_LANGUAGE.get_or_init(detect_system_language);
    LANGUAGES
        .iter()
        .find(|(known, _)| known == code)
        .map_or("English", |(_, name)| *name)
}

/// True when the chosen language differs from the one the app started with.
pub fn language_needs_restart(code: &str) -> bool {
    STARTUP_LANGUAGE
        .get()
        .is_some_and(|started| started != code)
}

/// A string list in the key file syntax, escaped like GLib does, so that
/// `KeyFile::string_list` reads back Wi-Fi names containing `;` or a backslash.
/// (glib-rs has no binding for `set_string_list`.)
fn string_list_value(items: &[String]) -> String {
    items
        .iter()
        .map(|item| {
            let mut escaped = String::new();
            for (index, c) in item.chars().enumerate() {
                match c {
                    '\\' => escaped.push_str("\\\\"),
                    ';' => escaped.push_str("\\;"),
                    '\n' => escaped.push_str("\\n"),
                    '\t' => escaped.push_str("\\t"),
                    '\r' => escaped.push_str("\\r"),
                    ' ' if index == 0 => escaped.push_str("\\s"),
                    c => escaped.push(c),
                }
            }
            escaped.push(';');
            escaped
        })
        .collect()
}

/// `AA:BB:CC:DD:EE:FF Home Wi-Fi` → pin. Damaged entries are skipped.
fn parse_pin(entry: &str) -> Option<Pin> {
    const BSSID_LEN: usize = 17;
    let bssid = entry.get(..BSSID_LEN)?;
    let ssid = entry.get(BSSID_LEN + 1..)?;
    (bssid.split(':').count() == 6 && !ssid.is_empty()).then(|| Pin {
        bssid: crate::trust::normalize_bssid(bssid),
        ssid: ssid.to_owned(),
    })
}

/// The login item: `$XDG_CONFIG_HOME/autostart/<app id>.desktop`, the
/// freedesktop way to start an app with the session.
fn autostart_path() -> PathBuf {
    glib::user_config_dir()
        .join("autostart")
        .join(format!("{}.desktop", crate::APP_ID))
}

pub fn autostart_enabled() -> bool {
    if crate::portal::in_flatpak() {
        Settings::load().start_at_login
    } else {
        autostart_path().exists()
    }
}

/// Starts Tunnelbird hidden in the top bar at login (`--background`), or stops doing so.
pub fn set_autostart(enabled: bool) -> Result<(), String> {
    let path = autostart_path();
    if !enabled {
        return match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(format!("{}: {err}", path.display())),
        };
    }
    // In a Snap, snapd starts the app at login from this file (the snap's
    // `autostart` key) and only reads the arguments: the command is its own.
    let command = if std::env::var_os("SNAP").is_some() {
        "tunnelbird".to_owned()
    } else {
        let exe = std::env::current_exe().map_err(|err| err.to_string())?;
        format!("\"{}\"", exe.display())
    };
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=Tunnelbird\nExec={command} --background\nIcon={}\nNoDisplay=true\nX-GNOME-Autostart-enabled=true\n",
        crate::APP_ID
    );
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|err| format!("{}: {err}", dir.display()))?;
    }
    std::fs::write(&path, entry).map_err(|err| format!("{}: {err}", path.display()))
}

fn path() -> PathBuf {
    glib::user_config_dir()
        .join("tunnelbird")
        .join("settings.ini")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_access_points_round_trip() {
        let pin = parse_pin("aa:bb:cc:00:11:22 Café; free Wi-Fi").expect("valid entry");
        assert_eq!(pin.bssid, "AA:BB:CC:00:11:22");
        assert_eq!(pin.ssid, "Café; free Wi-Fi");
        assert!(parse_pin("not-a-bssid Home").is_none());
        assert!(parse_pin("AA:BB:CC:00:11:22").is_none());
    }

    #[test]
    fn system_language_falls_back_to_english() {
        assert_eq!(pick_language(["fr_FR.UTF-8", "fr_FR", "fr", "C"]), "fr");
        assert_eq!(pick_language(["de_DE.UTF-8", "de", "C"]), "en");
        // A translated language later in the preference list wins over English.
        assert_eq!(pick_language(["de", "fr", "C"]), "fr");
        assert_eq!(pick_language(["C"]), "en");
    }

    #[test]
    fn wifi_names_with_special_characters_round_trip() {
        let names = vec![
            "Café; free".to_owned(),
            " leading space".to_owned(),
            "back\\slash".to_owned(),
            "Freebox-Home".to_owned(),
        ];
        let file = glib::KeyFile::new();
        file.set_value(GROUP, KEY_TRUSTED, &string_list_value(&names));
        let read: Vec<String> = file
            .string_list(GROUP, KEY_TRUSTED)
            .expect("list")
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(read, names);
    }
}
