// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

//! Update notices. Tunnelbird never replaces its own files: it tells the user
//! that a new version exists and how it will arrive, which depends on how it
//! was installed (see `docs/ROADMAP.md`, “Updates”):
//!
//! - Flatpak: the Flatpak portal watches Flathub and installs on request;
//! - Snap: snapd updates by itself, nothing to announce;
//! - APT/DNF repository: arrives with the system updates;
//! - package downloaded by hand: the website's download page;
//! - built from source: nothing (the developer pulls the code).
//!
//! The last two learn about new versions from the website's `latest.json`, at
//! most once a day, and only when Preferences allow it. Whatever the method, a
//! version installed while the old one still runs (background mode can last
//! weeks) asks for a restart.

use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

use futures_util::{Stream, StreamExt};
use serde::Deserialize;
use zbus::zvariant::OwnedVariant;
use zbus::{Connection, MatchRule, MessageStream, OwnedObjectPath};

use crate::geo::{self, HttpError};
use crate::version;

const LATEST_URL: &str = "https://tunnelbird.ferhatsn.fr/latest.json";
/// Where a package downloaded by hand finds its newer version.
pub const DOWNLOAD_URL: &str = "https://tunnelbird.ferhatsn.fr/download";
/// Repository files written by the website's `install.sh`.
const APT_SOURCES: [&str; 2] = [
    "/etc/apt/sources.list.d/tunnelbird.sources",
    "/etc/apt/sources.list.d/tunnelbird.list",
];
const DNF_REPO: &str = "/etc/yum.repos.d/tunnelbird.repo";
/// The store listing, whose newest `<release>` gives “What's New”.
const METAINFO: &str = include_str!("../data/fr.ferhatsn.Tunnelbird.metainfo.xml");

const PORTAL_SERVICE: &str = "org.freedesktop.portal.Flatpak";
const PORTAL_PATH: &str = "/org/freedesktop/portal/Flatpak";
const MONITOR_INTERFACE: &str = "org.freedesktop.portal.Flatpak.UpdateMonitor";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Install {
    Flatpak,
    Snap,
    /// From the website's APT or DNF repository: updates come with the system.
    Repository(Manager),
    /// A `.deb` or `.rpm` downloaded by hand, or the self-contained package
    /// the website's `install.sh` unpacks (its `AppRun` sets `APPDIR`): no
    /// repository brings updates.
    Package,
    Source,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Manager {
    Apt,
    Dnf,
}

impl Install {
    /// How this copy of Tunnelbird was installed. `TUNNELBIRD_INSTALL`
    /// (`flatpak`, `snap`, `apt`, `dnf`, `package`, `source`) overrides it to
    /// test the notices from a development build.
    pub fn current() -> Self {
        static INSTALL: OnceLock<Install> = OnceLock::new();
        *INSTALL.get_or_init(|| {
            if let Some(forced) = std::env::var("TUNNELBIRD_INSTALL")
                .ok()
                .and_then(|name| Self::from_name(&name))
            {
                return forced;
            }
            if crate::portal::in_flatpak() {
                Install::Flatpak
            } else if std::env::var_os("SNAP").is_some() {
                Install::Snap
            } else if std::env::var_os("APPDIR").is_some() {
                Install::Package
            } else if cfg!(debug_assertions) {
                Install::Source
            } else {
                match std::env::current_exe() {
                    Ok(exe) => classify(&exe, |path| Path::new(path).exists()),
                    Err(_) => Install::Source,
                }
            }
        })
    }

    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "flatpak" => Install::Flatpak,
            "snap" => Install::Snap,
            "apt" => Install::Repository(Manager::Apt),
            "dnf" => Install::Repository(Manager::Dnf),
            "package" => Install::Package,
            "source" => Install::Source,
            _ => return None,
        })
    }

    /// True when new versions are learned from the website's `latest.json`
    /// (the Flatpak portal and snapd know by themselves).
    pub fn checks_website(self) -> bool {
        matches!(self, Install::Repository(_) | Install::Package)
    }
}

/// Packaged builds live in `/usr/bin`; the repository files tell whether the
/// package came from the website's repositories or was downloaded by hand.
fn classify(exe: &Path, exists: impl Fn(&str) -> bool) -> Install {
    if !exe.starts_with("/usr/bin") {
        Install::Source
    } else if APT_SOURCES.iter().any(|path| exists(path)) {
        Install::Repository(Manager::Apt)
    } else if exists(DNF_REPO) {
        Install::Repository(Manager::Dnf)
    } else {
        Install::Package
    }
}

/// The website's `latest.json`.
#[derive(Debug, Clone, Deserialize)]
pub struct Latest {
    pub version: String,
}

pub async fn fetch_latest(session: &soup::Session) -> Result<Latest, HttpError> {
    let url = std::env::var("TUNNELBIRD_UPDATE_URL").unwrap_or_else(|_| LATEST_URL.to_owned());
    geo::get_json(session, &url).await
}

/// True when a newer version was installed while this one runs, so a restart
/// finishes the update. (Flatpak learns it from the portal instead.)
pub fn installed_while_running(install: Install) -> bool {
    match install {
        Install::Snap => snap_refreshed(),
        Install::Repository(_) | Install::Package => binary_replaced(),
        Install::Flatpak | Install::Source => false,
    }
}

/// The package manager replaced the binary: the running one is now “deleted”.
fn binary_replaced() -> bool {
    let Ok(target) = std::fs::read_link("/proc/self/exe") else {
        return false;
    };
    let target = target.to_string_lossy();
    // Only when a new binary took its place, not when the package was removed.
    target
        .strip_suffix(" (deleted)")
        .is_some_and(|path| Path::new(path).exists())
}

