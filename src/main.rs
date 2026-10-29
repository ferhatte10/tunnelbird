// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

//! Tunnelbird: a lightweight GTK4/libadwaita VPN client,
//! built on top of NetworkManager.
//!
//! Module map (see `docs/ARCHITECTURE.md`):
//! - [`demo`]    made-up data for screenshots (`TUNNELBIRD_DEMO`);
//! - [`config`]  parses `wg-quick` `.conf` files, [`editor`] shows and edits them;
//! - [`geo`]     looks up the public IP and its location;
//! - [`health`]  tells a working tunnel from one whose server stays silent;
//! - [`nm`]      talks to NetworkManager over D-Bus;
//! - [`provider`] recognises well-known VPN providers;
//! - [`settings`] stores user preferences, [`preferences`] edits them;
//! - [`share`]   exports a tunnel as a file or a QR code;
//! - [`traffic`] reads interface counters and draws the live graph;
//! - [`trust`]   decides when to connect automatically (trusted networks);
//! - [`update`]  tells when a new version is out and how it arrives;
//! - [`window`]  builds the user interface.

mod config;
mod demo;
mod editor;
mod geo;
mod health;
mod nm;
mod portal;
pub mod preferences;
mod provider;
mod settings;
mod share;
mod traffic;
mod tray;
mod trust;
mod update;
mod window;

use std::os::unix::process::CommandExt;
use std::sync::atomic::{AtomicBool, Ordering};

use adw::prelude::*;
use gettextrs::{LocaleCategory, gettext};
use gtk::{gio, glib};

pub const APP_ID: &str = "fr.ferhatsn.Tunnelbird";
/// Log domain used with GLib logging. Show debug messages with
/// `G_MESSAGES_DEBUG=tunnelbird`.
pub const LOG_DOMAIN: &str = "tunnelbird";
const GETTEXT_PACKAGE: &str = "tunnelbird";
const AUTHOR: &str = "Ferhat Saidoun";
const AUTHOR_WEBSITE: &str = "https://ferhatsn.fr";
const AUTHOR_GITHUB: &str = "https://github.com/ferhatte10";
/// Translations directory, chosen at build time by `build.rs`.
const LOCALEDIR: &str = env!("TUNNELBIRD_LOCALEDIR");

/// Set by [`request_restart`]; `main` relaunches the app once it has fully quit.
static RESTART: AtomicBool = AtomicBool::new(false);
/// `--background` (used at login): the first activation leaves the window hidden.
static START_HIDDEN: AtomicBool = AtomicBool::new(false);

fn main() -> glib::ExitCode {
    install_panic_logger();
    if demo::enabled() {
        demo::prepare();
    }
    let settings = settings::Settings::load();
    settings::apply_language(&settings.language);
    init_i18n();

    let app = adw::Application::builder()
        .application_id(APP_ID)
        .flags(if demo::enabled() {
            // Runs next to a real instance, never instead of it.
            gio::ApplicationFlags::HANDLES_OPEN | gio::ApplicationFlags::NON_UNIQUE
        } else {
            gio::ApplicationFlags::HANDLES_OPEN
        })
        .build();
    app.add_main_option(
        "background",
        glib::Char::from(b'b'),
        glib::OptionFlags::NONE,
        glib::OptionArg::None,
        &gettext("Start hidden in the top bar"),
        None,
    );
    app.connect_handle_local_options(|_, options| {
        if options.contains("background") {
            START_HIDDEN.store(true, Ordering::SeqCst);
        }
        // Carry on with the normal startup.
        std::ops::ControlFlow::Continue(())
    });

    app.connect_startup(move |app| {
        adw::StyleManager::default().set_color_scheme(settings.style.color_scheme());
        use_dev_icons();
        load_css();
        gtk::Window::set_default_icon_name(APP_ID);
        setup_app_actions(app);
        let ui = window::Ui::new(app);
        if let Some(dir) = std::env::var_os("TUNNELBIRD_SCREENSHOTS").filter(|_| demo::enabled()) {
            glib::spawn_future_local(ui.clone().take_screenshots(dir.into()));
        }

        let on_activate = ui.clone();
        app.connect_activate(move |_| {
            if !START_HIDDEN.swap(false, Ordering::SeqCst) {
                on_activate.window.present();
            }
        });
        // `tunnelbird file.conf` or "Open With" from a file manager imports the files.
        app.connect_open(move |_, files, _| {
            ui.window.present();
            ui.import_files(files.to_vec());
        });
    });

    let status = app.run();
    if demo::enabled() {
        demo::cleanup();
    }
    if RESTART.load(Ordering::SeqCst) {
        relaunch();
    }
    status
}

/// Quits the app and starts it again, e.g. to apply a new language.
pub fn request_restart(app: &adw::Application) {
    RESTART.store(true, Ordering::SeqCst);
    app.quit();
}

