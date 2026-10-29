// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

//! Main window: tunnel list, live traffic card, import and error reporting.
//!
//! Every failure goes through [`Ui::report_error`], which logs it and shows a
//! toast, so nothing fails silently and nothing panics.
//! [`Ui`] lives as long as the application, so closures simply hold an `Rc` to it.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;
use std::time::{Duration, Instant};

use adw::prelude::*;
use futures_channel::mpsc;
use futures_util::StreamExt;
use gettextrs::gettext;
use gtk::{gdk, gio, glib};

use crate::config::{self, WgConfig};
use crate::demo;
use crate::editor;
use crate::geo::{self, Location};
use crate::health::{Health, HealthWatch};
use crate::nm::{self, Nm, Tunnel, TunnelState};
use crate::settings::Settings;
use crate::share;
use crate::traffic::{self, Sample, TrafficGraph};
use crate::tray::{Tray, TrayAction, TrayModel, TrayTunnel};
use crate::trust::Decision;
use crate::update::{self, FlatpakEvent, FlatpakMonitor, Install, Manager};
use crate::{APP_ID, LOG_DOMAIN};

/// Groups bursts of NetworkManager signals into a single refresh.
const REFRESH_DEBOUNCE: Duration = Duration::from_millis(150);
/// Fallback refresh period when NetworkManager signals cannot be watched.
const POLL_FALLBACK: Duration = Duration::from_secs(3);
/// Routes and DNS settle just after NetworkManager reports "connected".
const LOOKUP_DELAY: Duration = Duration::from_secs(1);
/// Room reserved for "● Download 999,9 MB/s" so the legend does not shift.
const RATE_WIDTH_CHARS: i32 = 24;
/// Room reserved for "scale 999 MB/s".
const SCALE_WIDTH_CHARS: i32 = 16;
/// Samples shown in the top bar sparkline.
const TRAY_SPARK_POINTS: usize = 12;
/// Wait before the single retry of a failed public IP lookup.
const LOOKUP_RETRY: Duration = Duration::from_secs(3);
/// First update check after startup, once the network is up.
const FIRST_UPDATE_CHECK: Duration = Duration::from_secs(30);
/// How often to see whether the daily update check is due.
const UPDATE_TICK: Duration = Duration::from_secs(60 * 60);
/// How often to see whether a new version was installed meanwhile.
const RESTART_POLL: Duration = Duration::from_secs(60);
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// What the update banner says (see `update.rs`).
#[derive(Debug, Clone, PartialEq, Eq)]
enum UpdateNotice {
    None,
    /// A newer version exists; its number when known (Flathub does not say).
    Available(Option<String>),
    /// The Flatpak portal is installing it, in percent.
    Installing(u32),
    /// Installed: a restart runs it.
    Restart,
}

/// Public IP lookup of the connected tunnel, tied to that tunnel's UUID.
enum GeoState {
    Idle,
    Loading(String),
    Ready(String, Location),
    Failed(String),
}

impl GeoState {
    fn is_for(&self, uuid: &str) -> bool {
        match self {
            GeoState::Idle => false,
            GeoState::Loading(id) | GeoState::Ready(id, _) | GeoState::Failed(id) => id == uuid,
        }
    }
}

pub struct Ui {
    pub window: adw::ApplicationWindow,
    toasts: adw::ToastOverlay,
    stack: gtk::Stack,
    unavailable: adw::StatusPage,
    list: gtk::ListBox,
    live: LiveCard,
    nm: RefCell<Option<Nm>>,
    tunnels: RefCell<Vec<Tunnel>>,
    /// UUIDs of tunnels with a connect/disconnect request in flight.
    busy: RefCell<HashSet<String>>,
    /// UUIDs the user asked to connect, to detect activations that fail later.
    expecting: RefCell<HashSet<String>>,
    refresh_queued: Cell<bool>,
    http: soup::Session,
    geo: RefCell<GeoState>,
    app: adw::Application,
    /// Top bar icon, when a tray host (AppIndicator extension, KDE…) is running.
    tray: RefCell<Option<Rc<Tray>>>,
    /// Keeps the app alive while only the top bar icon is visible.
    hold: RefCell<Option<gio::ApplicationHoldGuard>>,
    /// The "still running in the top bar" notice is shown once per session.
    told_background: Cell<bool>,
    /// Watches whether the connected tunnel's server answers (see `health.rs`).
    health: RefCell<HealthWatch>,
    health_state: Cell<Health>,
    /// Tunnels this app asked to stop: their disconnection is expected, not a drop.
    user_stopped: RefCell<HashSet<String>>,
    /// Profile UUID of the last network seen, to act only when joining a new one.
    last_network: RefCell<Option<String>>,
    update_banner: adw::Banner,
    update_notice: RefCell<UpdateNotice>,
    /// The Flatpak portal's update watcher, in a Flatpak sandbox.
    flatpak: RefCell<Option<Rc<FlatpakMonitor>>>,
}

/// The "Active Connection" card: addresses, location and live graph.
struct LiveCard {
    group: adw::PreferencesGroup,
    name: gtk::Label,
    endpoint: gtk::Label,
    vpn_address: gtk::Label,
    location: gtk::Box,
    flag: gtk::Label,
    place: gtk::Label,
    public_ip: gtk::Label,
    locating: gtk::Spinner,
    relocate: gtk::Button,
    partial: gtk::Label,
    graph: TrafficGraph,
    rx_rate: gtk::Label,
    tx_rate: gtk::Label,
    scale: gtk::Label,
    totals: gtk::Label,
    /// Interface, counters and time of the previous sample, to compute rates.
    last: RefCell<Option<(String, u64, u64, Instant)>>,
    status: gtk::Label,
}

impl Ui {
    pub fn new(app: &adw::Application) -> Rc<Self> {
        let header = adw::HeaderBar::new();
        let import_button = gtk::Button::builder()
            .icon_name("list-add-symbolic")
            .tooltip_text(gettext("Import Configuration File"))
            .action_name("win.import")
            .build();
        clickable(&import_button);
        header.pack_start(&import_button);
        let menu = gio::Menu::new();
        menu.append(Some(&gettext("_Preferences")), Some("app.preferences"));
        menu.append(Some(&gettext("_About Tunnelbird")), Some("app.about"));
        let menu_button = gtk::MenuButton::builder()
            .icon_name("open-menu-symbolic")
            .tooltip_text(gettext("Main Menu"))
            .menu_model(&menu)
            .primary(true)
            .build();
        clickable(&menu_button);
        header.pack_end(&menu_button);

        let loading = gtk::Spinner::builder()
            .spinning(true)
            .width_request(32)
            .height_request(32)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .build();

        let empty = adw::StatusPage::builder()
            .icon_name(format!("{APP_ID}-symbolic"))
            .title(gettext("No Tunnels"))
            .description(gettext(
                "Import the .conf file provided by your VPN server, or drop it here.",
            ))
            .child(&pill_button(&gettext("_Import File…"), "win.import"))
            .build();

        let unavailable = adw::StatusPage::builder()
            .icon_name("network-error-symbolic")
            .title(gettext("NetworkManager Unavailable"))
            .child(&pill_button(&gettext("_Retry"), "win.retry"))
            .build();

        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["boxed-list", "tunnel-list"])
            .build();
        let tunnels_group = adw::PreferencesGroup::builder()
            .title(gettext("Tunnels"))
            .build();
        tunnels_group.add(&list);

        let live = LiveCard::new();
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(24)
            .margin_top(24)
            .margin_bottom(24)
            .margin_start(12)
            .margin_end(12)
            .build();
        content.append(&live.group);
        content.append(&tunnels_group);
        let scrolled = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(
                &adw::Clamp::builder()
                    .maximum_size(720)
                    .child(&content)
                    .build(),
            )
            .build();

        let stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .build();
        stack.add_named(&loading, Some("loading"));
        stack.add_named(&empty, Some("empty"));
        stack.add_named(&scrolled, Some("list"));
        stack.add_named(&unavailable, Some("unavailable"));