/// snapd switched `current` to another revision than the one running.
fn snap_refreshed() -> bool {
    let (Some(snap), Ok(running)) = (std::env::var_os("SNAP"), std::env::var("SNAP_REVISION"))
    else {
        return false;
    };
    let current = Path::new(&snap)
        .parent()
        .map(|dir| dir.join("current"))
        .and_then(|link| std::fs::read_link(link).ok());
    current.is_some_and(|revision| revision.as_os_str() != running.as_str())
}

/// Version and notes (AppStream markup, as `AdwAboutDialog` expects) of the
/// newest release in the store listing.
pub fn release_notes() -> Option<(String, String)> {
    version::newest_release(METAINFO)
}

/// What the Flatpak portal reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlatpakEvent {
    /// Flathub has a newer version than the installed one.
    Available,
    /// A newer version is installed; the running one is older.
    Installed,
    /// Installing, in percent.
    Progress(u32),
    Failed(String),
}

/// Flatpak's own update watcher, through the portal (works in the sandbox).
pub struct FlatpakMonitor {
    conn: Connection,
    path: OwnedObjectPath,
}

impl FlatpakMonitor {
    pub async fn start() -> zbus::Result<(Self, impl Stream<Item = FlatpakEvent> + use<>)> {
        let conn = Connection::session().await?;
        let options: HashMap<&str, OwnedVariant> = HashMap::new();
        let reply = conn
            .call_method(
                Some(PORTAL_SERVICE),
                PORTAL_PATH,
                Some(PORTAL_SERVICE),
                "CreateUpdateMonitor",
                &(options,),
            )
            .await?;
        let path: OwnedObjectPath = reply.body().deserialize()?;
        let rule = MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender(PORTAL_SERVICE)
            .path(path.clone())
            .interface(MONITOR_INTERFACE)
            .build()?;
        let events = MessageStream::for_match_rule(rule, &conn, None)
            .await?
            .filter_map(|message| async move {
                let message = message.ok()?;
                let member = message.header().member()?.to_string();
                let info: HashMap<String, OwnedVariant> = message.body().deserialize().ok()?;
                portal_event(&member, &info)
            });
        Ok((Self { conn, path }, events))
    }

    /// Asks the portal to install the update; progress comes as events.
    pub async fn update(&self) -> zbus::Result<()> {
        let options: HashMap<&str, OwnedVariant> = HashMap::new();
        self.conn
            .call_method(
                Some(PORTAL_SERVICE),
                &self.path,
                Some(MONITOR_INTERFACE),
                "Update",
                &("", options),
            )
            .await?;
        Ok(())
    }
}

fn portal_event(member: &str, info: &HashMap<String, OwnedVariant>) -> Option<FlatpakEvent> {
    let text = |key: &str| {
        info.get(key)
            .and_then(|v| v.try_clone().ok())
            .and_then(|v| String::try_from(v).ok())
    };
    let number = |key: &str| {
        info.get(key)
            .and_then(|v| v.try_clone().ok())
            .and_then(|v| u32::try_from(v).ok())
    };
    match member {
        "UpdateAvailable" => {
            let running = text("running-commit");
            let local = text("local-commit");
            if local.is_some() && local != running {
                Some(FlatpakEvent::Installed)
            } else if text("remote-commit").is_some_and(|remote| Some(remote) != local) {
                Some(FlatpakEvent::Available)
            } else {
                None
            }
        }
        // status: 0 running, 1 nothing to do, 2 done, 3 failed.
        "Progress" => match number("status")? {
            0 => Some(FlatpakEvent::Progress(number("progress").unwrap_or(0))),
            2 => Some(FlatpakEvent::Installed),
            3 => Some(FlatpakEvent::Failed(
                text("error_message").unwrap_or_else(|| "unknown error".to_owned()),
            )),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_method_from_binary_and_repositories() {
        let none = |_: &str| false;
        assert_eq!(
            classify(
                Path::new("/home/me/tunnelbird/target/release/tunnelbird"),
                none
            ),
            Install::Source
        );
        assert_eq!(
            classify(Path::new("/usr/local/bin/tunnelbird"), none),
            Install::Source
        );
        assert_eq!(
            classify(Path::new("/usr/bin/tunnelbird"), none),
            Install::Package
        );
        assert_eq!(
            classify(Path::new("/usr/bin/tunnelbird"), |p| p == APT_SOURCES[0]),
            Install::Repository(Manager::Apt)
        );
        assert_eq!(
            classify(Path::new("/usr/bin/tunnelbird"), |p| p == DNF_REPO),
            Install::Repository(Manager::Dnf)
        );
    }

    #[test]
    fn whats_new_is_this_version() {
        let (version, _) = release_notes().expect("the metainfo has a release");
        assert_eq!(version, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn portal_signals_become_events() {
        let info = |pairs: &[(&str, &str)]| -> HashMap<String, OwnedVariant> {
            pairs
                .iter()
                .map(|(k, v)| ((*k).to_owned(), OwnedVariant::from(zbus::Str::from(*v))))
                .collect()
        };
        let same = info(&[
            ("running-commit", "a"),
            ("local-commit", "a"),
            ("remote-commit", "a"),
        ]);
        assert_eq!(portal_event("UpdateAvailable", &same), None);
        let remote = info(&[
            ("running-commit", "a"),
            ("local-commit", "a"),
            ("remote-commit", "b"),
        ]);
        assert_eq!(
            portal_event("UpdateAvailable", &remote),
            Some(FlatpakEvent::Available)
        );
        let installed = info(&[
            ("running-commit", "a"),
            ("local-commit", "b"),
            ("remote-commit", "b"),
        ]);
        assert_eq!(
            portal_event("UpdateAvailable", &installed),
            Some(FlatpakEvent::Installed)
        );
    }
}