/// Replaces this process with a fresh one. The old instance has released its
/// D-Bus name by now, so the new one starts as the primary instance.
fn relaunch() {
    if portal::in_flatpak() {
        let spawned = glib::MainContext::default().block_on(portal::spawn_latest_version());
        if let Err(err) = spawned {
            glib::g_critical!(LOG_DOMAIN, "Cannot restart Tunnelbird: {err}");
        }
        return;
    }
    match std::env::current_exe() {
        Ok(exe) => {
            // After an update the running binary is “deleted”: start the new
            // one installed at the same path.
            let path = exe.to_string_lossy();
            let exe = path.strip_suffix(" (deleted)").unwrap_or(&path);
            let err = std::process::Command::new(exe).exec();
            glib::g_critical!(LOG_DOMAIN, "Cannot restart Tunnelbird: {err}");
        }
        Err(err) => glib::g_critical!(LOG_DOMAIN, "Cannot find the Tunnelbird binary: {err}"),
    }
}

fn setup_app_actions(app: &adw::Application) {
    let about = gio::SimpleAction::new("about", None);
    let weak = app.downgrade();
    about.connect_activate(move |_, _| {
        let Some(app) = weak.upgrade() else { return };
        let dialog = adw::AboutDialog::builder()
            .application_name("Tunnelbird")
            .application_icon(APP_ID)
            .developer_name(AUTHOR)
            .developers([format!("{AUTHOR} {AUTHOR_WEBSITE}").as_str()])
            .website(AUTHOR_WEBSITE)
            .copyright(format!("© 2026 {AUTHOR}"))
            .version(env!("CARGO_PKG_VERSION"))
            .comments(gettext("Connect to your VPN in one click."))
            // Translators: replace with your names, one per line.
            .artists([AUTHOR])
            .license_type(gtk::License::Gpl30)
            .translator_credits(gettext("translator-credits"))
            .build();
        dialog.add_link(&gettext("Author on GitHub"), AUTHOR_GITHUB);
        if let Some((version, notes)) = update::release_notes() {
            dialog.set_release_notes_version(&version);
            dialog.set_release_notes(&notes);
        }
        dialog.present(app.active_window().as_ref());
    });
    app.add_action(&about);

    let quit = gio::SimpleAction::new("quit", None);
    let weak = app.downgrade();
    quit.connect_activate(move |_, _| {
        if let Some(app) = weak.upgrade() {
            app.quit();
        }
    });
    app.add_action(&quit);

    app.set_accels_for_action("app.quit", &["<Control>q"]);
    app.set_accels_for_action("app.preferences", &["<Control>comma"]);
    app.set_accels_for_action("win.import", &["<Control>o"]);
    app.set_accels_for_action("window.close", &["<Control>w"]);
}

/// App-specific styles from `data/style.css`, compiled into the binary.
fn load_css() {
    let Some(display) = gtk::gdk::Display::default() else {
        glib::g_warning!(LOG_DOMAIN, "No display: custom styles not loaded");
        return;
    };
    let provider = gtk::CssProvider::new();
    provider.load_from_string(include_str!("../data/style.css"));
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

/// With `cargo run`, the app icon is not installed yet: load it from `data/icons`.
/// Installed builds use the system icon theme (`make install`, Flatpak).
fn use_dev_icons() {
    #[cfg(debug_assertions)]
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::IconTheme::for_display(&display)
            .add_search_path(concat!(env!("CARGO_MANIFEST_DIR"), "/data/icons"));
    }
}

/// Sets up gettext. English is the source language; translations follow the
/// language chosen in Preferences, else the system locale (`LANGUAGE`, `LC_ALL`, `LANG`). A failure only means the UI
/// stays in English, so it is logged and never fatal.
fn init_i18n() {
    // SAFETY: called first thing in `main`, before GTK or zbus start any thread.
    if unsafe { gettextrs::setlocale(LocaleCategory::LcAll, "") }.is_none() {
        glib::g_warning!(LOG_DOMAIN, "Unsupported locale, falling back to English");
    }
    let dir = std::env::var("TUNNELBIRD_LOCALEDIR").unwrap_or_else(|_| LOCALEDIR.to_owned());
    if let Err(err) = gettextrs::bindtextdomain(GETTEXT_PACKAGE, dir) {
        glib::g_warning!(LOG_DOMAIN, "Cannot bind translations: {err}");
    }
    if let Err(err) = gettextrs::bind_textdomain_codeset(GETTEXT_PACKAGE, "UTF-8") {
        glib::g_warning!(LOG_DOMAIN, "Cannot set translation encoding: {err}");
    }
    if let Err(err) = gettextrs::textdomain(GETTEXT_PACKAGE) {
        glib::g_warning!(LOG_DOMAIN, "Cannot select translation domain: {err}");
    }
}

/// A panic is a bug: make sure it reaches the system journal before the default report.
fn install_panic_logger() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        glib::g_critical!(LOG_DOMAIN, "Tunnelbird crashed: {info}");
        default_hook(info);
    }));
}