        let update_banner = adw::Banner::new("");
        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&header);
        toolbar.add_top_bar(&update_banner);
        toolbar.set_content(Some(&stack));
        let toasts = adw::ToastOverlay::new();
        toasts.set_child(Some(&toolbar));

        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title("Tunnelbird")
            .default_width(560)
            .default_height(700)
            .width_request(360)
            .height_request(400)
            .content(&toasts)
            .build();

        let ui = Rc::new(Self {
            window,
            toasts,
            stack,
            unavailable,
            list,
            live,
            nm: RefCell::new(None),
            tunnels: RefCell::new(Vec::new()),
            busy: RefCell::new(HashSet::new()),
            expecting: RefCell::new(HashSet::new()),
            refresh_queued: Cell::new(false),
            http: geo::new_session(),
            geo: RefCell::new(GeoState::Idle),
            app: app.clone(),
            tray: RefCell::new(None),
            hold: RefCell::new(None),
            told_background: Cell::new(false),
            health: RefCell::new(HealthWatch::default()),
            health_state: Cell::new(Health::Unknown),
            user_stopped: RefCell::new(HashSet::new()),
            last_network: RefCell::new(None),
            update_banner,
            update_notice: RefCell::new(UpdateNotice::None),
            flatpak: RefCell::new(None),
        });
        ui.setup_actions();
        ui.setup_drop_target();
        ui.setup_traffic_timer();
        ui.setup_close();
        if demo::enabled() {
            // Sized for store screenshots (Flathub: 1000×700 at most).
            ui.window.set_default_size(640, 700);
            ui.start_demo();
            // Update notices only when a test `latest.json` is given explicitly.
            if std::env::var_os("TUNNELBIRD_UPDATE_URL").is_some() {
                ui.setup_updates();
            }
            return ui;
        }
        ui.setup_updates();
        glib::spawn_future_local(ui.clone().start());
        glib::spawn_future_local(ui.clone().start_tray());
        ui
    }

    fn setup_actions(self: &Rc<Self>) {
        // Preferences live here: they need the tunnel list and the current network.
        let prefs = gio::SimpleAction::new("preferences", None);
        let ui = self.clone();
        prefs.connect_activate(move |_, _| {
            let ui = ui.clone();
            glib::spawn_future_local(async move {
                // Clone first: a RefCell borrow must not live across `.await`.
                let nm = ui.nm.borrow().clone();
                let (network, saved_wifi) = match nm {
                    None if demo::enabled() => (Some(demo::network()), demo::saved_wifi()),
                    Some(nm) => (
                        nm.current_network().await.unwrap_or_else(|err| {
                            glib::g_debug!(LOG_DOMAIN, "Cannot read the current network: {err}");
                            None
                        }),
                        nm.saved_wifi().await.unwrap_or_else(|err| {
                            glib::g_warning!(LOG_DOMAIN, "Cannot list saved Wi-Fi networks: {err}");
                            Vec::new()
                        }),
                    ),
                    None => (None, Vec::new()),
                };
                let tunnels = ui
                    .tunnels
                    .borrow()
                    .iter()
                    .map(|t| (t.uuid.clone(), t.name.clone()))
                    .collect();
                crate::preferences::present(
                    &ui.app,
                    crate::preferences::Context {
                        tunnels,
                        network,
                        saved_wifi,
                    },
                );
            });
        });
        self.app.add_action(&prefs);

        let import = gio::SimpleAction::new("import", None);
        let ui = self.clone();
        import.connect_activate(move |_, _| ui.open_import_dialog());
        self.window.add_action(&import);

        let retry = gio::SimpleAction::new("retry", None);
        let ui = self.clone();
        retry.connect_activate(move |_, _| {
            ui.stack.set_visible_child_name("loading");
            glib::spawn_future_local(ui.clone().start());
        });
        self.window.add_action(&retry);

        let relocate = gio::SimpleAction::new("relocate", None);
        let ui = self.clone();
        relocate.connect_activate(move |_, _| {
            let connected = ui.connected();
            if let Some(tunnel) = connected.filter(|t| t.full_tunnel) {
                ui.lookup_location(tunnel.uuid);
            }
        });
        self.window.add_action(&relocate);

        // Row "share" menu: the parameter is the tunnel UUID.
        for (name, qr) in [("export-tunnel", false), ("show-qr", true)] {
            let action = gio::SimpleAction::new(name, Some(glib::VariantTy::STRING));
            let ui = self.clone();
            action.connect_activate(move |_, uuid| {
                if let Some(uuid) = uuid.and_then(|v| v.get::<String>()) {
                    ui.share_tunnel(&uuid, qr);
                }
            });
            self.window.add_action(&action);
        }
    }

    /// Accepts `.conf` files dropped anywhere on the window.
    fn setup_drop_target(self: &Rc<Self>) {
        let target = gtk::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY);
        let ui = self.clone();
        target.connect_drop(move |_, value, _, _| match value.get::<gdk::FileList>() {
            Ok(files) => {
                ui.import_files(files.files());
                true
            }
            Err(err) => {
                glib::g_warning!(LOG_DOMAIN, "Unexpected drop content: {err}");
                false
            }
        });
        self.window.add_controller(target);
    }

    fn setup_traffic_timer(self: &Rc<Self>) {
        let ui = self.clone();
        glib::timeout_add_seconds_local(1, move || {
            ui.sample_traffic();
            glib::ControlFlow::Continue
        });
    }

    /// Connects to NetworkManager, starts watching its signals and loads the tunnels.
    async fn start(self: Rc<Self>) {
        let nm = match Nm::connect().await {
            Ok(nm) => nm,
            Err(err) => {
                glib::g_warning!(LOG_DOMAIN, "Cannot connect to the system bus: {err}");
                self.show_unavailable(&nm::error_message(&err));
                return;
            }
        };
        *self.nm.borrow_mut() = Some(nm.clone());

        match nm.changes().await {
            Ok(mut changes) => {
                let ui = self.clone();
                glib::spawn_future_local(async move {
                    while changes.next().await.is_some() {
                        ui.queue_refresh();
                    }
                    glib::g_warning!(LOG_DOMAIN, "NetworkManager signal stream ended");
                });
            }
            Err(err) => {
                glib::g_warning!(
                    LOG_DOMAIN,
                    "Cannot watch NetworkManager signals, polling instead: {err}"
                );
                let ui = self.clone();
                glib::timeout_add_local(POLL_FALLBACK, move || {
                    ui.queue_refresh();
                    glib::ControlFlow::Continue
                });
            }
        }
        self.refresh().await;
    }

    /// Demo mode: made-up tunnels and a full graph instead of NetworkManager.
    fn start_demo(self: &Rc<Self>) {
        let tunnels = demo::tunnels();
        if let Some(connected) = tunnels.first() {
            *self.geo.borrow_mut() = GeoState::Ready(connected.uuid.clone(), demo::location());
        }
        *self.tunnels.borrow_mut() = tunnels;
        for tick in 0..demo::HISTORY {
            self.live.graph.push(demo::sample(tick));
        }
        self.render();
    }

    /// Takes the store screenshots in `dir` (demo mode), then quits.
    pub async fn take_screenshots(self: Rc<Self>, dir: std::path::PathBuf) {
        let settle = |ms| glib::timeout_future(Duration::from_millis(ms));
        if let Err(err) = std::fs::create_dir_all(&dir) {
            glib::g_critical!(LOG_DOMAIN, "Cannot create {}: {err}", dir.display());
            return self.app.quit();
        }
        let shot = async |name: &str| {
            let path = dir.join(format!("{name}.png"));
            match demo::capture(&self.window, &path).await {
                Ok(()) => glib::g_message!(LOG_DOMAIN, "Saved {}", path.display()),
                Err(err) => glib::g_critical!(LOG_DOMAIN, "Cannot save {name}: {err}"),
            }
        };
        let close_dialog = || {
            if let Some(dialog) = self.window.visible_dialog() {
                dialog.force_close();
            }
        };

        settle(3000).await;
        shot("main").await;

        // The whole tunnel list, scrolled to the bottom.
        let scrolled = self
            .stack
            .child_by_name("list")
            .and_downcast::<gtk::ScrolledWindow>();
        if let Some(scrolled) = &scrolled {
            let adjustment = scrolled.vadjustment();
            adjustment.set_value(adjustment.upper() - adjustment.page_size());
        }
        settle(800).await;
        shot("tunnels").await;
        if let Some(scrolled) = &scrolled {
            scrolled.vadjustment().set_value(0.0);
        }

        let window = self.window.clone();
        glib::spawn_future_local(async move {
            let config = demo::config(false);
            editor::run(&window, editor::Mode::Import, "home-server", &config, None).await;
        });
        settle(1200).await;
        shot("import").await;
        close_dialog();
        settle(800).await;

        self.app.activate_action("preferences", None);
        settle(1200).await;
        if let Some(prefs) = self
            .window
            .visible_dialog()
            .and_downcast::<adw::PreferencesDialog>()
        {
            prefs.set_visible_page_name("connection");
        }
        settle(800).await;
        shot("trusted-networks").await;
        close_dialog();
        settle(800).await;

        share::show_qr(&self.window, "Amsterdam", &demo::config(true));
        settle(1200).await;
        shot("qr-code").await;
        close_dialog();
        settle(800).await;

        adw::StyleManager::default().set_color_scheme(adw::ColorScheme::ForceDark);
        settle(1500).await;
        shot("main-dark").await;
        self.app.quit();
    }

    fn show_unavailable(&self, detail: &str) {
        self.unavailable.set_description(Some(&format!(
            "{}\n\n{}",
            gettext("Tunnelbird needs NetworkManager to manage VPN tunnels. Make sure it is installed and running."),
            glib::markup_escape_text(detail)
        )));
        self.stack.set_visible_child_name("unavailable");
    }

    fn queue_refresh(self: &Rc<Self>) {
        if self.refresh_queued.replace(true) {
            return;
        }
        let ui = self.clone();
        glib::timeout_add_local_once(REFRESH_DEBOUNCE, move || {
            ui.refresh_queued.set(false);
            glib::spawn_future_local(ui.refresh());
        });
    }

    async fn refresh(self: Rc<Self>) {
        let Some(nm) = self.nm.borrow().clone() else {
            return;
        };
        match nm.tunnels().await {
            Ok(tunnels) => {
                glib::spawn_future_local(self.clone().check_network());
                self.detect_failed_connections(&tunnels);
                self.detect_drops(&tunnels);
                // Rebuilding the rows makes the list jump: only do it on real changes.
                if *self.tunnels.borrow() == tunnels {
                    return;
                }
                *self.tunnels.borrow_mut() = tunnels;
                self.render();
            }
            Err(err) => {
                glib::g_warning!(LOG_DOMAIN, "Cannot list tunnels: {err}");
                if self.stack.visible_child_name().as_deref() == Some("loading") {
                    self.show_unavailable(&nm::error_message(&err));
                } else {
                    self.report_error(
                        &gettext("Cannot read the tunnel list"),
                        &nm::error_message(&err),
                    );
                }
            }
        }
    }

    /// Applies the trusted networks rule when the computer joins a network.
    async fn check_network(self: Rc<Self>) {
        let Some(nm) = self.nm.borrow().clone() else {
            return;
        };
        let network = match nm.current_network().await {
            Ok(Some(network)) => network,
            Ok(None) => {
                *self.last_network.borrow_mut() = None;
                return;
            }
            Err(err) => {
                glib::g_debug!(LOG_DOMAIN, "Cannot read the current network: {err}");
                return;
            }
        };
        // Only on joining: a user who disconnects by hand afterwards is respected.
        if self.last_network.borrow().as_deref() == Some(network.uuid.as_str()) {
            return;
        }
        *self.last_network.borrow_mut() = Some(network.uuid.clone());

        let policy = Settings::load().trust;
        match policy.decide(&network) {
            Decision::Stay => return,
            Decision::Pin(pin) => {
                glib::g_info!(
                    LOG_DOMAIN,
                    "Strict check: trusting access point {} of “{}”",
                    pin.bssid,
                    pin.ssid
                );
                let mut settings = Settings::load();
                settings.trust.pins.push(pin);
                if let Err(err) = settings.save() {
                    glib::g_warning!(LOG_DOMAIN, "Cannot save the trusted access point: {err}");
                }
                return;
            }
            Decision::Connect => {}
        }
        let target = {
            let tunnels = self.tunnels.borrow();
            if tunnels
                .iter()
                .any(|t| matches!(t.state, TunnelState::Connected | TunnelState::Connecting))
            {
                return; // Already protected.
            }
            tunnels.iter().find(|t| t.uuid == policy.tunnel).cloned()
        };
        let Some(target) = target else {
            glib::g_warning!(
                LOG_DOMAIN,
                "Automatic connection: the chosen tunnel no longer exists"
            );
            return;
        };
        let place = match (network.kind, &network.ssid) {
            (crate::trust::NetworkKind::Wifi, Some(ssid)) => {
                gettext("Wi-Fi “{name}”").replace("{name}", ssid)
            }
            (crate::trust::NetworkKind::Wired, _) => gettext("This wired network"),
            _ => gettext("This network"),
        };
        glib::g_info!(
            LOG_DOMAIN,
            "Untrusted network, connecting “{}”",
            target.name
        );
        let notice =
            gio::Notification::new(&gettext("Connecting “{name}”").replace("{name}", &target.name));
        notice.set_body(Some(
            &gettext("{network} is not one of your trusted networks.").replace("{network}", &place),
        ));
        self.app.send_notification(Some("auto-connect"), &notice);
        self.toggle(target, true);
    }

    /// A tunnel that was connected and is now disconnected, without this app
    /// asking for it, dropped on its own (server gone, network change…): the
    /// traffic no longer goes through the VPN, so the user must know.
    fn detect_drops(&self, tunnels: &[Tunnel]) {
        let old = self.tunnels.borrow();
        let busy = self.busy.borrow();
        let mut stopped = self.user_stopped.borrow_mut();
        for tunnel in tunnels
            .iter()
            .filter(|t| t.state == TunnelState::Disconnected)
        {
            if stopped.remove(&tunnel.uuid) || busy.contains(&tunnel.uuid) {
                continue;
            }
            let was_connected = old
                .iter()
                .any(|o| o.uuid == tunnel.uuid && o.state == TunnelState::Connected);
            if was_connected {
                glib::g_warning!(LOG_DOMAIN, "Tunnel “{}” dropped", tunnel.name);
                let notice = gio::Notification::new(&gettext("VPN disconnected"));
                notice.set_body(Some(
                    &gettext(
                        "“{name}” stopped on its own. Your traffic no longer goes through the VPN.",
                    )
                    .replace("{name}", &tunnel.name),
                ));
                notice.set_priority(gio::NotificationPriority::High);
                self.app.send_notification(Some("dropped"), &notice);
            }
        }
    }

    /// NetworkManager accepts an activation request first and may fail afterwards
    /// (unreachable server, bad keys): spot tunnels that fell back to disconnected.
    fn detect_failed_connections(&self, tunnels: &[Tunnel]) {
        let mut expecting = self.expecting.borrow_mut();
        let busy = self.busy.borrow();
        for tunnel in tunnels {
            if !expecting.contains(&tunnel.uuid) {
                continue;
            }
            match tunnel.state {
                TunnelState::Connected => {
                    expecting.remove(&tunnel.uuid);
                }
                TunnelState::Disconnected if !busy.contains(&tunnel.uuid) => {
                    expecting.remove(&tunnel.uuid);
                    glib::g_warning!(LOG_DOMAIN, "Activation of “{}” failed", tunnel.name);
                    self.toast(
                        &gettext("Could not connect to “{name}”. Check the server address and your keys.")
                            .replace("{name}", &tunnel.name),
                    );
                }
                _ => {}
            }
        }
    }

    fn render(self: &Rc<Self>) {
        self.list.remove_all();
        let tunnels = self.tunnels.borrow();
        self.stack
            .set_visible_child_name(if tunnels.is_empty() { "empty" } else { "list" });
        for tunnel in tunnels.iter() {
            self.list.append(&self.tunnel_row(tunnel));
        }
        drop(tunnels);
        self.ensure_location();
        self.render_live();
    }

    fn connected(&self) -> Option<Tunnel> {
        self.tunnels
            .borrow()
            .iter()
            .find(|t| t.state == TunnelState::Connected)
            .cloned()
    }

    fn render_live(&self) {
        self.live
            .show(self.connected().as_ref(), &self.geo.borrow());
        self.update_tray();
    }

    /// Closing the window hides it while the top bar icon is available (and the
    /// user kept "Run in Background"); otherwise it quits as usual.
    fn setup_close(self: &Rc<Self>) {
        let ui = self.clone();
        self.window.connect_close_request(move |window| {
            if ui.tray.borrow().is_none() || !Settings::load().background {
                ui.app.quit();
                return glib::Propagation::Proceed;
            }
            window.set_visible(false);
            if !ui.told_background.replace(true) {
                let notice = gio::Notification::new(&gettext("Tunnelbird is still running"));
                notice.set_body(Some(&gettext(
                    "Use its icon in the top bar to switch VPN, open the window or quit.",
                )));
                ui.app.send_notification(Some("background"), &notice);
            }
            glib::Propagation::Stop
        });
    }

    /// Update notices (see `update.rs`): “What's New” after an update, then
    /// whatever tells this install method about new versions.
    fn setup_updates(self: &Rc<Self>) {
        let ui = self.clone();
        self.update_banner
            .connect_button_clicked(move |_| ui.update_clicked());
        self.whats_new();

        let install = Install::current();
        match install {
            Install::Flatpak => {
                glib::spawn_future_local(self.clone().watch_flatpak());
            }
            Install::Repository(_) | Install::Package => self.schedule_update_checks(),
            Install::Snap | Install::Source => {}
        }
        if install != Install::Flatpak {
            let ui = self.clone();
            glib::timeout_add_local(RESTART_POLL, move || {
                if update::installed_while_running(install) {
                    ui.show_update(UpdateNotice::Restart);
                    return glib::ControlFlow::Break;
                }
                glib::ControlFlow::Continue
            });
        }
    }

    /// First start of a new version: remember it and offer its release notes.
    fn whats_new(&self) {
        let mut settings = Settings::load();
        let previous = std::mem::replace(&mut settings.last_version, VERSION.to_owned());
        if previous == VERSION {
            return;
        }
        if !update::is_newer(&settings.update_available, VERSION) {
            settings.update_available.clear();
        }
        if let Err(err) = settings.save() {
            glib::g_warning!(LOG_DOMAIN, "Cannot save settings: {err}");
        }
        // Nothing to announce on the very first start or after a downgrade.
        if previous.is_empty() || !update::is_newer(VERSION, &previous) {
            return;
        }
        self.toasts.add_toast(
            adw::Toast::builder()
                .title(gettext("Tunnelbird was updated to {version}").replace("{version}", VERSION))
                .button_label(gettext("_What's New"))
                .action_name("app.about")
                .timeout(10)
                .build(),
        );
    }

    /// Installs that rely on the website: `latest.json` once a day at most.
    fn schedule_update_checks(self: &Rc<Self>) {
        // A version found before this start is still newer: no need to ask again.
        let known = Settings::load().update_available;
        if update::is_newer(&known, VERSION) {
            self.show_update(UpdateNotice::Available(Some(known)));
        }
        let ui = self.clone();
        glib::timeout_add_local_once(FIRST_UPDATE_CHECK, move || {
            ui.check_for_update();
            glib::timeout_add_local(UPDATE_TICK, move || {
                ui.check_for_update();
                glib::ControlFlow::Continue
            });
        });
    }

    fn check_for_update(self: &Rc<Self>) {
        let settings = Settings::load();
        if !settings.check_updates || !update::check_due(settings.last_update_check, unix_now()) {
            return;
        }
        let ui = self.clone();
        glib::spawn_future_local(async move {
            let latest = match update::fetch_latest(&ui.http).await {
                Ok(latest) => latest,
                // Offline or the site is down: normal, tried again within the hour.
                Err(err) => return glib::g_info!(LOG_DOMAIN, "Cannot check for updates: {err}"),
            };
            let newer = update::is_newer(&latest.version, VERSION);
            let mut settings = Settings::load();
            settings.last_update_check = unix_now();
            settings.update_available = if newer {
                latest.version.clone()
            } else {
                String::new()
            };
            if let Err(err) = settings.save() {
                glib::g_warning!(LOG_DOMAIN, "Cannot save settings: {err}");
            }
            if newer {
                ui.show_update(UpdateNotice::Available(Some(latest.version)));
            }
        });
    }

    /// Follows the Flatpak portal: Flathub has a new version, it is being
    /// installed, it is installed.
    async fn watch_flatpak(self: Rc<Self>) {
        let (monitor, events) = match FlatpakMonitor::start().await {
            Ok(started) => started,
            Err(err) => {
                return glib::g_info!(LOG_DOMAIN, "No Flatpak update monitor: {err}");
            }
        };
        *self.flatpak.borrow_mut() = Some(Rc::new(monitor));
        let mut events = std::pin::pin!(events);
        while let Some(event) = events.next().await {
            match event {
                FlatpakEvent::Available => self.show_update(UpdateNotice::Available(None)),
                FlatpakEvent::Progress(percent) => {
                    self.show_update(UpdateNotice::Installing(percent))
                }
                FlatpakEvent::Installed => self.show_update(UpdateNotice::Restart),
                FlatpakEvent::Failed(detail) => self.update_failed(&detail),
            }
        }
    }

    fn update_failed(&self, detail: &str) {
        self.update_notice.replace(UpdateNotice::None);
        self.show_update(UpdateNotice::Available(None));
        self.report_error(&gettext("Cannot update Tunnelbird"), detail);
    }

    /// Shows `notice` in the banner and the top bar, and as a notification
    /// when the window is hidden. A pending restart is never replaced.
    fn show_update(&self, notice: UpdateNotice) {
        {
            let current = self.update_notice.borrow();
            if *current == notice || *current == UpdateNotice::Restart {
                return;
            }
        }
        let install = Install::current();
        let (title, button) = match &notice {
            UpdateNotice::None => (String::new(), None),
            UpdateNotice::Available(version) => (
                match version {
                    Some(version) => {
                        gettext("Tunnelbird {version} is available").replace("{version}", version)
                    }
                    None => gettext("A new version of Tunnelbird is available"),
                },
                Some(match install {
                    Install::Flatpak => gettext("_Update"),
                    Install::Repository(_) => gettext("_How to Update"),
                    _ => gettext("_Download"),
                }),
            ),
            UpdateNotice::Installing(percent) => (
                gettext("Updating Tunnelbird… {percent}%")
                    .replace("{percent}", &percent.to_string()),
                None,
            ),
            // snapd starts the new revision itself once the app has quit.
            UpdateNotice::Restart if install == Install::Snap => (
                gettext("Tunnelbird was updated: quit it and open it again to finish"),
                Some(gettext("_Quit")),
            ),
            UpdateNotice::Restart => (
                gettext("Tunnelbird was updated: restart it to finish"),
                Some(gettext("_Restart")),
            ),
        };
        self.update_banner.set_title(&title);
        self.update_banner.set_button_label(button.as_deref());
        self.update_banner
            .set_revealed(notice != UpdateNotice::None);
        let notify = matches!(notice, UpdateNotice::Available(_) | UpdateNotice::Restart);
        if notify && !self.window.is_visible() {
            // Clicking the notification opens the window, where the banner is.
            self.app
                .send_notification(Some("update"), &gio::Notification::new(&title));
        }
        self.update_notice.replace(notice);
        self.update_tray();
    }

    fn update_clicked(self: &Rc<Self>) {
        let notice = self.update_notice.borrow().clone();
        match (notice, Install::current()) {
            (UpdateNotice::Restart, Install::Snap) => self.app.quit(),
            (UpdateNotice::Restart, _) => crate::request_restart(&self.app),
            (UpdateNotice::Available(_), Install::Flatpak) => {
                let Some(monitor) = self.flatpak.borrow().clone() else {
                    return;
                };
                self.show_update(UpdateNotice::Installing(0));
                let ui = self.clone();
                glib::spawn_future_local(async move {
                    if let Err(err) = monitor.update().await {
                        ui.update_failed(&err.to_string());
                    }
                });
            }
            (UpdateNotice::Available(_), Install::Repository(manager)) => {
                self.explain_system_update(manager)
            }
            (UpdateNotice::Available(_), _) => {
                gtk::UriLauncher::new(update::DOWNLOAD_URL).launch(
                    Some(&self.window),
                    None::<&gio::Cancellable>,
                    |result| {
                        if let Err(err) = result {
                            glib::g_warning!(LOG_DOMAIN, "Cannot open the download page: {err}");
                        }
                    },
                );
            }
            (UpdateNotice::None | UpdateNotice::Installing(_), _) => {}
        }
    }

    /// Installed from the website's repository: the update comes with the
    /// system's, so say where to find it rather than doing it.
    fn explain_system_update(self: &Rc<Self>, manager: Manager) {
        let command = match manager {
            Manager::Apt => "sudo apt update && sudo apt upgrade",
            Manager::Dnf => "sudo dnf upgrade --refresh",
        };
        let dialog = adw::AlertDialog::new(
            Some(&gettext("Update Tunnelbird")),
            Some(&gettext(
                "The new version comes with your system updates: open Software, or your update manager, and install the available updates. From a terminal:",
            )),
        );
        let label = gtk::Label::builder()
            .label(command)
            .css_classes(["monospace"])
            .wrap(true)
            .hexpand(true)
            .xalign(0.0)
            .build();
        let copy = gtk::Button::builder()
            .icon_name("edit-copy-symbolic")
            .tooltip_text(gettext("Copy"))
            .valign(gtk::Align::Center)
            .css_classes(["flat"])
            .build();
        clickable(&copy);
        copy.connect_clicked(move |button| {
            button.clipboard().set_text(command);
            button.set_icon_name("object-select-symbolic");
        });
        let row = gtk::Box::builder()
            .spacing(6)
            .css_classes(["card", "command"])
            .build();
        row.append(&label);
        row.append(&copy);
        dialog.set_extra_child(Some(&row));
        dialog.add_response("close", &gettext("_Close"));
        dialog.present(Some(&self.window));
    }

    /// Publishes the top bar icon and handles its menu clicks.
    async fn start_tray(self: Rc<Self>) {
        let (sender, mut actions) = mpsc::unbounded();
        let tray = match Tray::start(sender).await {
            Ok(tray) => tray,
            Err(err) => {
                glib::g_info!(
                    LOG_DOMAIN,
                    "No top bar icon, closing the window quits: {err}"
                );
                // Started hidden at login but nowhere to show the icon: show the window.
                if !self.window.is_visible() {
                    self.window.present();
                }
                return;
            }
        };
        *self.tray.borrow_mut() = Some(Rc::new(tray));
        *self.hold.borrow_mut() = Some(self.app.hold());
        self.update_tray();
        while let Some(action) = actions.next().await {
            self.handle_tray(action);
        }
    }

    fn handle_tray(self: &Rc<Self>, action: TrayAction) {
        match action {
            TrayAction::ShowWindow => self.window.present(),
            TrayAction::Toggle(uuid) => {
                let tunnel = self
                    .tunnels
                    .borrow()
                    .iter()
                    .find(|t| t.uuid == uuid)
                    .cloned();
                if let Some(tunnel) = tunnel {
                    let connect = !matches!(
                        tunnel.state,
                        TunnelState::Connected | TunnelState::Connecting
                    );
                    self.toggle(tunnel, connect);
                }
            }
            TrayAction::Disconnect => {
                if let Some(tunnel) = self.connected() {
                    self.toggle(tunnel, false);
                }
            }
            TrayAction::Quit => self.app.quit(),
        }
    }

    /// Mirrors the tunnels, live traffic and location into the top bar menu.
    fn update_tray(&self) {
        let Some(tray) = self.tray.borrow().clone() else {
            return;
        };
        let connected = self.connected();
        let recent = self.live.graph.recent(TRAY_SPARK_POINTS);
        let last = recent.last().copied().unwrap_or_default();
        let line = |arrow: &str, values: Vec<f64>, rate: f64| {
            format!(
                "{arrow} {}  {}",
                traffic::sparkline(&values),
                per_second(rate)
            )
        };
        let location = match (&*self.geo.borrow(), &connected) {
            (GeoState::Ready(_, location), Some(tunnel)) if tunnel.full_tunnel => {
                let (flag, place) = geo::describe(location);
                Some(format!("{flag} {place} · {}", location.ip))
            }
            _ => None,
        };
        tray.set(TrayModel {
            tunnels: self
                .tunnels
                .borrow()
                .iter()
                .map(|t| TrayTunnel {
                    uuid: t.uuid.clone(),
                    name: join_dot([Some(t.name.clone()), t.provider.map(str::to_owned)]),
                    connected: matches!(t.state, TunnelState::Connected | TunnelState::Connecting),
                })
                .collect(),
            connected: connected.is_some(),
            status: match &connected {
                Some(tunnel) if self.health_state.get() == Health::NoResponse => {
                    gettext("“{name}”: server not responding").replace("{name}", &tunnel.name)
                }
                Some(tunnel) => gettext("Connected to “{name}”").replace("{name}", &tunnel.name),
                None => gettext("Not connected"),
            },
            down: line("↓", recent.iter().map(|s| s.rx).collect(), last.rx),
            up: line("↑", recent.iter().map(|s| s.tx).collect(), last.tx),
            location,
            update: match &*self.update_notice.borrow() {
                UpdateNotice::Available(_) => Some(gettext("Update Available…")),
                UpdateNotice::Restart => Some(gettext("Restart to Finish the Update…")),
                UpdateNotice::None | UpdateNotice::Installing(_) => None,
            },
            open_label: gettext("Open Tunnelbird"),
            disconnect_label: gettext("Disconnect"),
            quit_label: gettext("Quit"),
            ..TrayModel::default()
        });
    }

    /// Starts a public IP lookup when a full tunnel becomes the connected one.
    /// A partial tunnel leaves the public IP unchanged, so it is not looked up.
    fn ensure_location(self: &Rc<Self>) {
        match self.connected() {
            Some(tunnel) if tunnel.full_tunnel => {
                if !self.geo.borrow().is_for(&tunnel.uuid) {
                    self.lookup_location(tunnel.uuid);
                }
            }
            _ => *self.geo.borrow_mut() = GeoState::Idle,
        }
    }

    fn lookup_location(self: &Rc<Self>, uuid: String) {
        *self.geo.borrow_mut() = GeoState::Loading(uuid.clone());
        self.render_live();
        let ui = self.clone();
        glib::spawn_future_local(async move {
            glib::timeout_future(LOOKUP_DELAY).await;
            let mut result = geo::lookup(&ui.http).await;
            if let Err(err) = &result {
                glib::g_debug!(LOG_DOMAIN, "Public IP lookup failed, retrying: {err}");
                glib::timeout_future(LOOKUP_RETRY).await;
                result = geo::lookup(&ui.http).await;
            }
            // The user may have switched tunnel in the meantime.
            if !matches!(&*ui.geo.borrow(), GeoState::Loading(id) if *id == uuid) {
                return;
            }
            let state = match result {
                Ok(location) => GeoState::Ready(uuid, location),
                Err(err) => {
                    glib::g_warning!(LOG_DOMAIN, "Public IP lookup failed: {err}");
                    GeoState::Failed(uuid)
                }
            };
            *ui.geo.borrow_mut() = state;
            ui.render_live();
        });
    }

    fn tunnel_row(self: &Rc<Self>, tunnel: &Tunnel) -> adw::ActionRow {
        let in_flight = self.busy.borrow().contains(&tunnel.uuid)
            || matches!(
                tunnel.state,
                TunnelState::Connecting | TunnelState::Disconnecting
            );
        let state = match tunnel.state {
            TunnelState::Connected => gettext("Connected"),
            TunnelState::Connecting => gettext("Connecting…"),
            TunnelState::Disconnecting => gettext("Disconnecting…"),
            TunnelState::Disconnected => tunnel
                .endpoint
                .clone()
                .unwrap_or_else(|| gettext("Disconnected")),
        };
        // "Mullvad · 185.65.134.82:51820" when the provider is recognised.
        let subtitle = join_dot([tunnel.provider.map(str::to_owned), Some(state)]);
        let row = adw::ActionRow::builder()
            .title(tunnel.name.as_str())
            .subtitle(subtitle)
            .use_markup(false)
            .build();

        let icon = gtk::Image::from_icon_name("network-vpn-symbolic");
        if tunnel.state == TunnelState::Connected {
            icon.add_css_class("success");
        }
        row.add_prefix(&icon);

        if in_flight {
            row.add_suffix(&gtk::Spinner::builder().spinning(true).build());
        }

        let switch = gtk::Switch::builder()
            .valign(gtk::Align::Center)
            .active(matches!(
                tunnel.state,
                TunnelState::Connected | TunnelState::Connecting
            ))
            .sensitive(!in_flight)
            .build();
        switch.update_property(&[gtk::accessible::Property::Label(
            &gettext("Connect “{name}”").replace("{name}", &tunnel.name),
        )]);
        let ui = self.clone();
        let target = tunnel.clone();
        switch.connect_state_set(move |_, on| {
            ui.toggle(target.clone(), on);
            glib::Propagation::Proceed
        });
        clickable(&switch);
        clickable(&row);
        row.add_suffix(&switch);
        row.set_activatable_widget(Some(&switch));

        // One "share" button with a small menu keeps three icons per row.
        let uuid = tunnel.uuid.to_variant();
        let menu = gio::Menu::new();
        let item = gio::MenuItem::new(Some(&gettext("Export as File…")), None);
        item.set_action_and_target_value(Some("win.export-tunnel"), Some(&uuid));
        menu.append_item(&item);
        let item = gio::MenuItem::new(Some(&gettext("Show QR Code")), None);
        item.set_action_and_target_value(Some("win.show-qr"), Some(&uuid));
        menu.append_item(&item);
        let share = gtk::MenuButton::builder()
            .icon_name("send-to-symbolic")
            .tooltip_text(gettext("Share"))
            .valign(gtk::Align::Center)
            .css_classes(["flat"])
            .menu_model(&menu)
            .sensitive(!in_flight)
            .build();
        clickable(&share);
        row.add_suffix(&share);

        let edit = gtk::Button::builder()
            .icon_name("document-edit-symbolic")
            .tooltip_text(gettext("Edit"))
            .valign(gtk::Align::Center)
            .css_classes(["flat"])
            .sensitive(!in_flight)
            .build();
        let ui = self.clone();
        let target = tunnel.clone();
        edit.connect_clicked(move |_| ui.edit_tunnel(target.clone()));
        clickable(&edit);
        row.add_suffix(&edit);

        let delete = gtk::Button::builder()
            .icon_name("user-trash-symbolic")
            .tooltip_text(gettext("Delete"))
            .valign(gtk::Align::Center)
            .css_classes(["flat"])
            .sensitive(!in_flight)
            .build();
        let ui = self.clone();
        let target = tunnel.clone();
        delete.connect_clicked(move |_| ui.confirm_delete(target.clone()));
        clickable(&delete);
        row.add_suffix(&delete);

        row
    }

    fn toggle(self: &Rc<Self>, tunnel: Tunnel, connect: bool) {
        let Some(nm) = self.nm.borrow().clone() else {
            return;
        };
        // Only one tunnel at a time: connecting stops the others first.
        let others: Vec<Tunnel> = if connect {
            self.tunnels
                .borrow()
                .iter()
                .filter(|t| t.uuid != tunnel.uuid && t.active_path.is_some())
                .cloned()
                .collect()
        } else {
            Vec::new()
        };
        {
            let mut busy = self.busy.borrow_mut();
            busy.insert(tunnel.uuid.clone());
            busy.extend(others.iter().map(|t| t.uuid.clone()));
            let mut stopped = self.user_stopped.borrow_mut();
            stopped.extend(others.iter().map(|t| t.uuid.clone()));
            if !connect {
                stopped.insert(tunnel.uuid.clone());
            }
        }
        if connect {
            self.expecting.borrow_mut().insert(tunnel.uuid.clone());
        }
        let ui = self.clone();
        // Runs on the next main loop iteration, so the switch emitting this
        // signal is not destroyed by `render` while it is still in use.
        glib::spawn_future_local(async move {
            ui.render();
            for other in &others {
                let result = nm.deactivate(other).await;
                ui.busy.borrow_mut().remove(&other.uuid);
                if let Err(err) = result {
                    ui.busy.borrow_mut().remove(&tunnel.uuid);
                    ui.expecting.borrow_mut().remove(&tunnel.uuid);
                    ui.report_error(
                        &gettext("Cannot disconnect “{name}”").replace("{name}", &other.name),
                        &nm::error_message(&err),
                    );
                    ui.refresh().await;
                    return;
                }
                glib::g_info!(
                    LOG_DOMAIN,
                    "Disconnected “{}” to connect “{}”",
                    other.name,
                    tunnel.name
                );
            }
            if let Some(other) = others.first() {
                ui.toast(
                    &gettext("“{old}” disconnected, connecting “{new}”")
                        .replace("{old}", &other.name)
                        .replace("{new}", &tunnel.name),
                );
            }
            let result = if connect {
                nm.activate(&tunnel).await
            } else {
                nm.deactivate(&tunnel).await
            };
            ui.busy.borrow_mut().remove(&tunnel.uuid);
            if let Err(err) = result {
                ui.expecting.borrow_mut().remove(&tunnel.uuid);
                let context = if connect {
                    gettext("Cannot connect “{name}”")
                } else {
                    gettext("Cannot disconnect “{name}”")
                };
                ui.report_error(
                    &context.replace("{name}", &tunnel.name),
                    &nm::error_message(&err),
                );
            }
            ui.refresh().await;
        });
    }

    /// Opens the editor on an existing tunnel and saves the changes.
    /// Exports a saved tunnel as a file, or shows its QR code.
    fn share_tunnel(self: &Rc<Self>, uuid: &str, qr: bool) {
        let tunnel = self
            .tunnels
            .borrow()
            .iter()
            .find(|t| t.uuid == uuid)
            .cloned();
        let (Some(tunnel), Some(nm)) = (tunnel, self.nm.borrow().clone()) else {
            return;
        };
        let ui = self.clone();
        glib::spawn_future_local(async move {
            let context = gettext("Cannot share “{name}”").replace("{name}", &tunnel.name);
            let config = match nm.read_config(&tunnel).await {
                Ok(config) => config,
                Err(err) => return ui.report_error(&context, &nm::error_message(&err)),
            };
            if qr {
                return share::show_qr(&ui.window, &tunnel.name, &config);
            }
            let window = ui.window.clone().upcast::<gtk::Window>();
            match share::export_file(&window, &tunnel.name, &config).await {
                Ok(Some(path)) => ui.toast(&gettext("Saved to {path}").replace("{path}", &path)),
                Ok(None) => {}
                Err(message) => ui.report_error(&context, &message),
            }
        });
    }

    fn edit_tunnel(self: &Rc<Self>, tunnel: Tunnel) {
        let Some(nm) = self.nm.borrow().clone() else {
            return;
        };
        let ui = self.clone();
        glib::spawn_future_local(async move {
            let context = gettext("Cannot edit “{name}”").replace("{name}", &tunnel.name);
            let current = match nm.read_config(&tunnel).await {
                Ok(config) => config,
                Err(err) => return ui.report_error(&context, &nm::error_message(&err)),
            };
            let Some(edited) = editor::run(
                &ui.window,
                editor::Mode::Edit,
                &tunnel.name,
                &current,
                tunnel.provider,
            )
            .await
            else {
                return;
            };
            if let Err(err) = nm.update(&tunnel, &edited.config, &edited.name).await {
                return ui.report_error(&context, &nm::error_message(&err));
            }
            glib::g_info!(LOG_DOMAIN, "Tunnel “{}” updated", edited.name);
            ui.clone().refresh().await;
            if tunnel.state != TunnelState::Connected {
                return ui.toast(&gettext("“{name}” saved").replace("{name}", &edited.name));
            }
            // An active tunnel keeps its old settings until it reconnects.
            let toast = adw::Toast::builder()
                .title(gettext("Saved. Reconnect to apply the changes."))
                .button_label(gettext("_Reconnect"))
                .timeout(10)
                .build();
            let ui_for_button = ui.clone();
            toast.connect_button_clicked(move |_| ui_for_button.reconnect(&tunnel.uuid));
            ui.toasts.add_toast(toast);
        });
    }

    fn reconnect(self: &Rc<Self>, uuid: &str) {
        let tunnel = self
            .tunnels
            .borrow()
            .iter()
            .find(|t| t.uuid == uuid)
            .cloned();
        let (Some(tunnel), Some(nm)) = (tunnel, self.nm.borrow().clone()) else {
            return;
        };
        self.user_stopped.borrow_mut().insert(tunnel.uuid.clone());
        let ui = self.clone();
        glib::spawn_future_local(async move {
            let result = async {
                nm.deactivate(&tunnel).await?;
                nm.activate(&tunnel).await
            };
            if let Err(err) = result.await {
                ui.report_error(
                    &gettext("Cannot connect “{name}”").replace("{name}", &tunnel.name),
                    &nm::error_message(&err),
                );
            }
            ui.refresh().await;
        });
    }

    fn confirm_delete(self: &Rc<Self>, tunnel: Tunnel) {
        let dialog = adw::AlertDialog::new(
            Some(&gettext("Delete “{name}”?").replace("{name}", &tunnel.name)),
            Some(&gettext(
                "The tunnel will be removed from this computer. You can import the file again later.",
            )),
        );
        dialog.add_responses(&[
            ("cancel", &gettext("_Cancel")),
            ("delete", &gettext("_Delete")),
        ]);
        dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");
        let ui = self.clone();
        dialog.connect_response(None, move |_, response| {
            if response != "delete" {
                return;
            }
            let Some(nm) = ui.nm.borrow().clone() else {
                return;
            };
            let ui = ui.clone();
            let tunnel = tunnel.clone();
            glib::spawn_future_local(async move {
                match nm.delete(&tunnel).await {
                    Ok(()) => {
                        ui.toast(&gettext("“{name}” deleted").replace("{name}", &tunnel.name))
                    }
                    Err(err) => ui.report_error(
                        &gettext("Cannot delete “{name}”").replace("{name}", &tunnel.name),
                        &nm::error_message(&err),
                    ),
                }
                ui.refresh().await;
            });
        });
        dialog.present(Some(&self.window));
    }

    fn open_import_dialog(self: &Rc<Self>) {
        let filter = gtk::FileFilter::new();
        filter.set_name(Some(&gettext("VPN Configuration")));
        filter.add_pattern("*.conf");
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let dialog = gtk::FileDialog::builder()
            .title(gettext("Import VPN Configuration"))
            .filters(&filters)
            .modal(true)
            .build();

        let ui = self.clone();
        glib::spawn_future_local(async move {
            match dialog.open_multiple_future(Some(&ui.window)).await {
                Ok(model) => {
                    let files = model.iter::<gio::File>().filter_map(Result::ok).collect();
                    ui.import_files(files);
                }
                Err(err) if err.matches(gtk::DialogError::Dismissed) => {}
                Err(err) => {
                    ui.report_error(&gettext("Cannot open the file chooser"), &err.to_string())
                }
            }
        });
    }

    /// Imports files one after the other, then refreshes the list once.
    pub fn import_files(self: &Rc<Self>, files: Vec<gio::File>) {
        let ui = self.clone();
        glib::spawn_future_local(async move {
            for file in files {
                ui.import_file(&file).await;
            }
            ui.refresh().await;
        });
    }

    async fn import_file(&self, file: &gio::File) {
        let label = file
            .basename()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| file.uri().to_string());
        let context = gettext("Cannot import “{file}”").replace("{file}", &label);

        let bytes = match file.load_contents_future().await {
            Ok((bytes, _etag)) => bytes,
            Err(err) => return self.report_error(&context, &err.to_string()),
        };
        let config = match config::parse_bytes(&bytes) {
            Ok(config) => config,
            Err(err) => return self.report_error(&context, &err.to_string()),
        };
        let Some(nm) = self.nm.borrow().clone() else {
            return self.report_error(&context, &gettext("NetworkManager is not available"));
        };

        let stem = file
            .basename()
            .and_then(|path| path.file_stem().map(|s| s.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "vpn".to_owned());
        // Nothing is saved before the user has seen and confirmed the tunnel.
        let provider = detect_provider(&config, &stem);
        let Some(edited) =
            editor::run(&self.window, editor::Mode::Import, &stem, &config, provider).await
        else {
            glib::g_info!(LOG_DOMAIN, "Import of “{label}” cancelled");
            return;
        };
        match nm
            .import(
                &edited.config,
                &edited.name,
                &config::interface_name(&edited.name),
            )
            .await
        {
            Ok(name) => {
                glib::g_info!(LOG_DOMAIN, "Imported “{label}” as “{name}”");
                let mut message = gettext("“{name}” imported").replace("{name}", &name);
                if !config.ignored.is_empty() {
                    message.push_str(" · ");
                    message.push_str(
                        &gettext("ignored settings: {keys}")
                            .replace("{keys}", &config.ignored.join(", ")),
                    );
                }
                self.toast(&message);
            }
            Err(err) => self.report_error(&context, &nm::error_message(&err)),
        }
    }

    /// Takes one traffic sample of the connected tunnel (called every second).
    fn sample_traffic(&self) {
        let tunnels = self.tunnels.borrow();
        let Some(tunnel) = tunnels.iter().find(|t| t.state == TunnelState::Connected) else {
            self.health.borrow_mut().reset();
            self.set_health(Health::Unknown, None);
            return;
        };
        let counters = if demo::enabled() {
            Some(demo::counters())
        } else {
            traffic::read_counters(&tunnel.interface)
        };
        let Some(counters) = counters else {
            glib::g_debug!(LOG_DOMAIN, "No counters for interface {}", tunnel.interface);
            return;
        };
        let (rx, tx) = (counters.rx_bytes, counters.tx_bytes);
        let now = Instant::now();
        let mut last = self.live.last.borrow_mut();
        if let Some((interface, last_rx, last_tx, at)) = last.as_ref()
            && *interface == tunnel.interface
        {
            let seconds = now.duration_since(*at).as_secs_f64().max(0.001);
            self.live.update(
                Sample {
                    rx: rx.saturating_sub(*last_rx) as f64 / seconds,
                    tx: tx.saturating_sub(*last_tx) as f64 / seconds,
                },
                rx,
                tx,
            );
        }
        *last = Some((tunnel.interface.clone(), rx, tx, now));
        drop(last);
        let health = self.health.borrow_mut().record(
            &tunnel.interface,
            counters.rx_packets,
            counters.tx_packets,
        );
        let name = tunnel.name.clone();
        drop(tunnels);
        self.set_health(health, Some(&name));
        self.update_tray();
    }

    /// Shows the server health; warns once when it stops answering.
    fn set_health(&self, health: Health, tunnel: Option<&str>) {
        let previous = self.health_state.replace(health);
        self.live.show_health(health);
        if health == previous {
            return;
        }
        match (health, tunnel) {
            (Health::NoResponse, Some(name)) => {
                glib::g_warning!(LOG_DOMAIN, "Server of “{name}” is not responding");
                let notice = gio::Notification::new(&gettext("VPN server not responding"));
                notice.set_body(Some(
                    &gettext("“{name}” is connected, but its server does not answer. Check your internet connection or try another server.")
                        .replace("{name}", name),
                ));
                self.app.send_notification(Some("no-response"), &notice);
            }
            _ if previous == Health::NoResponse => {
                self.app.withdraw_notification("no-response");
            }
            _ => {}
        }
    }

    /// Logs a failure and shows it: the toast carries the short context, the
    /// "Details" button opens the full message (toasts are a single line).
    fn report_error(&self, context: &str, detail: &str) {
        glib::g_warning!(LOG_DOMAIN, "{context}: {detail}");
        // Actions started from the top bar may fail while the window is hidden.
        if !self.window.is_visible() {
            let notice = gio::Notification::new(context);
            notice.set_body(Some(detail));
            self.app.send_notification(None, &notice);
        }
        let toast = adw::Toast::builder()
            .title(context)
            .use_markup(false)
            .button_label(gettext("_Details"))
            .timeout(8)
            .build();
        let window = self.window.clone();
        let (context, detail) = (context.to_owned(), detail.to_owned());
        toast.connect_button_clicked(move |_| {
            let dialog = adw::AlertDialog::new(Some(&context), Some(&detail));
            dialog.add_response("close", &gettext("_Close"));
            dialog.present(Some(&window));
        });
        self.toasts.add_toast(toast);
    }

    fn toast(&self, message: &str) {
        self.toasts.add_toast(
            adw::Toast::builder()
                .title(message)
                .use_markup(false)
                .timeout(5)
                .build(),
        );
    }
}

