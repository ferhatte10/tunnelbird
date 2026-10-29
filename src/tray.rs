// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

//! Top bar icon and menu, like other background apps (Docker Desktop, Steam…).
//!
//! Implements two small D-Bus protocols on the session bus with the zbus we
//! already use, instead of a tray crate that would pull a second zbus:
//! - `org.kde.StatusNotifierItem`: the icon itself (shown on GNOME by the
//!   AppIndicator extension, enabled by default on Ubuntu; native on KDE);
//! - `com.canonical.dbusmenu`: its menu.
//!
//! zbus serves both interfaces from its own thread, so they read a shared
//! [`TrayModel`] behind a mutex and send user clicks back to the GTK thread
//! through a channel ([`TrayAction`]).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use futures_channel::mpsc::UnboundedSender;
use futures_util::StreamExt;
use gtk::gdk_pixbuf::Pixbuf;
use gtk::{gio, glib};
use zbus::object_server::SignalEmitter;
use zbus::wire::StructureBuilder;
use zbus::{Connection, OwnedObjectPath, OwnedVariant, Variant, fdo, interface};

use crate::LOG_DOMAIN;

const ITEM_PATH: &str = "/StatusNotifierItem";
/// Top bar icons (full colour), embedded so the pixmap fallback never depends on install paths.
const ICON_CONNECTED: &str =
    include_str!("../data/icons/hicolor/scalable/status/fr.ferhatsn.Tunnelbird-tray-connected.svg");
const ICON_DISCONNECTED: &str = include_str!(
    "../data/icons/hicolor/scalable/status/fr.ferhatsn.Tunnelbird-tray-disconnected.svg"
);
const MENU_PATH: &str = "/MenuBar";

// Fixed menu item ids; tunnels use TUNNEL_BASE + their index.
const ID_STATUS: i32 = 1;
const ID_DOWN: i32 = 2;
const ID_UP: i32 = 3;
const ID_LOCATION: i32 = 4;
const ID_OPEN: i32 = 10;
const ID_DISCONNECT: i32 = 11;
const ID_QUIT: i32 = 12;
const ID_SEPARATORS: [i32; 3] = [20, 21, 22];
const TUNNEL_BASE: i32 = 100;

/// What the user clicked in the top bar menu.
#[derive(Debug, Clone)]
pub enum TrayAction {
    ShowWindow,
    /// Connect this tunnel (others are disconnected), or disconnect it if it is the active one.
    Toggle(String),
    Disconnect,
    Quit,
}

#[derive(Debug, Clone, Default)]
pub struct TrayTunnel {
    pub uuid: String,
    pub name: String,
    pub connected: bool,
}

/// Everything the menu shows. Labels are already translated by the GTK side.
#[derive(Debug, Clone, Default)]
pub struct TrayModel {
    pub tunnels: Vec<TrayTunnel>,
    pub connected: bool,
    pub status: String,
    pub down: String,
    pub up: String,
    pub location: Option<String>,
    pub open_label: String,
    pub disconnect_label: String,
    pub quit_label: String,
    /// Menu revision; [`Tray::set`] overwrites it so hosts know the layout changed.
    pub revision: u32,
}

type Shared = Arc<Mutex<TrayModel>>;
/// One icon image for StatusNotifierItem: width, height, ARGB32 pixels in network order.
type Pixmap = (i32, i32, Vec<u8>);
/// StatusNotifierItem tooltip: icon name, icon pixmaps, title, description.
type ToolTip = (String, Vec<Pixmap>, String, String);

/// A poisoned lock only means another thread panicked mid-update: the data is
/// still usable, and a tray glitch must never take the app down.
fn lock(model: &Shared) -> MutexGuard<'_, TrayModel> {
    model
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub struct Tray {
    conn: Connection,
    model: Shared,
}

impl Tray {
    /// Publishes the icon. Fails when no tray host is running (for instance
    /// GNOME without the AppIndicator extension): the caller then keeps the
    /// usual "closing the window quits" behaviour.
    pub async fn start(actions: UnboundedSender<TrayAction>) -> zbus::Result<Self> {
        let model: Shared = Arc::default();
        let pixmaps = Arc::new([
            render_pixmaps(ICON_DISCONNECTED),
            render_pixmaps(ICON_CONNECTED),
        ]);
        let conn = zbus::connection::Builder::session()
            .serve_at(
                ITEM_PATH,
                Item {
                    model: model.clone(),
                    actions: actions.clone(),
                    pixmaps,
                },
            )
            .serve_at(
                MENU_PATH,
                Menu {
                    model: model.clone(),
                    actions,
                },
            )
            .build()
            .await?;
        // Registered under the connection's unique name (":1.42"), which the
        // specification allows: no well-known name to own, so no clash between
        // apps and no extra Flatpak permission.
        let name = conn
            .unique_name()
            .map(ToString::to_string)
            .ok_or_else(|| zbus::Error::Failure("no unique bus name".to_owned()))?;
        register(&conn, &name).await?;
        watch_host_restarts(conn.clone(), name);
        Ok(Self { conn, model })
    }

