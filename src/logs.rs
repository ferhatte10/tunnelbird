// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

//! The logs of a tunnel, in a dialog: what this app saw happen (always there,
//! kept for the session) merged with what NetworkManager and its OpenVPN plugin
//! wrote to the system journal (`journalctl --follow`, stopped when the dialog
//! closes). A Flatpak or Snap sandbox, or an account without the right, cannot
//! read the journal: the dialog says so and gives the command to use instead.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::ffi::OsStr;
use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};
use serde::Deserialize;

use crate::LOG_DOMAIN;
use crate::nm::Tunnel;
use crate::profile::Kind;
use crate::share;
use crate::window;

/// Journal lines read back when the dialog opens (about a week of them).
const JOURNAL_LINES: &str = "--lines=5000";
/// Most lines kept from the journal, and from this app, per tunnel.
const MAX_JOURNAL: usize = 3000;
const MAX_EVENTS: usize = 200;
/// How often the dialog shows what arrived meanwhile.
const RENDER_TICK: Duration = Duration::from_millis(300);
/// Ticks to wait for the journal before saying that there is nothing.
const EMPTY_AFTER_TICKS: u32 = 3;
/// Distance from the end, in pixels, under which new lines keep it in view.
const STICK_TO_END: f64 = 24.0;
const SANDBOX_COMMAND: &str = "journalctl -u NetworkManager --since today";
const GROUP_COMMAND: &str = "sudo usermod -aG systemd-journal $USER";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

/// Who wrote a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    App,
    NetworkManager,
    OpenVpn,
}

impl Source {
    fn label(self) -> &'static str {
        match self {
            Source::App => "Tunnelbird",
            Source::NetworkManager => "NetworkManager",
            Source::OpenVpn => "OpenVPN",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Microseconds since the Unix epoch.
    pub time: i64,
    pub source: Source,
    pub severity: Severity,
    pub text: String,
}

/// What this app saw of each tunnel since it started.
#[derive(Default)]
pub struct Events {
    by_tunnel: HashMap<String, VecDeque<Entry>>,
    revision: u64,
}

impl Events {
    pub fn push(&mut self, uuid: &str, severity: Severity, text: String) {
        self.push_at(uuid, glib::real_time(), Source::App, severity, text);
    }

    pub fn push_at(
        &mut self,
        uuid: &str,
        time: i64,
        source: Source,
        severity: Severity,
        text: String,
    ) {
        let entries = self.by_tunnel.entry(uuid.to_owned()).or_default();
        if entries.len() == MAX_EVENTS {
            entries.pop_front();
        }
        entries.push_back(Entry {
            time,
            source,
            severity,
            text,
        });
        self.revision += 1;
    }
}

/// Why NetworkManager ended a connection, when it says more than "as asked".
pub fn reason_text(reason: u32) -> Option<String> {
    // NMActiveConnectionStateReason.
    Some(match reason {
        3 => gettext("the network connection was lost"),
        4 => gettext("the VPN service stopped"),
        5 => gettext("the server sent an invalid network configuration"),
        6 => gettext("the server did not answer in time"),
        7 | 8 => gettext("the VPN service could not start"),
        9 => gettext("a password or key is missing"),
        10 => gettext("the login was refused"),
        12..=14 => gettext("the network interface could not be set up"),
        _ => return None,
    })
}

/// Keeps the journal lines of one tunnel. The lines of the OpenVPN process
/// name no tunnel: they belong to the one that started last.
struct JournalFilter {
    uuid: String,
    name: String,
    /// WireGuard only: an OpenVPN `tun0` serves any tunnel over time.
    interface: Option<String>,
    openvpn: bool,
    started: bool,
}

impl JournalFilter {
    fn new(tunnel: &Tunnel) -> Self {
        let openvpn = tunnel.kind == Kind::OpenVpn;
        Self {
            uuid: tunnel.uuid.clone(),
            name: tunnel.name.clone(),
            interface: (!openvpn && !tunnel.interface.is_empty())
                .then(|| format!("({})", tunnel.interface)),
            openvpn,
            started: false,
        }
    }