impl LiveCard {
    fn new() -> Self {
        let name = gtk::Label::builder()
            .xalign(0.0)
            .css_classes(["title-3"])
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        let endpoint = gtk::Label::builder()
            .xalign(0.0)
            .css_classes(["dim-label", "caption"])
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        let titles = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(2)
            .hexpand(true)
            .build();
        titles.append(&name);
        titles.append(&endpoint);
        let status = gtk::Label::builder()
            .label(gettext("Connected"))
            .css_classes(["success", "heading"])
            .valign(gtk::Align::Start)
            .build();
        let top = gtk::Box::builder().spacing(12).build();
        top.append(&titles);
        top.append(&status);
        let status = status.clone();

        let caption = |classes: &[&str]| {
            gtk::Label::builder()
                .xalign(0.0)
                .wrap(true)
                .css_classes(classes.to_vec())
                .build()
        };
        let vpn_address = single_line(&["caption", "numeric"]);
        let flag = gtk::Label::builder()
            .css_classes(["title-1"])
            .valign(gtk::Align::Center)
            .build();
        flag.update_property(&[gtk::accessible::Property::Label(&gettext("Country flag"))]);
        let place = gtk::Label::builder()
            .xalign(0.0)
            .css_classes(["heading"])
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        let public_ip = single_line(&["dim-label", "caption", "numeric"]);
        let place_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(2)
            .hexpand(true)
            .valign(gtk::Align::Center)
            .build();
        place_box.append(&place);
        place_box.append(&public_ip);
        let locating = gtk::Spinner::new();
        let relocate = gtk::Button::builder()
            .icon_name("view-refresh-symbolic")
            .tooltip_text(gettext("Check Location Again"))
            .action_name("win.relocate")
            .valign(gtk::Align::Center)
            .css_classes(["flat", "circular"])
            .build();
        clickable(&relocate);
        let location = gtk::Box::builder().spacing(12).build();
        location.append(&flag);
        location.append(&place_box);
        location.append(&locating);
        location.append(&relocate);
        let partial = caption(&["dim-label", "caption"]);
        partial.set_label(&gettext(
            "Only traffic to the VPN network goes through this tunnel: your public IP address does not change.",
        ));

        let graph = TrafficGraph::new();
        graph
            .widget()
            .update_property(&[gtk::accessible::Property::Label(&gettext(
                "Traffic over the last minute",
            ))]);

        // Rates change every second: tabular digits and reserved widths keep
        // the layout still instead of shifting with each new value.
        let rate = || {
            gtk::Label::builder()
                .xalign(0.0)
                .use_markup(true)
                .width_chars(RATE_WIDTH_CHARS)
                .css_classes(["numeric"])
                .build()
        };
        let rx_rate = rate();
        let tx_rate = rate();
        let scale = gtk::Label::builder()
            .xalign(1.0)
            .hexpand(true)
            .width_chars(SCALE_WIDTH_CHARS)
            .css_classes(["dim-label", "caption", "numeric"])
            .build();
        let legend = gtk::Box::builder().spacing(18).build();
        legend.append(&rx_rate);
        legend.append(&tx_rate);
        legend.append(&scale);

        let totals = single_line(&["dim-label", "caption", "numeric"]);

        let card = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .css_classes(["card"])
            .build();
        let inner = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .margin_top(18)
            .margin_bottom(18)
            .margin_start(18)
            .margin_end(18)
            .build();
        inner.append(&top);
        inner.append(&location);
        inner.append(&vpn_address);
        inner.append(&partial);
        inner.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        inner.append(graph.widget());
        inner.append(&legend);
        inner.append(&totals);
        card.append(&inner);

        let group = adw::PreferencesGroup::builder()
            .title(gettext("Active Connection"))
            .visible(false)
            .build();
        group.add(&card);

        let live = Self {
            group,
            name,
            endpoint,
            vpn_address,
            location,
            flag,
            place,
            public_ip,
            locating,
            relocate,
            partial,
            graph,
            rx_rate,
            tx_rate,
            scale,
            totals,
            last: RefCell::new(None),
            status,
        };
        live.update(Sample::default(), 0, 0);
        live
    }

