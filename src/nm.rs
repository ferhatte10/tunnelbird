// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

//! NetworkManager client over the system D-Bus.
//!
//! Tunnelbird never creates the network interface itself: it asks NetworkManager
//! to store, activate or deactivate a WireGuard profile. Permissions are
//! therefore handled by polkit and the app runs without any privilege.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use futures_util::{Stream, StreamExt};
use gtk::glib;
use zbus::{
    Connection, MatchRule, MessageStream, ObjectPath, OwnedObjectPath, OwnedVariant, Variant, proxy,
};

use crate::config::{Interface, IpNet, Peer, WgConfig};
use crate::trust::{self, Network, NetworkKind};

const NM_SERVICE: &str = "org.freedesktop.NetworkManager";

type Settings = HashMap<String, HashMap<String, OwnedVariant>>;

#[proxy(
    interface = "org.freedesktop.NetworkManager",
    default_service = "org.freedesktop.NetworkManager",
    default_path = "/org/freedesktop/NetworkManager"
)]
trait NetworkManager {
    fn activate_connection(
        &self,
        connection: &ObjectPath<'_>,
        device: &ObjectPath<'_>,
        specific_object: &ObjectPath<'_>,
    ) -> zbus::Result<OwnedObjectPath>;

    fn deactivate_connection(&self, active_connection: &ObjectPath<'_>) -> zbus::Result<()>;

    #[zbus(property)]
    fn active_connections(&self) -> zbus::Result<Vec<OwnedObjectPath>>;
}

#[proxy(
    interface = "org.freedesktop.NetworkManager.Settings",
    default_service = "org.freedesktop.NetworkManager",
    default_path = "/org/freedesktop/NetworkManager/Settings"
)]
trait NmSettings {
    fn list_connections(&self) -> zbus::Result<Vec<OwnedObjectPath>>;

    fn add_connection(
        &self,
        connection: HashMap<&str, HashMap<&str, Variant<'_>>>,
    ) -> zbus::Result<OwnedObjectPath>;
}

#[proxy(
    interface = "org.freedesktop.NetworkManager.Settings.Connection",
    default_service = "org.freedesktop.NetworkManager"
)]
trait NmConnection {
    fn get_settings(&self) -> zbus::Result<Settings>;

    fn get_secrets(&self, setting_name: &str) -> zbus::Result<Settings>;

    fn update(&self, properties: Settings) -> zbus::Result<()>;

    fn delete(&self) -> zbus::Result<()>;
}

#[proxy(
    interface = "org.freedesktop.NetworkManager.Connection.Active",
    default_service = "org.freedesktop.NetworkManager"
)]
trait NmActive {
    #[zbus(property)]
    fn uuid(&self) -> zbus::Result<String>;

    #[zbus(property, name = "Type")]
    fn kind(&self) -> zbus::Result<String>;

    #[zbus(property)]
    fn state(&self) -> zbus::Result<u32>;

    /// The settings profile this activation comes from.
    #[zbus(property)]
    fn connection(&self) -> zbus::Result<OwnedObjectPath>;

    /// True when this connection holds the default IPv4 route.
    #[zbus(property, name = "Default")]
    fn is_default(&self) -> zbus::Result<bool>;

    #[zbus(property)]
    fn devices(&self) -> zbus::Result<Vec<OwnedObjectPath>>;
}

#[proxy(
    interface = "org.freedesktop.NetworkManager.Device.Wireless",
    default_service = "org.freedesktop.NetworkManager"
)]
trait NmWireless {
    #[zbus(property)]
    fn active_access_point(&self) -> zbus::Result<OwnedObjectPath>;
}