    fn keeps(&mut self, identifier: &str, message: &str) -> bool {
        let mine = message.contains(&self.uuid);
        if message.contains("vpn[") && message.contains("]: starting openvpn") {
            self.started = mine;
        }
        mine || message.contains(&format!("'{}'", self.name))
            || message.contains(&format!("\"{}\"", self.name))
            || self.interface.as_ref().is_some_and(|i| message.contains(i))
            || (self.openvpn
                && self.started
                && (identifier == "nm-openvpn" || names_tun_device(message)))
    }
}

/// `(tun0)`, not the IPv6 guard's `(tunnelbird-v6)`.
fn names_tun_device(message: &str) -> bool {
    message
        .split("(tun")
        .skip(1)
        .any(|rest| rest.starts_with(|c: char| c.is_ascii_digit()))
}

#[derive(Deserialize)]
struct Record {
    #[serde(rename = "__REALTIME_TIMESTAMP")]
    time: String,
    #[serde(rename = "MESSAGE")]
    message: Option<serde_json::Value>,
    #[serde(rename = "PRIORITY")]
    priority: Option<String>,
    #[serde(rename = "SYSLOG_IDENTIFIER", default)]
    identifier: String,
}

/// One line of `journalctl --output=json`, when the filter keeps it.
fn parse_line(line: &str, filter: &mut JournalFilter) -> Option<Entry> {
    let record: Record = serde_json::from_str(line).ok()?;
    let message = match record.message? {
        serde_json::Value::String(text) => text,
        // The journal gives text that is not valid UTF-8 as an array of bytes.
        bytes @ serde_json::Value::Array(_) => {
            let bytes: Vec<u8> = serde_json::from_value(bytes).ok()?;
            String::from_utf8_lossy(&bytes).into_owned()
        }
        _ => return None,
    };
    if !filter.keeps(&record.identifier, &message) {
        return None;
    }
    let (level, text) = split_level(&message);
    let severity = match level {
        Some("error") => Severity::Error,
        Some("warn") => Severity::Warning,
        Some(_) => Severity::Info,
        None => guess_severity(record.priority.as_deref(), text),
    };
    let source = if record.identifier == "nm-openvpn" {
        Source::OpenVpn
    } else {
        Source::NetworkManager
    };
    Some(Entry {
        time: record.time.parse().ok()?,
        source,
        severity,
        text: text.to_owned(),
    })
}

/// Splits `<info>  [1793313623.4992] text`: the time is shown apart.
fn split_level(message: &str) -> (Option<&str>, &str) {
    let split = message
        .strip_prefix('<')
        .and_then(|rest| rest.split_once('>'))
        .and_then(|(level, rest)| {
            let rest = rest.trim_start().strip_prefix('[')?.split_once("] ")?.1;
            Some((level, rest))
        });
    match split {
        Some((level, text)) => (Some(level), text),
        None => (None, message),
    }
}

/// OpenVPN writes its errors, too, at the normal priority.
fn guess_severity(priority: Option<&str>, text: &str) -> Severity {
    let priority = priority.and_then(|p| p.parse::<u8>().ok()).unwrap_or(6);
    let lower = text.to_lowercase();
    let note = ["note:", "deprecated", "warning:"]
        .iter()
        .any(|start| lower.starts_with(start));
    if priority <= 3
        || (!note
            && ["error", "fatal", "failed"]
                .iter()
                .any(|w| lower.contains(w)))
    {
        Severity::Error
    } else if priority == 4 || lower.starts_with("warning:") {
        Severity::Warning
    } else {
        Severity::Info
    }
}

/// Whether the journal can be read here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Access {
    Yes,
    /// A Flatpak or Snap sandbox has no journal to read.
    Sandboxed,
    /// The account is not allowed to.
    Denied,
}

fn access_from_stderr(line: &str) -> Option<Access> {
    let refused = [
        "not seeing messages",
        "insufficient permissions",
        "No journal files",
    ];
    refused
        .iter()
        .any(|hint| line.contains(hint))
        .then_some(Access::Denied)
}