    fn show(&self, tunnel: Option<&Tunnel>, geo: &GeoState) {
        let Some(tunnel) = tunnel else {
            self.group.set_visible(false);
            self.reset();
            return;
        };
        let switched = self
            .last
            .borrow()
            .as_ref()
            .is_some_and(|(iface, ..)| *iface != tunnel.interface);
        if switched {
            self.reset();
        }
        self.name.set_label(&tunnel.name);
        let server = join_dot([
            Some(config::TUNNEL_TYPE.to_owned()),
            tunnel.provider.map(str::to_owned),
            tunnel.endpoint.clone(),
        ]);
        self.endpoint.set_visible(!server.is_empty());
        self.endpoint.set_label(&server);
        self.show_addresses(tunnel, geo);
        self.group.set_visible(true);
    }

    fn show_addresses(&self, tunnel: &Tunnel, geo: &GeoState) {
        self.vpn_address.set_visible(!tunnel.addresses.is_empty());
        self.vpn_address.set_label(
            &gettext("Address in the VPN: {address}")
                .replace("{address}", &tunnel.addresses.join(", ")),
        );
        self.partial.set_visible(!tunnel.full_tunnel);
        self.location.set_visible(tunnel.full_tunnel);
        if !tunnel.full_tunnel {
            return;
        }

        let (flag, place, public_ip, loading) = match geo {
            GeoState::Ready(_, location) => {
                let (flag, place) = geo::describe(location);
                let mut ip = gettext("Public IP: {ip}").replace("{ip}", &location.ip);
                if let Some(org) = location.asn_org.as_deref().filter(|o| !o.is_empty()) {
                    ip.push_str(" · ");
                    ip.push_str(org);
                }
                let place = if place.is_empty() {
                    gettext("Unknown location")
                } else {
                    place
                };
                (flag, place, ip, false)
            }
            GeoState::Failed(_) => (
                "🌐".to_owned(),
                gettext("Location unavailable"),
                gettext("Check your connection, then try again."),
                false,
            ),
            GeoState::Loading(_) | GeoState::Idle => {
                ("🌐".to_owned(), gettext("Locating…"), String::new(), true)
            }
        };
        self.flag.set_label(&flag);
        self.place.set_label(&place);
        self.public_ip.set_label(&public_ip);
        self.public_ip.set_visible(!public_ip.is_empty());
        self.locating.set_visible(loading);
        self.locating.set_spinning(loading);
        self.relocate.set_visible(!loading);
    }