    /// Replaces the menu content and tells the tray host to redraw it.
    pub fn set(&self, mut model: TrayModel) {
        let (icon_changed, revision) = {
            let mut current = lock(&self.model);
            model.revision = current.revision.wrapping_add(1);
            let icon_changed = current.connected != model.connected;
            *current = model;
            (icon_changed, current.revision)
        };
        let conn = self.conn.clone();
        glib::spawn_future_local(async move {
            if let Err(err) = emit_updates(&conn, revision, icon_changed).await {
                glib::g_debug!(LOG_DOMAIN, "Cannot update the top bar menu: {err}");
            }
        });
    }
}

const WATCHER: &str = "org.kde.StatusNotifierWatcher";

async fn register(conn: &Connection, name: &str) -> zbus::Result<()> {
    conn.call_method(
        Some(WATCHER),
        "/StatusNotifierWatcher",
        Some(WATCHER),
        "RegisterStatusNotifierItem",
        &(name,),
    )
    .await?;
    Ok(())
}

/// The tray host forgets every icon when it restarts (GNOME reloads its
/// extensions on screen lock/unlock, KDE when plasmashell restarts): register
/// again each time it comes back, so the icon stays as long as the app runs.
fn watch_host_restarts(conn: Connection, name: String) {
    glib::spawn_future_local(async move {
        let changes = async {
            let proxy = fdo::DBusProxy::new(&conn).await?;
            proxy
                .receive_name_owner_changed_with_args(&[(0, WATCHER)])
                .await
        };
        let mut changes = match changes.await {
            Ok(stream) => stream,
            Err(err) => {
                glib::g_warning!(LOG_DOMAIN, "Cannot watch the top bar host: {err}");
                return;
            }
        };
        while let Some(signal) = changes.next().await {
            let came_back = signal.args().is_ok_and(|args| args.new_owner().is_some());
            if !came_back {
                continue;
            }
            match register(&conn, &name).await {
                Ok(()) => {
                    glib::g_info!(LOG_DOMAIN, "Top bar host restarted, icon registered again")
                }
                Err(err) => {
                    glib::g_warning!(LOG_DOMAIN, "Cannot register the top bar icon again: {err}")
                }
            }
        }
    });
}

async fn emit_updates(conn: &Connection, revision: u32, icon_changed: bool) -> zbus::Result<()> {
    let menu = SignalEmitter::new(conn, MENU_PATH)?;
    menu.emit("com.canonical.dbusmenu", "LayoutUpdated", &(revision, 0i32))
        .await?;
    let item = SignalEmitter::new(conn, ITEM_PATH)?;
    item.emit("org.kde.StatusNotifierItem", "NewToolTip", &())
        .await?;
    if icon_changed {
        item.emit("org.kde.StatusNotifierItem", "NewIcon", &())
            .await?;
    }
    Ok(())
}

fn icon_name(model: &TrayModel) -> String {
    if model.connected {
        format!("{}-tray-connected", crate::APP_ID)
    } else {
        format!("{}-tray-disconnected", crate::APP_ID)
    }
}

/// `org.kde.StatusNotifierItem`: the icon.
struct Item {
    model: Shared,
    actions: UnboundedSender<TrayAction>,
    /// Pre-rendered fallback images: `[disconnected, connected]`.
    pixmaps: Arc<[Vec<Pixmap>; 2]>,
}

#[interface(name = "org.kde.StatusNotifierItem")]
impl Item {
    fn activate(&self, _x: i32, _y: i32) {
        send(&self.actions, TrayAction::ShowWindow);
    }

    fn secondary_activate(&self, _x: i32, _y: i32) {
        send(&self.actions, TrayAction::ShowWindow);
    }

    fn scroll(&self, _delta: i32, _orientation: &str) {}

    fn context_menu(&self, _x: i32, _y: i32) {}

    #[zbus(property)]
    fn category(&self) -> &str {
        "ApplicationStatus"
    }

    #[zbus(property)]
    fn id(&self) -> &str {
        "tunnelbird"
    }

    #[zbus(property)]
    fn title(&self) -> &str {
        "Tunnelbird"
    }

    #[zbus(property)]
    fn status(&self) -> &str {
        "Active"
    }

    #[zbus(property)]
    fn window_id(&self) -> i32 {
        0
    }