/// What the dialog shows, filled from the journal in the background.
struct Journal {
    entries: RefCell<VecDeque<Entry>>,
    dirty: Cell<bool>,
    access: Cell<Access>,
}

/// Reads `journalctl --follow` until it stops (the dialog kills it when closing).
fn follow_journal(tunnel: &Tunnel, journal: &Rc<Journal>) -> Option<gio::Subprocess> {
    if crate::portal::in_flatpak() || std::env::var_os("SNAP").is_some() {
        journal.access.set(Access::Sandboxed);
        return None;
    }
    let process = gio::Subprocess::newv(
        &[
            OsStr::new("journalctl"),
            OsStr::new("--follow"),
            OsStr::new("--no-pager"),
            OsStr::new("--output=json"),
            OsStr::new(JOURNAL_LINES),
            OsStr::new("_SYSTEMD_UNIT=NetworkManager.service"),
        ],
        gio::SubprocessFlags::STDOUT_PIPE | gio::SubprocessFlags::STDERR_PIPE,
    );
    let process = match process {
        Ok(process) => process,
        Err(err) => {
            glib::g_warning!(LOG_DOMAIN, "Cannot start journalctl: {err}");
            journal.access.set(Access::Sandboxed);
            return None;
        }
    };
    if let Some(stdout) = process.stdout_pipe() {
        let stream = gio::DataInputStream::new(&stdout);
        let journal = journal.clone();
        let mut filter = JournalFilter::new(tunnel);
        glib::spawn_future_local(async move {
            while let Ok(Some(line)) = stream.read_line_utf8_future(glib::Priority::LOW).await {
                let Some(entry) = parse_line(&line, &mut filter) else {
                    continue;
                };
                let mut entries = journal.entries.borrow_mut();
                if entries.len() == MAX_JOURNAL {
                    entries.pop_front();
                }
                entries.push_back(entry);
                journal.dirty.set(true);
            }
        });
    }
    if let Some(stderr) = process.stderr_pipe() {
        let stream = gio::DataInputStream::new(&stderr);
        let journal = journal.clone();
        glib::spawn_future_local(async move {
            while let Ok(Some(line)) = stream.read_line_utf8_future(glib::Priority::LOW).await {
                if let Some(access) = access_from_stderr(&line) {
                    journal.access.set(access);
                    journal.dirty.set(true);
                }
            }
        });
    }
    Some(process)
}

/// The date and time of a line, in the user's format.
fn format_time(time: i64) -> String {
    glib::DateTime::from_unix_local(time / 1_000_000)
        .and_then(|time| time.format("%x %X"))
        .map(String::from)
        .unwrap_or_default()
}

fn severity_label(severity: Severity) -> Option<String> {
    match severity {
        Severity::Info => None,
        Severity::Warning => Some(gettext("Warning")),
        Severity::Error => Some(gettext("Error")),
    }
}

/// One line as text, for Copy and Save.
fn plain_line(entry: &Entry) -> String {
    let mut line = format!("{}  {}  ", format_time(entry.time), entry.source.label());
    if let Some(label) = severity_label(entry.severity) {
        line.push_str(&label);
        line.push_str("  ");
    }
    line.push_str(&entry.text);
    line
}

/// The row of a line: date, source and severity above the message.
fn entry_row(entry: &Entry) -> gtk::Box {
    let header = gtk::Box::builder().spacing(8).build();
    header.append(
        &gtk::Label::builder()
            .label(format_time(entry.time))
            .css_classes(["caption", "log-time"])
            .build(),
    );
    header.append(
        &gtk::Label::builder()
            .label(entry.source.label())
            .css_classes(["caption-heading", "accent"])
            .hexpand(true)
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build(),
    );
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .css_classes(["log-entry"])
        .build();
    if let Some(label) = severity_label(entry.severity) {
        let (color, tint) = match entry.severity {
            Severity::Warning => ("warning", "log-warning"),
            _ => ("error", "log-error"),
        };
        header.append(
            &gtk::Label::builder()
                .label(label)
                .css_classes(["caption-heading", color])
                .build(),
        );
        row.add_css_class(tint);
    }
    row.append(&header);
    row.append(
        &gtk::Label::builder()
            .label(entry.text.as_str())
            .use_markup(false)
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .xalign(0.0)
            .selectable(true)
            .css_classes(["monospace", "log-message"])
            .build(),
    );
    row
}