    fn show_health(&self, health: Health) {
        let not_responding = health == Health::NoResponse;
        self.status.set_label(&if not_responding {
            gettext("Server not responding")
        } else {
            gettext("Connected")
        });
        self.status.set_css_classes(if not_responding {
            &["warning", "heading"]
        } else {
            &["success", "heading"]
        });
    }

    fn reset(&self) {
        *self.last.borrow_mut() = None;
        self.graph.clear();
        self.update(Sample::default(), 0, 0);
    }

    fn update(&self, sample: Sample, total_rx: u64, total_tx: u64) {
        if total_rx > 0 || total_tx > 0 {
            self.graph.push(sample);
        }
        let (rx_color, tx_color) = traffic::series_colors();
        let legend = |color, label: String, rate: f64| {
            format!(
                "<span foreground=\"{}\">●</span> {} <b>{}</b>",
                traffic::hex_color(color),
                glib::markup_escape_text(&label),
                glib::markup_escape_text(&per_second(rate))
            )
        };
        self.rx_rate
            .set_markup(&legend(rx_color, gettext("Download"), sample.rx));
        self.tx_rate
            .set_markup(&legend(tx_color, gettext("Upload"), sample.tx));
        self.scale
            .set_label(&gettext("scale {rate}").replace("{rate}", &per_second(self.graph.scale())));
        self.totals.set_label(
            &gettext("Since connected: {rx} received · {tx} sent")
                .replace("{rx}", &glib::format_size(total_rx))
                .replace("{tx}", &glib::format_size(total_tx)),
        );
    }
}