    #[zbus(property)]
    fn icon_name(&self) -> String {
        icon_name(&lock(&self.model))
    }

    /// Lets `cargo run` find icons that are not installed yet. Tray hosts look
    /// for the file directly in this folder, not in a `hicolor` tree.
    #[zbus(property)]
    fn icon_theme_path(&self) -> &str {
        if cfg!(debug_assertions) {
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/data/icons/hicolor/scalable/status"
            )
        } else {
            ""
        }
    }

    /// Used by tray hosts when the icon name cannot be found (e.g. not installed).
    #[zbus(property)]
    fn icon_pixmap(&self) -> Vec<Pixmap> {
        let connected = lock(&self.model).connected;
        self.pixmaps[usize::from(connected)].clone()
    }

    #[zbus(property)]
    fn tool_tip(&self) -> ToolTip {
        let model = lock(&self.model);
        (
            icon_name(&model),
            Vec::new(),
            "Tunnelbird".to_owned(),
            model.status.clone(),
        )
    }

    #[zbus(property)]
    fn item_is_menu(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn menu(&self) -> OwnedObjectPath {
        OwnedObjectPath::try_from(MENU_PATH).unwrap_or_default()
    }
}

/// `com.canonical.dbusmenu`: the menu, rebuilt from the model on each request.
struct Menu {
    model: Shared,
    actions: UnboundedSender<TrayAction>,
}

/// One menu entry, as `(ia{sv}av)` on the wire.
#[derive(serde::Serialize, zbus::wire::Type)]
struct Layout {
    id: i32,
    properties: HashMap<String, OwnedVariant>,
    children: Vec<OwnedVariant>,
}

#[interface(name = "com.canonical.dbusmenu")]
impl Menu {
    fn get_layout(
        &self,
        _parent_id: i32,
        _recursion_depth: i32,
        _property_names: Vec<String>,
    ) -> fdo::Result<(u32, Layout)> {
        let (revision, entries) = {
            let model = lock(&self.model);
            (model.revision, entries(&model))
        };
        let children = entries
            .iter()
            .map(|(id, props)| child_variant(*id, props))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| fdo::Error::Failed(err.to_string()))?;
        let root = HashMap::from([("children-display".to_owned(), owned("submenu"))]);
        Ok((
            revision,
            Layout {
                id: 0,
                properties: root,
                children,
            },
        ))
    }

    fn get_group_properties(
        &self,
        ids: Vec<i32>,
        _property_names: Vec<String>,
    ) -> Vec<(i32, HashMap<String, OwnedVariant>)> {
        entries(&lock(&self.model))
            .into_iter()
            .filter(|(id, _)| ids.is_empty() || ids.contains(id))
            .collect()
    }

    fn get_property(&self, id: i32, name: &str) -> fdo::Result<OwnedVariant> {
        entries(&lock(&self.model))
            .into_iter()
            .find(|(entry, _)| *entry == id)
            .and_then(|(_, mut props)| props.remove(name))
            .ok_or_else(|| fdo::Error::InvalidArgs(format!("no property {name} on item {id}")))
    }

    fn event(&self, id: i32, event_id: &str, _data: OwnedVariant, _timestamp: u32) {
        if event_id == "clicked" {
            self.clicked(id);
        }
    }

    fn event_group(&self, events: Vec<(i32, String, OwnedVariant, u32)>) -> Vec<i32> {
        for (id, event_id, _, _) in &events {
            if event_id == "clicked" {
                self.clicked(*id);
            }
        }
        Vec::new()
    }

    fn about_to_show(&self, _id: i32) -> bool {
        false
    }

    fn about_to_show_group(&self, _ids: Vec<i32>) -> (Vec<i32>, Vec<i32>) {
        (Vec::new(), Vec::new())
    }

    #[zbus(property)]
    fn version(&self) -> u32 {
        3
    }

    #[zbus(property)]
    fn text_direction(&self) -> &str {
        "ltr"
    }

    #[zbus(property)]
    fn status(&self) -> &str {
        "normal"
    }

    #[zbus(property)]
    fn icon_theme_path(&self) -> Vec<String> {
        Vec::new()
    }
}

impl Menu {
    fn clicked(&self, id: i32) {
        let action = match id {
            ID_OPEN => TrayAction::ShowWindow,
            ID_DISCONNECT => TrayAction::Disconnect,
            ID_QUIT => TrayAction::Quit,
            id if id >= TUNNEL_BASE => {
                let model = lock(&self.model);
                match model.tunnels.get((id - TUNNEL_BASE) as usize) {
                    Some(tunnel) => TrayAction::Toggle(tunnel.uuid.clone()),
                    None => return,
                }
            }
            _ => return,
        };
        send(&self.actions, action);
    }
}