/// The list of lines; only those on screen exist as widgets.
struct LogList {
    store: gio::ListStore,
    /// What the "Problems" button leaves in the list.
    shown: gtk::FilterListModel,
    list: gtk::ListView,
    scrolled: gtk::ScrolledWindow,
    /// How many lines are in the store, and the time of the last one.
    stored: Cell<(usize, i64)>,
}

impl LogList {
    fn new(problems_only: &gtk::ToggleButton) -> Self {
        let store = gio::ListStore::new::<glib::BoxedAnyObject>();
        let button = problems_only.clone();
        let filter = gtk::CustomFilter::new(move |item| {
            !button.is_active()
                || item
                    .downcast_ref::<glib::BoxedAnyObject>()
                    .is_some_and(|item| item.borrow::<Entry>().severity != Severity::Info)
        });
        problems_only.connect_toggled({
            let filter = filter.clone();
            move |_| filter.changed(gtk::FilterChange::Different)
        });
        let shown = gtk::FilterListModel::new(Some(store.clone()), Some(filter));
        let factory = gtk::SignalListItemFactory::new();
        factory.connect_bind(|_, item| {
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            let row = item
                .item()
                .and_downcast::<glib::BoxedAnyObject>()
                .map(|entry| entry_row(&entry.borrow::<Entry>()));
            item.set_child(row.as_ref());
        });
        factory.connect_unbind(|_, item| {
            if let Some(item) = item.downcast_ref::<gtk::ListItem>() {
                item.set_child(gtk::Widget::NONE);
            }
        });
        let list = gtk::ListView::builder()
            .model(&gtk::NoSelection::new(Some(shown.clone())))
            .factory(&factory)
            .show_separators(true)
            .css_classes(["log-list"])
            .build();
        list.update_property(&[gtk::accessible::Property::Label(&gettext("Logs"))]);
        let scrolled = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&list)
            .build();
        Self {
            store,
            shown,
            list,
            scrolled,
            stored: Cell::new((0, 0)),
        }
    }

    /// Adds what is new; starts over only when a line belongs further up.
    fn update(&self, entries: &[Entry]) {
        let (stored, last) = self.stored.get();
        let in_order = entries.len() >= stored
            && (stored == 0 || entries.get(stored - 1).is_some_and(|e| e.time == last));
        let from = if in_order { stored } else { 0 };
        let adjustment = self.scrolled.vadjustment();
        let at_end =
            adjustment.value() + adjustment.page_size() >= adjustment.upper() - STICK_TO_END;
        let added: Vec<glib::BoxedAnyObject> = entries[from..]
            .iter()
            .cloned()
            .map(glib::BoxedAnyObject::new)
            .collect();
        self.store
            .splice(from as u32, self.store.n_items() - from as u32, &added);
        if let Some(entry) = entries.last() {
            self.stored.set((entries.len(), entry.time));
        }
        if at_end {
            self.scroll_to_end();
        }
    }

    fn scroll_to_end(&self) {
        if let Some(last) = self.shown.n_items().checked_sub(1) {
            self.list.scroll_to(last, gtk::ListScrollFlags::NONE, None);
        }
    }

    /// What the list shows, as text.
    fn text(&self) -> String {
        model_text(&self.shown)
    }
}

/// A filter model announces plain objects: its typed iterator would panic.
fn model_text(model: &impl IsA<gio::ListModel>) -> String {
    let mut text = String::new();
    for item in model.iter::<glib::Object>().flatten() {
        if let Some(item) = item.downcast_ref::<glib::BoxedAnyObject>() {
            text.push_str(&plain_line(&item.borrow::<Entry>()));
            text.push('\n');
        }
    }
    text
}