/// Provider of a configuration about to be imported, from its server, DNS and file name.
fn detect_provider(config: &WgConfig, name: &str) -> Option<&'static str> {
    let dns: Vec<String> = config
        .interface
        .dns_servers
        .iter()
        .map(ToString::to_string)
        .collect();
    crate::provider::detect(&crate::provider::Hints {
        name,
        endpoint: config.peers.first().and_then(|p| p.endpoint.as_deref()),
        dns: &dns,
    })
}

/// Joins the present parts with " · ".
fn join_dot<const N: usize>(parts: [Option<String>; N]) -> String {
    parts
        .into_iter()
        .flatten()
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

/// A one-line label that ellipsizes instead of wrapping, so its height never changes.
fn single_line(classes: &[&str]) -> gtk::Label {
    gtk::Label::builder()
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .css_classes(classes.to_vec())
        .build()
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64)
}

fn per_second(bytes: f64) -> String {
    gettext("{size}/s").replace("{size}", &glib::format_size(bytes.round() as u64))
}

fn pill_button(label: &str, action: &str) -> gtk::Button {
    let button = gtk::Button::builder()
        .label(label)
        .use_underline(true)
        .action_name(action)
        .halign(gtk::Align::Center)
        .css_classes(["pill", "suggested-action"])
        .build();
    clickable(&button);
    button
}

/// Shows a hand cursor over something clickable. GNOME apps usually keep the
/// arrow; Tunnelbird uses the hand so actions are obvious at a glance.
pub fn clickable(widget: &impl IsA<gtk::Widget>) {
    widget.set_cursor_from_name(Some("pointer"));
}