/// Builds the menu: status and live traffic, tunnels, then app actions.
fn entries(model: &TrayModel) -> Vec<(i32, HashMap<String, OwnedVariant>)> {
    let mut list = vec![(ID_STATUS, info(&model.status))];
    if model.connected {
        list.push((ID_DOWN, info(&model.down)));
        list.push((ID_UP, info(&model.up)));
        if let Some(location) = &model.location {
            list.push((ID_LOCATION, info(location)));
        }
    }
    list.push((ID_SEPARATORS[0], separator()));
    for (index, tunnel) in model.tunnels.iter().enumerate() {
        let mut props = action(&tunnel.name, true);
        props.insert("toggle-type".to_owned(), owned("radio"));
        props.insert(
            "toggle-state".to_owned(),
            owned(i32::from(tunnel.connected)),
        );
        list.push((TUNNEL_BASE + index as i32, props));
    }
    list.push((ID_SEPARATORS[1], separator()));
    list.push((ID_OPEN, action(&model.open_label, true)));
    list.push((
        ID_DISCONNECT,
        action(&model.disconnect_label, model.connected),
    ));
    list.push((ID_SEPARATORS[2], separator()));
    list.push((ID_QUIT, action(&model.quit_label, true)));
    list
}

fn info(label: &str) -> HashMap<String, OwnedVariant> {
    action(label, false)
}

fn action(label: &str, enabled: bool) -> HashMap<String, OwnedVariant> {
    HashMap::from([
        ("label".to_owned(), owned(label)),
        ("enabled".to_owned(), owned(enabled)),
    ])
}

fn separator() -> HashMap<String, OwnedVariant> {
    HashMap::from([("type".to_owned(), owned("separator"))])
}

/// Every value used here (strings, booleans, integers) converts without error.
fn owned<'a>(value: impl Into<Variant<'a>>) -> OwnedVariant {
    OwnedVariant::try_from(value.into()).unwrap_or_else(|_| OwnedVariant::from(false))
}

/// A child entry wrapped in a variant, as dbusmenu expects (`av` of `(ia{sv}av)`).
fn child_variant(id: i32, props: &HashMap<String, OwnedVariant>) -> zbus::Result<OwnedVariant> {
    let props: HashMap<String, Variant<'_>> = props
        .iter()
        .map(|(key, value)| (key.clone(), Variant::from(value.clone())))
        .collect();
    let structure = StructureBuilder::new()
        .add_field(id)
        .add_field(props)
        .add_field(Vec::<Variant<'_>>::new())
        .build()?;
    OwnedVariant::try_from(Variant::from(structure))
}

fn send(actions: &UnboundedSender<TrayAction>, action: TrayAction) {
    if let Err(err) = actions.unbounded_send(action) {
        glib::g_warning!(LOG_DOMAIN, "Top bar action lost: {err}");
    }
}

/// Renders an SVG icon at the usual tray sizes.
/// A size that fails to render is skipped (and logged): the icon name still works.
fn render_pixmaps(svg: &'static str) -> Vec<Pixmap> {
    [16, 22, 32, 48]
        .into_iter()
        .filter_map(|size| {
            let bytes = glib::Bytes::from_static(svg.as_bytes());
            let stream = gio::MemoryInputStream::from_bytes(&bytes);
            match Pixbuf::from_stream_at_scale(&stream, size, size, true, gio::Cancellable::NONE) {
                Ok(pixbuf) => Some(to_argb(&pixbuf)),
                Err(err) => {
                    glib::g_debug!(LOG_DOMAIN, "Cannot render the {size}px top bar icon: {err}");
                    None
                }
            }
        })
        .collect()
}

/// GdkPixbuf stores RGBA rows (with padding); StatusNotifierItem wants packed ARGB.
fn to_argb(pixbuf: &Pixbuf) -> Pixmap {
    let (width, height) = (pixbuf.width(), pixbuf.height());
    let stride = pixbuf.rowstride().max(0) as usize;
    let channels = pixbuf.n_channels().max(0) as usize;
    let bytes = pixbuf.read_pixel_bytes();
    let mut argb = Vec::with_capacity((width.max(0) * height.max(0) * 4) as usize);
    for y in 0..height.max(0) as usize {
        for x in 0..width.max(0) as usize {
            let start = y * stride + x * channels;
            let Some(pixel) = bytes.get(start..start + channels) else {
                break;
            };
            let alpha = if channels == 4 { pixel[3] } else { 255 };
            argb.extend_from_slice(&[alpha, pixel[0], pixel[1], pixel[2]]);
        }
    }
    (width, height, argb)
}