fn merged(uuid: &str, events: &Events, journal: &Journal) -> Vec<Entry> {
    let mut entries: Vec<Entry> = journal.entries.borrow().iter().cloned().collect();
    if let Some(own) = events.by_tunnel.get(uuid) {
        entries.extend(own.iter().cloned());
    }
    entries.sort_by_key(|entry| entry.time);
    entries
}

fn notice(text: &str, command: &'static str) -> gtk::Box {
    let notice = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .build();
    notice.append(
        &gtk::Label::builder()
            .label(text)
            .wrap(true)
            .xalign(0.0)
            .build(),
    );
    notice.append(&window::command_box(command));
    notice
}

/// Asks where to save the logs. 0600: they name servers and addresses.
async fn save(
    parent: &impl IsA<gtk::Widget>,
    name: &str,
    text: &str,
) -> Result<Option<String>, String> {
    let dialog = gtk::FileDialog::builder()
        .title(gettext("Save Logs"))
        .initial_name(format!("tunnelbird-{}.log", share::file_stem(name)))
        .initial_folder(&share::start_folder())
        .modal(true)
        .build();
    let window = parent.as_ref().root().and_downcast::<gtk::Window>();
    let file = match dialog.save_future(window.as_ref()).await {
        Ok(file) => file,
        Err(err) if err.matches(gtk::DialogError::Dismissed) => return Ok(None),
        Err(err) => return Err(err.to_string()),
    };
    let Some(path) = file.path() else {
        return Err(gettext("This location cannot be written to"));
    };
    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&path)
        .and_then(|mut out| out.write_all(text.as_bytes()))
        .map_err(|err| format!("{}: {err}", path.display()))?;
    Ok(Some(path.display().to_string()))
}