#[proxy(
    interface = "org.freedesktop.NetworkManager.AccessPoint",
    default_service = "org.freedesktop.NetworkManager"
)]
trait NmAccessPoint {
    #[zbus(property)]
    fn hw_address(&self) -> zbus::Result<String>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TunnelState {
    Disconnected,
    Connecting,
    Connected,
    Disconnecting,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Tunnel {
    pub path: OwnedObjectPath,
    pub uuid: String,
    pub name: String,
    pub interface: String,
    pub endpoint: Option<String>,
    /// Addresses of this computer inside the VPN (`10.8.0.3/32`…).
    pub addresses: Vec<String>,
    /// True when all internet traffic goes through the tunnel (`AllowedIPs` has a `/0`).
    pub full_tunnel: bool,
    /// Well-known VPN provider recognised from the settings (see `provider.rs`).
    pub provider: Option<&'static str>,
    pub state: TunnelState,
    /// Matching active connection, needed to deactivate the tunnel.
    pub active_path: Option<OwnedObjectPath>,
}

#[derive(Clone)]
pub struct Nm {
    conn: Connection,
}

impl Nm {
    pub async fn connect() -> zbus::Result<Self> {
        Ok(Self {
            conn: Connection::system().await?,
        })
    }

    /// WireGuard profiles known to NetworkManager with their current state, sorted by name.
    pub async fn tunnels(&self) -> zbus::Result<Vec<Tunnel>> {
        let active = self.active_wireguard().await?;
        let settings = NmSettingsProxy::new(&self.conn).await?;
        let mut tunnels = Vec::new();

        for path in settings.list_connections().await? {
            let proxy = NmConnectionProxy::builder(&self.conn)
                .path(path.clone())
                .build()
                .await?;
            // A profile can vanish between listing and reading: skip it.
            let settings = match proxy.get_settings().await {
                Ok(settings) => settings,
                Err(err) => {
                    glib::g_debug!(
                        crate::LOG_DOMAIN,
                        "Cannot read profile {}: {err}",
                        path.as_str()
                    );
                    continue;
                }
            };
            if get_str(&settings, "connection", "type").as_deref() != Some("wireguard") {
                continue;
            }
            let uuid = get_str(&settings, "connection", "uuid").unwrap_or_default();
            let (state, active_path) = match active.get(&uuid) {
                Some((path, state)) => (*state, Some(path.clone())),
                None => (TunnelState::Disconnected, None),
            };
            let name = get_str(&settings, "connection", "id").unwrap_or_default();
            let endpoint = first_endpoint(&settings);
            let provider = crate::provider::detect(&crate::provider::Hints {
                name: &name,
                endpoint: endpoint.as_deref(),
                dns: &dns_servers(&settings),
            });
            tunnels.push(Tunnel {
                path,
                name,
                interface: get_str(&settings, "connection", "interface-name").unwrap_or_default(),
                endpoint,
                provider,
                addresses: addresses(&settings),
                full_tunnel: is_full_tunnel(&settings),
                uuid,
                state,
                active_path,
            });
        }
        tunnels.sort_by_key(|t| t.name.to_lowercase());
        Ok(tunnels)
    }

    async fn active_wireguard(
        &self,
    ) -> zbus::Result<HashMap<String, (OwnedObjectPath, TunnelState)>> {
        let nm = NetworkManagerProxy::new(&self.conn).await?;
        let mut active = HashMap::new();
        for path in nm.active_connections().await? {
            let proxy = NmActiveProxy::builder(&self.conn)
                .path(path.clone())
                .build()
                .await?;
            if proxy.kind().await.ok().as_deref() != Some("wireguard") {
                continue;
            }
            let state = match proxy.state().await.unwrap_or(0) {
                1 => TunnelState::Connecting,
                2 => TunnelState::Connected,
                3 => TunnelState::Disconnecting,
                _ => TunnelState::Disconnected,
            };
            if let Ok(uuid) = proxy.uuid().await {
                active.insert(uuid, (path, state));
            }
        }
        Ok(active)
    }

    pub async fn activate(&self, tunnel: &Tunnel) -> zbus::Result<()> {
        let nm = NetworkManagerProxy::new(&self.conn).await?;
        let root = ObjectPath::from_static_str_unchecked("/");
        nm.activate_connection(&tunnel.path, &root, &root).await?;
        Ok(())
    }

    pub async fn deactivate(&self, tunnel: &Tunnel) -> zbus::Result<()> {
        let Some(active) = &tunnel.active_path else {
            return Ok(());
        };
        NetworkManagerProxy::new(&self.conn)
            .await?
            .deactivate_connection(active)
            .await
    }

    /// Names of the Wi-Fi networks saved on this computer, sorted, without duplicates.
    pub async fn saved_wifi(&self) -> zbus::Result<Vec<String>> {
        let settings = NmSettingsProxy::new(&self.conn).await?;
        let mut names = Vec::new();
        for path in settings.list_connections().await? {
            let proxy = NmConnectionProxy::builder(&self.conn)
                .path(path)
                .build()
                .await?;
            let Ok(profile) = proxy.get_settings().await else {
                continue;
            };
            if get_str(&profile, "connection", "type").as_deref() != Some("802-11-wireless") {
                continue;
            }
            if let Some(ssid) = wifi_ssid(&profile).filter(|s| !s.is_empty()) {
                names.push(ssid);
            }
        }
        names.sort_by_key(|name| name.to_lowercase());
        names.dedup();
        Ok(names)
    }

    /// The network this computer reaches the internet through (Wi-Fi, cable,
    /// mobile), ignoring tunnels: with a full tunnel up, the tunnel holds the
    /// default route, so the physical connection is looked up among active ones.
    pub async fn current_network(&self) -> zbus::Result<Option<Network>> {
        let nm = NetworkManagerProxy::new(&self.conn).await?;
        let mut best: Option<(bool, Network)> = None;
        for path in nm.active_connections().await? {
            let active = NmActiveProxy::builder(&self.conn)
                .path(path)
                .build()
                .await?;
            let (Ok(nm_type), Ok(2)) = (active.kind().await, active.state().await) else {
                continue;
            };
            let Some(kind) = trust::kind_of(&nm_type) else {
                continue;
            };
            let (ssid, bssid) = if kind == NetworkKind::Wifi {
                let profile = NmConnectionProxy::builder(&self.conn)
                    .path(active.connection().await?)
                    .build()
                    .await?;
                (
                    wifi_ssid(&profile.get_settings().await?),
                    self.access_point_bssid(&active).await,
                )
            } else {
                (None, None)
            };
            let is_default = active.is_default().await.unwrap_or(false);
            let network = Network {
                uuid: active.uuid().await?,
                kind,
                ssid,
                bssid,
            };
            // Prefer the connection holding the default route, else the first one.
            if best
                .as_ref()
                .is_none_or(|(default, _)| is_default && !default)
            {
                best = Some((is_default, network));
            }
        }
        Ok(best.map(|(_, network)| network))
    }

    /// Hardware address of the access point a Wi-Fi activation is using.
    /// `None` when it cannot be read: the strict check then refuses to trust.
    async fn access_point_bssid(&self, active: &NmActiveProxy<'_>) -> Option<String> {
        let device = active.devices().await.ok()?.into_iter().next()?;
        let wireless = NmWirelessProxy::builder(&self.conn)
            .path(device)
            .build()
            .await
            .ok()?;
        let ap = wireless.active_access_point().await.ok()?;
        if ap.as_str() == "/" {
            return None;
        }
        let ap = NmAccessPointProxy::builder(&self.conn)
            .path(ap)
            .build()
            .await
            .ok()?;
        ap.hw_address()
            .await
            .ok()
            .filter(|address| !address.is_empty())
    }

    /// Reads a tunnel back as a configuration, keys included, for the editor.
    /// Keys are kept in memory only for the time the editor is open.
    pub async fn read_config(&self, tunnel: &Tunnel) -> zbus::Result<WgConfig> {
        let proxy = self.connection_proxy(tunnel).await?;
        let settings = proxy.get_settings().await?;
        let secrets = proxy.get_secrets("wireguard").await?;
        Ok(config_from_settings(&settings, &secrets))
    }

    /// Saves an edited tunnel. Only the fields the editor manages are replaced:
    /// any other setting of the profile (routes, permissions…) is kept.
    pub async fn update(&self, tunnel: &Tunnel, config: &WgConfig, name: &str) -> zbus::Result<()> {
        let proxy = self.connection_proxy(tunnel).await?;
        let mut current = proxy.get_settings().await?;
        let new = to_nm_settings(config, name, &tunnel.interface, &tunnel.uuid);
        for (section, managed) in MANAGED_KEYS {
            let target = current.entry((*section).to_owned()).or_default();
            target.retain(|key, _| !managed.contains(&key.as_str()));
            for (key, value) in new.get(section).into_iter().flatten() {
                target.insert(
                    (*key).to_owned(),
                    OwnedVariant::try_from(value.try_clone()?)?,
                );
            }
        }
        proxy.update(current).await
    }

    async fn connection_proxy(&self, tunnel: &Tunnel) -> zbus::Result<NmConnectionProxy<'_>> {
        NmConnectionProxy::builder(&self.conn)
            .path(tunnel.path.clone())
            .build()
            .await
    }

    pub async fn delete(&self, tunnel: &Tunnel) -> zbus::Result<()> {
        let proxy = NmConnectionProxy::builder(&self.conn)
            .path(tunnel.path.clone())
            .build()
            .await?;
        proxy.delete().await
    }

    /// Stores a new profile in NetworkManager and returns its final, unique name.
    pub async fn import(
        &self,
        config: &WgConfig,
        name: &str,
        interface: &str,
    ) -> zbus::Result<String> {
        let existing = self.tunnels().await?;
        let mut unique = name.to_owned();
        let mut n = 2;
        while existing.iter().any(|t| t.name == unique) {
            unique = format!("{name} ({n})");
            n += 1;
        }
        // Two profiles may share an interface name only if never active together;
        // keep them distinct so switching tunnels never trips over a busy interface.
        let mut ifname = interface.to_owned();
        let mut n = 2;
        while existing.iter().any(|t| t.interface == ifname) {
            let suffix = format!("-{n}");
            let base: String = interface.chars().take(15 - suffix.len()).collect();
            ifname = format!("{base}{suffix}");
            n += 1;
        }
        let uuid = uuid::Uuid::new_v4().to_string();
        NmSettingsProxy::new(&self.conn)
            .await?
            .add_connection(to_nm_settings(config, &unique, &ifname, &uuid))
            .await?;
        Ok(unique)
    }

    /// Yields an item on every relevant NetworkManager change
    /// (profile added/removed/updated, active connection state change).
    pub async fn changes(&self) -> zbus::Result<impl Stream<Item = ()> + use<>> {
        let signal = || {
            MatchRule::builder()
                .msg_type(zbus::message::Type::Signal)
                .sender(NM_SERVICE)
        };
        let rules = [
            signal().interface("org.freedesktop.NetworkManager.Settings"),
            signal()
                .interface("org.freedesktop.NetworkManager.Settings.Connection")
                .member("Updated"),
            signal()
                .interface("org.freedesktop.NetworkManager.Connection.Active")
                .member("StateChanged"),
            signal()
                .path("/org/freedesktop/NetworkManager")
                .interface("org.freedesktop.DBus.Properties")
                .member("PropertiesChanged"),
        ];
        let mut streams = Vec::new();
        for rule in rules {
            streams.push(MessageStream::for_match_rule(rule.build()?, &self.conn, None).await?);
        }
        Ok(futures_util::stream::select_all(streams).map(|_| ()))
    }
}

/// Human-readable message for a D-Bus error (polkit refusal, invalid profile…).
/// Settings the editor owns, per section. Legacy forms (`addresses`, `dns`,
/// `dns-data`) are listed too, so an edit never leaves an old value that would
/// override the new one.
const MANAGED_KEYS: &[(&str, &[&str])] = &[
    ("connection", &["id"]),
    (
        "wireguard",
        &[
            "private-key",
            "private-key-flags",
            "peers",
            "mtu",
            "listen-port",
            "fwmark",
        ],
    ),
    (
        "ipv4",
        &[
            "method",
            "address-data",
            "addresses",
            "dns",
            "dns-data",
            "dns-search",
            "dns-priority",
        ],
    ),
    (
        "ipv6",
        &[
            "method",
            "address-data",
            "addresses",
            "dns",
            "dns-data",
            "dns-search",
            "dns-priority",
        ],
    ),
];

/// Rebuilds a configuration from a NetworkManager profile and its secrets.
fn config_from_settings(settings: &Settings, secrets: &Settings) -> WgConfig {
    let wg_u32 = |key: &str| get_u32(settings, "wireguard", key);
    let secret_peers = peers(secrets);
    let preshared = |public_key: &str| {
        secret_peers.iter().find_map(|peer| {
            let key = String::try_from(peer.get("public-key")?.try_clone().ok()?).ok()?;
            if key != public_key {
                return None;
            }
            String::try_from(peer.get("preshared-key")?.try_clone().ok()?).ok()
        })
    };
    let peers = peers(settings)
        .iter()
        .filter_map(|peer| {
            let text = |key: &str| String::try_from(peer.get(key)?.try_clone().ok()?).ok();
            let public_key = text("public-key")?;
            let allowed_ips = peer
                .get("allowed-ips")
                .and_then(|v| v.try_clone().ok())
                .and_then(|v| Vec::<String>::try_from(v).ok())
                .unwrap_or_default()
                .iter()
                .filter_map(|ip| parse_net_text(ip))
                .collect();
            let persistent_keepalive = peer
                .get("persistent-keepalive")
                .and_then(|v| v.try_clone().ok())
                .and_then(|v| u32::try_from(v).ok())
                .filter(|seconds| *seconds > 0);
            Some(Peer {
                preshared_key: preshared(&public_key),
                endpoint: text("endpoint"),
                allowed_ips,
                persistent_keepalive,
                public_key,
            })
        })
        .collect();
    let dns_search = ["ipv4", "ipv6"]
        .iter()
        .filter_map(|family| settings.get(*family)?.get("dns-search")?.try_clone().ok())
        .filter_map(|v| Vec::<String>::try_from(v).ok())
        .flatten()
        .filter(|domain| domain != "~")
        .collect();
    WgConfig {
        interface: Interface {
            private_key: get_str(secrets, "wireguard", "private-key").unwrap_or_default(),
            addresses: address_nets(settings),
            dns_servers: dns_servers(settings)
                .iter()
                .filter_map(|ip| ip.parse().ok())
                .collect(),
            dns_search,
            listen_port: wg_u32("listen-port")
                .and_then(|p| u16::try_from(p).ok())
                .filter(|p| *p > 0),
            mtu: wg_u32("mtu").filter(|mtu| *mtu > 0),
            fwmark: wg_u32("fwmark").filter(|mark| *mark > 0),
        },
        peers,
        ignored: Vec::new(),
    }
}

/// Wi-Fi name of a profile (`802-11-wireless.ssid`, raw bytes, usually UTF-8).
fn wifi_ssid(settings: &Settings) -> Option<String> {
    let bytes = settings
        .get("802-11-wireless")?
        .get("ssid")?
        .try_clone()
        .ok()?;
    let bytes = Vec::<u8>::try_from(bytes).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

fn get_u32(settings: &Settings, section: &str, key: &str) -> Option<u32> {
    u32::try_from(settings.get(section)?.get(key)?.try_clone().ok()?).ok()
}

fn parse_net_text(text: &str) -> Option<IpNet> {
    let (addr, prefix) = text.split_once('/')?;
    Some(IpNet {
        addr: addr.parse().ok()?,
        prefix: prefix.parse().ok()?,
    })
}

/// Addresses with their prefix, from `address-data`.
fn address_nets(settings: &Settings) -> Vec<IpNet> {
    let mut nets = Vec::new();
    for family in ["ipv4", "ipv6"] {
        let Some(entries) = settings
            .get(family)
            .and_then(|s| s.get("address-data"))
            .and_then(|v| v.try_clone().ok())
            .and_then(|v| Vec::<HashMap<String, OwnedVariant>>::try_from(v).ok())
        else {
            continue;
        };
        for entry in entries {
            let addr = entry
                .get("address")
                .and_then(|v| String::try_from(v.try_clone().ok()?).ok())
                .and_then(|a| a.parse::<IpAddr>().ok());
            let prefix = entry
                .get("prefix")
                .and_then(|v| u32::try_from(v.try_clone().ok()?).ok())
                .and_then(|p| u8::try_from(p).ok());
            if let (Some(addr), Some(prefix)) = (addr, prefix) {
                nets.push(IpNet { addr, prefix });
            }
        }
    }
    nets
}

pub fn error_message(err: &zbus::Error) -> String {
    match err {
        zbus::Error::MethodError(_, Some(detail), _) => detail.clone(),
        other => other.to_string(),
    }
}

fn get_str(settings: &Settings, section: &str, key: &str) -> Option<String> {
    let value = settings.get(section)?.get(key)?;
    String::try_from(value.try_clone().ok()?).ok()
}

fn first_endpoint(settings: &Settings) -> Option<String> {
    peers(settings)
        .iter()
        .find_map(|peer| String::try_from(peer.get("endpoint")?.try_clone().ok()?).ok())
}

fn peers(settings: &Settings) -> Vec<HashMap<String, OwnedVariant>> {
    settings
        .get("wireguard")
        .and_then(|wg| wg.get("peers"))
        .and_then(|peers| peers.try_clone().ok())
        .and_then(|peers| Vec::try_from(peers).ok())
        .unwrap_or_default()
}

fn is_full_tunnel(settings: &Settings) -> bool {
    peers(settings).iter().any(|peer| {
        peer.get("allowed-ips")
            .and_then(|ips| ips.try_clone().ok())
            .and_then(|ips| Vec::<String>::try_from(ips).ok())
            .is_some_and(|ips| ips.iter().any(|ip| ip.ends_with("/0")))
    })
}

/// DNS servers of the profile, from `dns-data` (recent NetworkManager) or the
/// legacy `dns` arrays (`au` for IPv4 in network order, `aay` for IPv6).
fn dns_servers(settings: &Settings) -> Vec<String> {
    let mut servers = Vec::new();
    for family in ["ipv4", "ipv6"] {
        let Some(section) = settings.get(family) else {
            continue;
        };
        if let Some(data) = section
            .get("dns-data")
            .and_then(|v| v.try_clone().ok())
            .and_then(|v| Vec::<String>::try_from(v).ok())
        {
            servers.extend(data);
            continue;
        }
        let Some(legacy) = section.get("dns").and_then(|v| v.try_clone().ok()) else {
            continue;
        };
        if family == "ipv4" {
            if let Ok(list) = Vec::<u32>::try_from(legacy) {
                servers.extend(
                    list.into_iter()
                        .map(|n| Ipv4Addr::from(n.to_ne_bytes()).to_string()),
                );
            }
        } else if let Ok(list) = Vec::<Vec<u8>>::try_from(legacy) {
            servers.extend(list.into_iter().filter_map(|bytes| {
                <[u8; 16]>::try_from(bytes)
                    .ok()
                    .map(|b| Ipv6Addr::from(b).to_string())
            }));
        }
    }
    servers
}

fn addresses(settings: &Settings) -> Vec<String> {
    let mut list = Vec::new();
    for family in ["ipv4", "ipv6"] {
        let Some(data) = settings.get(family).and_then(|ip| ip.get("address-data")) else {
            continue;
        };
        let Some(entries) = data
            .try_clone()
            .ok()
            .and_then(|d| Vec::<HashMap<String, OwnedVariant>>::try_from(d).ok())
        else {
            continue;
        };
        for entry in entries {
            if let Some(address) = entry
                .get("address")
                .and_then(|a| String::try_from(a.try_clone().ok()?).ok())
            {
                list.push(address);
            }
        }
    }
    list
}

/// Converts a `wg-quick` configuration into a NetworkManager profile, in the same
/// layout as `nmcli connection import type wireguard`.
fn to_nm_settings<'a>(
    config: &'a WgConfig,
    name: &'a str,
    interface: &'a str,
    uuid: &'a str,
) -> HashMap<&'a str, HashMap<&'a str, Variant<'a>>> {
    let iface = &config.interface;

    let connection = HashMap::from([
        ("id", Variant::from(name)),
        ("uuid", Variant::from(uuid)),
        ("type", Variant::from("wireguard")),
        ("interface-name", Variant::from(interface)),
        ("autoconnect", Variant::from(false)),
    ]);

    let peers: Vec<HashMap<&str, Variant>> = config
        .peers
        .iter()
        .map(|peer| {
            let mut map = HashMap::from([
                ("public-key", Variant::from(peer.public_key.as_str())),
                (
                    "allowed-ips",
                    Variant::from(
                        peer.allowed_ips
                            .iter()
                            .map(net_to_string)
                            .collect::<Vec<_>>(),
                    ),
                ),
            ]);
            if let Some(endpoint) = &peer.endpoint {
                map.insert("endpoint", Variant::from(endpoint.as_str()));
            }
            if let Some(psk) = &peer.preshared_key {
                map.insert("preshared-key", Variant::from(psk.as_str()));
                // 0 = secret stored by NetworkManager (file readable by root only).
                map.insert("preshared-key-flags", Variant::from(0u32));
            }
            if let Some(keepalive) = peer.persistent_keepalive {
                map.insert("persistent-keepalive", Variant::from(keepalive));
            }
            map
        })
        .collect();

    let mut wireguard = HashMap::from([
        ("private-key", Variant::from(iface.private_key.as_str())),
        ("private-key-flags", Variant::from(0u32)),
        ("peers", Variant::from(peers)),
    ]);
    if let Some(port) = iface.listen_port {
        wireguard.insert("listen-port", Variant::from(u32::from(port)));
    }
    if let Some(mtu) = iface.mtu {
        wireguard.insert("mtu", Variant::from(mtu));
    }
    if let Some(mark) = iface.fwmark {
        wireguard.insert("fwmark", Variant::from(mark));
    }