/// `read_journal` is false in demo mode, which never shows real data.
pub fn present(
    parent: &impl IsA<gtk::Widget>,
    tunnel: &Tunnel,
    events: &Rc<RefCell<Events>>,
    read_journal: bool,
) {
    let problems_only = gtk::ToggleButton::builder()
        .icon_name("dialog-warning-symbolic")
        .tooltip_text(gettext("Problems Only"))
        .build();
    window::clickable(&problems_only);
    let log = Rc::new(LogList::new(&problems_only));
    let empty = adw::StatusPage::builder()
        .icon_name("document-open-recent-symbolic")
        .build();
    let loading = gtk::Spinner::builder()
        .spinning(true)
        .width_request(32)
        .height_request(32)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .build();
    let stack = gtk::Stack::builder().vexpand(true).build();
    stack.add_named(&loading, Some("loading"));
    stack.add_named(&empty, Some("empty"));
    stack.add_named(&log.scrolled, Some("log"));

    let toasts = adw::ToastOverlay::new();
    let copy = gtk::Button::builder()
        .icon_name("edit-copy-symbolic")
        .tooltip_text(gettext("Copy Logs"))
        .build();
    window::clickable(&copy);
    let copied = log.clone();
    copy.connect_clicked(move |button| {
        button.clipboard().set_text(copied.text().trim_end());
        button.set_icon_name("object-select-symbolic");
    });
    let save_button = gtk::Button::builder()
        .icon_name("document-save-symbolic")
        .tooltip_text(gettext("Save Logs"))
        .build();
    window::clickable(&save_button);
    let saved = log.clone();
    let name = tunnel.name.clone();
    let overlay = toasts.clone();
    save_button.connect_clicked(move |button| {
        let (button, saved, name, overlay) =
            (button.clone(), saved.clone(), name.clone(), overlay.clone());
        glib::spawn_future_local(async move {
            let message = match save(&button, &name, &saved.text()).await {
                Ok(Some(path)) => gettext("Saved to {path}").replace("{path}", &path),
                Ok(None) => return,
                Err(err) => err,
            };
            overlay.add_toast(
                adw::Toast::builder()
                    .title(message)
                    .use_markup(false)
                    .timeout(5)
                    .build(),
            );
        });
    });
    let header = adw::HeaderBar::builder()
        .title_widget(&adw::WindowTitle::new(&gettext("Logs"), &tunnel.name))
        .build();
    header.pack_start(&problems_only);
    header.pack_end(&save_button);
    header.pack_end(&copy);

    let notices = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.append(&notices);
    content.append(&stack);
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&content));
    toasts.set_child(Some(&toolbar));
    let dialog = adw::Dialog::builder()
        .content_width(720)
        .content_height(560)
        .child(&toasts)
        .build();

    let journal = Rc::new(Journal {
        entries: RefCell::new(VecDeque::new()),
        dirty: Cell::new(true),
        access: Cell::new(Access::Yes),
    });
    let process = read_journal
        .then(|| follow_journal(tunnel, &journal))
        .flatten();

    let closed = Rc::new(Cell::new(false));
    {
        let closed = closed.clone();
        dialog.connect_closed(move |_| {
            closed.set(true);
            if let Some(process) = &process {
                process.force_exit();
            }
        });
    }

    let uuid = tunnel.uuid.clone();
    let events = events.clone();
    let mut seen_revision = None;
    let mut ticks = 0;
    let mut told = Access::Yes;
    let mut tick = move || {
        if closed.get() {
            return glib::ControlFlow::Break;
        }
        let access = journal.access.get();
        if access != told {
            told = access;
            notices.append(&match access {
                Access::Sandboxed => notice(
                    &gettext(
                        "Here, Tunnelbird can only show what it saw itself: the app's sandbox has no access to the system journal. For the full log, run this in a terminal:",
                    ),
                    SANDBOX_COMMAND,
                ),
                _ => notice(
                    &gettext(
                        "Here, Tunnelbird can only show what it saw itself: your account may not read the system journal. To allow it, run this in a terminal, then log in again:",
                    ),
                    GROUP_COMMAND,
                ),
            });
        }
        let revision = events.borrow().revision;
        ticks += 1;
        if journal.dirty.replace(false) || seen_revision != Some(revision) {
            seen_revision = Some(revision);
            log.update(&merged(&uuid, &events.borrow(), &journal));
        }
        let name = if log.shown.n_items() > 0 {
            "log"
        } else if problems_only.is_active() {
            empty.set_title(&gettext("No Problems"));
            empty.set_description(Some(&gettext("Nothing went wrong on this tunnel.")));
            "empty"
        } else if ticks >= EMPTY_AFTER_TICKS || !read_journal {
            empty.set_title(&gettext("No Events Yet"));
            empty.set_description(Some(&gettext(
                "Events appear here as soon as this tunnel connects.",
            )));
            "empty"
        } else {
            "loading"
        };
        stack.set_visible_child_name(name);
        glib::ControlFlow::Continue
    };
    tick();
    glib::timeout_add_local(RENDER_TICK, tick);
    dialog.present(Some(parent));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filter(openvpn: bool) -> JournalFilter {
        JournalFilter {
            uuid: "3d676f74-c7a4-4fa0-8c32-8e205a98d18a".to_owned(),
            name: "password-only".to_owned(),
            interface: (!openvpn).then(|| "(wg-home)".to_owned()),
            openvpn,
            started: false,
        }
    }

    #[test]
    fn openvpn_lines_follow_the_tunnel_that_started_last() {
        let mut mine = filter(true);
        assert!(!mine.keeps("nm-openvpn", "TUN/TAP device tun0 opened"));
        assert!(mine.keeps(
            "NetworkManager",
            "<info>  [1.0] vpn[0x6,3d676f74-c7a4-4fa0-8c32-8e205a98d18a,\"password-only\"]: starting openvpn"
        ));
        assert!(mine.keeps("nm-openvpn", "TUN/TAP device tun0 opened"));
        assert!(mine.keeps(
            "NetworkManager",
            "<info>  [1.0] device (tun0): state change"
        ));
        assert!(!mine.keeps(
            "NetworkManager",
            "<info>  [1.0] device (tunnelbird-v6): state change"
        ));
        assert!(!mine.keeps(
            "NetworkManager",
            "<info>  [2.0] vpn[0x7,24618de3-ff94-4477-9ad2-dbdc76ac8f68,\"other\"]: starting openvpn"
        ));
        assert!(!mine.keeps("nm-openvpn", "TUN/TAP device tun0 opened"));
    }

    #[test]
    fn other_connections_are_left_out() {
        let mut mine = filter(false);
        assert!(mine.keeps("NetworkManager", "device (wg-home): state change"));
        assert!(mine.keeps(
            "NetworkManager",
            "policy: set 'password-only' (wg-home) as default"
        ));
        assert!(!mine.keeps(
            "NetworkManager",
            "dhcp4 (wlp0s20f3): state changed new lease"
        ));
        assert!(!mine.keeps("nm-openvpn", "TUN/TAP device tun0 opened"));
    }

    #[test]
    fn parses_a_journal_record() {
        let line = r#"{"__REALTIME_TIMESTAMP":"1793313623105000","MESSAGE":"<warn>  [1793313623.1055] device (wg-home): failed","PRIORITY":"4","SYSLOG_IDENTIFIER":"NetworkManager"}"#;
        assert_eq!(
            parse_line(line, &mut filter(false)),
            Some(Entry {
                time: 1_793_313_623_105_000,
                source: Source::NetworkManager,
                severity: Severity::Warning,
                text: "device (wg-home): failed".to_owned(),
            })
        );
    }

    #[test]
    fn reads_a_message_given_as_bytes() {
        let line = r#"{"__REALTIME_TIMESTAMP":"1","MESSAGE":[40,119,103,45,104,111,109,101,41],"SYSLOG_IDENTIFIER":"NetworkManager"}"#;
        let entry = parse_line(line, &mut filter(false)).expect("kept");
        assert_eq!(entry.text, "(wg-home)");
    }

    #[test]
    fn broken_lines_are_skipped() {
        assert_eq!(parse_line("not json", &mut filter(false)), None);
    }

    #[test]
    fn openvpn_errors_stand_out_but_not_its_notes() {
        assert_eq!(
            guess_severity(Some("5"), "AUTH: Received control message: AUTH_FAILED"),
            Severity::Error
        );
        assert_eq!(
            guess_severity(Some("5"), "TLS Error: TLS handshake failed"),
            Severity::Error
        );
        assert_eq!(
            guess_severity(Some("5"), "Note: cipher negotiation failed in this case"),
            Severity::Info
        );
        assert_eq!(
            guess_severity(Some("5"), "Initialization Sequence Completed"),
            Severity::Info
        );
    }

    #[test]
    fn a_refused_journal_is_noticed() {
        assert_eq!(
            access_from_stderr(
                "Hint: You are currently not seeing messages from other users and the system."
            ),
            Some(Access::Denied)
        );
        assert_eq!(access_from_stderr("-- No entries --"), None);
    }

    #[test]
    fn copies_from_a_model_of_plain_objects() {
        let model = gio::ListStore::new::<glib::Object>();
        model.append(&glib::BoxedAnyObject::new(Entry {
            time: 0,
            source: Source::OpenVpn,
            severity: Severity::Info,
            text: "Initialization Sequence Completed".to_owned(),
        }));
        let text = model_text(&model);
        assert!(text.contains("OpenVPN  Initialization Sequence Completed\n"));
    }

    #[test]
    fn events_are_bounded() {
        let mut events = Events::default();
        for number in 0..=MAX_EVENTS {
            events.push_at(
                "id",
                number as i64,
                Source::App,
                Severity::Info,
                String::new(),
            );
        }
        let kept = &events.by_tunnel["id"];
        assert_eq!(kept.len(), MAX_EVENTS);
        assert_eq!(kept.front().map(|entry| entry.time), Some(1));
    }
}