    let ipv4 = ip_settings(config, true);
    let ipv6 = ip_settings(config, false);

    HashMap::from([
        ("connection", connection),
        ("wireguard", wireguard),
        ("ipv4", ipv4),
        ("ipv6", ipv6),
    ])
}

fn ip_settings(config: &WgConfig, v4: bool) -> HashMap<&'static str, Variant<'static>> {
    let iface = &config.interface;
    let addresses: Vec<HashMap<&str, Variant>> = iface
        .addresses
        .iter()
        .filter(|net| net.addr.is_ipv4() == v4)
        .map(|net| {
            HashMap::from([
                ("address", Variant::from(net.addr.to_string())),
                ("prefix", Variant::from(u32::from(net.prefix))),
            ])
        })
        .collect();

    if addresses.is_empty() {
        return HashMap::from([("method", Variant::from("disabled"))]);
    }

    let mut ip = HashMap::from([
        ("method", Variant::from("manual")),
        ("address-data", Variant::from(addresses)),
    ]);

    // Legacy `dns` format (au / aay), understood by every NetworkManager version.
    let servers = iface.dns_servers.iter().filter(|ip| ip.is_ipv4() == v4);
    let dns = if v4 {
        let list: Vec<u32> = servers
            .filter_map(|ip| match ip {
                IpAddr::V4(ip) => Some(u32::from_ne_bytes(ip.octets())),
                IpAddr::V6(_) => None,
            })
            .collect();
        (!list.is_empty()).then(|| Variant::from(list))
    } else {
        let list: Vec<Vec<u8>> = servers
            .filter_map(|ip| match ip {
                IpAddr::V6(ip) => Some(ip.octets().to_vec()),
                IpAddr::V4(_) => None,
            })
            .collect();
        (!list.is_empty()).then(|| Variant::from(list))
    };

    if let Some(dns) = dns {
        ip.insert("dns", dns);
        // Like wg-quick: the VPN DNS servers answer every query (`~` = root
        // domain) and take precedence over the local network's ones.
        let mut search = iface.dns_search.clone();
        if config.is_full_tunnel() {
            search.push("~".to_owned());
            ip.insert("dns-priority", Variant::from(-50i32));
        }
        ip.insert("dns-search", Variant::from(search));
    } else if !iface.dns_search.is_empty() {
        ip.insert("dns-search", Variant::from(iface.dns_search.clone()));
    }
    ip
}

fn net_to_string(net: &IpNet) -> String {
    format!("{}/{}", net.addr, net.prefix)
}
