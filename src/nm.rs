// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

//! NetworkManager client over the system D-Bus.
//!
//! Tunnelbird never creates the network interface itself: it asks NetworkManager
//! to store, activate or deactivate a WireGuard profile, or an OpenVPN one run by
//! NetworkManager's OpenVPN plugin. Permissions are therefore handled by polkit
//! and the app runs without any privilege.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use futures_util::{Stream, StreamExt};
use gtk::glib;
use zbus::{
    Connection, MatchRule, MessageStream, ObjectPath, OwnedObjectPath, OwnedVariant, Variant, proxy,
};

use crate::config::{Interface, IpNet, Peer, WgConfig};
use crate::openvpn;
use crate::ovpn::OvpnConfig;
use crate::profile::{Kind, Profile};
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

    #[allow(clippy::type_complexity)]
    fn add_and_activate_connection2(
        &self,
        connection: HashMap<&str, HashMap<&str, Variant<'_>>>,
        device: &ObjectPath<'_>,
        specific_object: &ObjectPath<'_>,
        options: HashMap<&str, Variant<'_>>,
    ) -> zbus::Result<(
        OwnedObjectPath,
        OwnedObjectPath,
        HashMap<String, OwnedVariant>,
    )>;

    fn get_all_devices(&self) -> zbus::Result<Vec<OwnedObjectPath>>;

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

    /// True when this connection holds the default IPv6 route.
    #[zbus(property, name = "Default6")]
    fn is_default6(&self) -> zbus::Result<bool>;

    #[zbus(property)]
    fn devices(&self) -> zbus::Result<Vec<OwnedObjectPath>>;

    #[zbus(property)]
    fn ip4_config(&self) -> zbus::Result<OwnedObjectPath>;
}

#[proxy(
    interface = "org.freedesktop.NetworkManager.Device",
    default_service = "org.freedesktop.NetworkManager"
)]
trait NmDevice {
    #[zbus(property)]
    fn interface(&self) -> zbus::Result<String>;

    #[zbus(property)]
    fn ip_interface(&self) -> zbus::Result<String>;

    #[zbus(property)]
    fn ip4_config(&self) -> zbus::Result<OwnedObjectPath>;
}

#[proxy(
    interface = "org.freedesktop.NetworkManager.IP4Config",
    default_service = "org.freedesktop.NetworkManager"
)]
trait NmIp4Config {
    #[zbus(property)]
    fn address_data(&self) -> zbus::Result<Vec<HashMap<String, OwnedVariant>>>;
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
    pub kind: Kind,
    /// Network interface. OpenVPN's (`tun0`…) is only known while connected.
    pub interface: String,
    pub endpoint: Option<String>,
    /// Addresses of this computer inside the VPN (`10.8.0.3/32`…).
    pub addresses: Vec<String>,
    /// True when all internet traffic goes through the tunnel (`AllowedIPs` has a `/0`).
    pub full_tunnel: bool,
    /// Connected and holding the default IPv6 route. A full tunnel without it
    /// would let IPv6 traffic leave outside the VPN (see `needs_ipv6_guard`).
    pub carries_ipv6: bool,
    /// Well-known VPN provider recognised from the settings (see `provider.rs`).
    pub provider: Option<&'static str>,
    pub state: TunnelState,
    /// Matching active connection, needed to deactivate the tunnel.
    pub active_path: Option<OwnedObjectPath>,
}

/// A tunnel being connected or connected.
struct Active {
    path: OwnedObjectPath,
    state: TunnelState,
    /// True when it holds the default route (all traffic goes through it).
    is_default: bool,
    /// The same for IPv6.
    is_default6: bool,
    /// OpenVPN: the interface NetworkManager created and its addresses.
    link: Option<(String, Vec<String>)>,
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

    /// WireGuard and OpenVPN profiles known to NetworkManager with their
    /// current state, sorted by name.
    pub async fn tunnels(&self) -> zbus::Result<Vec<Tunnel>> {
        let mut active = self.active_tunnels().await?;
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
            let Some(kind) = tunnel_kind(&settings) else {
                continue;
            };
            let uuid = get_str(&settings, "connection", "uuid").unwrap_or_default();
            let active = active.remove(&uuid);
            let name = get_str(&settings, "connection", "id").unwrap_or_default();
            let (interface, endpoint, addresses, full_tunnel, dns) = match kind {
                Kind::WireGuard => (
                    get_str(&settings, "connection", "interface-name").unwrap_or_default(),
                    first_endpoint(&settings),
                    addresses(&settings),
                    is_full_tunnel(&settings),
                    dns_servers(&settings),
                ),
                Kind::OpenVpn => {
                    let (interface, addresses) = active
                        .as_ref()
                        .and_then(|a| a.link.clone())
                        .unwrap_or_default();
                    // The server decides (redirect-gateway): known once connected.
                    let full = match &active {
                        Some(a) if a.state == TunnelState::Connected => a.is_default,
                        _ => !get_bool(&settings, "ipv4", "never-default"),
                    };
                    let endpoint =
                        openvpn::from_nm_data(&vpn_data(&settings), None, |_| None).endpoint();
                    (interface, endpoint, addresses, full, Vec::new())
                }
            };
            let provider = crate::provider::detect(&crate::provider::Hints {
                name: &name,
                endpoint: endpoint.as_deref(),
                dns: &dns,
            });
            let carries_ipv6 = active
                .as_ref()
                .is_some_and(|a| a.state == TunnelState::Connected && a.is_default6);
            let (state, active_path) = match active {
                Some(active) => (active.state, Some(active.path)),
                None => (TunnelState::Disconnected, None),
            };
            tunnels.push(Tunnel {
                path,
                name,
                kind,
                interface,
                endpoint,
                provider,
                addresses,
                full_tunnel,
                carries_ipv6,
                uuid,
                state,
                active_path,
            });
        }
        tunnels.sort_by_key(|t| t.name.to_lowercase());
        Ok(tunnels)
    }

    async fn active_tunnels(&self) -> zbus::Result<HashMap<String, Active>> {
        let nm = NetworkManagerProxy::new(&self.conn).await?;
        let mut active = HashMap::new();
        for path in nm.active_connections().await? {
            let proxy = NmActiveProxy::builder(&self.conn)
                .path(path.clone())
                .build()
                .await?;
            let kind = proxy.kind().await.ok();
            if !matches!(kind.as_deref(), Some("wireguard" | "vpn")) {
                continue;
            }
            let state = match proxy.state().await.unwrap_or(0) {
                1 => TunnelState::Connecting,
                2 => TunnelState::Connected,
                3 => TunnelState::Disconnecting,
                _ => TunnelState::Disconnected,
            };
            let link = if kind.as_deref() == Some("vpn") && state == TunnelState::Connected {
                self.vpn_link(&proxy).await
            } else {
                None
            };
            if let Ok(uuid) = proxy.uuid().await {
                let is_default = proxy.is_default().await.unwrap_or(false);
                let is_default6 = proxy.is_default6().await.unwrap_or(false);
                active.insert(
                    uuid,
                    Active {
                        path,
                        state,
                        is_default,
                        is_default6,
                        link,
                    },
                );
            }
        }
        Ok(active)
    }

    /// The interface and addresses of a connected OpenVPN tunnel. The active
    /// connection does not name its interface: it is the device carrying the
    /// same IP configuration (or, failing that, the same addresses).
    async fn vpn_link(&self, active: &NmActiveProxy<'_>) -> Option<(String, Vec<String>)> {
        let config = active
            .ip4_config()
            .await
            .ok()
            .filter(|p| p.as_str() != "/")?;
        let addresses = self.ip4_addresses(&config).await;
        let nm = NetworkManagerProxy::new(&self.conn).await.ok()?;
        for path in nm.get_all_devices().await.ok()? {
            let Ok(device) = NmDeviceProxy::builder(&self.conn).path(path).build().await else {
                continue;
            };
            let Ok(device_config) = device.ip4_config().await else {
                continue;
            };
            let same = device_config == config
                || (!addresses.is_empty()
                    && self
                        .ip4_addresses(&device_config)
                        .await
                        .iter()
                        .any(|address| addresses.contains(address)));
            if !same {
                continue;
            }
            let interface = match device.ip_interface().await {
                Ok(name) if !name.is_empty() => name,
                _ => device.interface().await.ok()?,
            };
            return Some((interface, addresses));
        }
        glib::g_debug!(
            crate::LOG_DOMAIN,
            "No interface found for an OpenVPN tunnel"
        );
        None
    }

    /// `address/prefix` list of an IPv4 configuration object.
    async fn ip4_addresses(&self, path: &OwnedObjectPath) -> Vec<String> {
        if path.as_str() == "/" {
            return Vec::new();
        }
        let Ok(config) = NmIp4ConfigProxy::builder(&self.conn)
            .path(path.clone())
            .build()
            .await
        else {
            return Vec::new();
        };
        config
            .address_data()
            .await
            .unwrap_or_default()
            .iter()
            .filter_map(|entry| {
                let address = String::try_from(entry.get("address")?.try_clone().ok()?).ok()?;
                let prefix = u32::try_from(entry.get("prefix")?.try_clone().ok()?).ok()?;
                Some(format!("{address}/{prefix}"))
            })
            .collect()
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

    /// Turns the IPv6 guard on or off (see `needs_ipv6_guard`). The guard is
    /// an in-memory dummy connection whose two `unreachable` routes (`::/1`,
    /// `8000::/1`) are more specific than any default route, so IPv6 to the
    /// internet fails at once and applications fall back to IPv4, which goes
    /// through the tunnel. The local network (its own `/64`, `fe80::/10`) is
    /// more specific still and keeps working. NetworkManager forgets the
    /// connection when it is deactivated, and at the latest on restart.
    pub async fn set_ipv6_guard(&self, on: bool) -> zbus::Result<()> {
        let nm = NetworkManagerProxy::new(&self.conn).await?;
        let mut guard = None;
        for path in nm.active_connections().await? {
            let proxy = NmActiveProxy::builder(&self.conn)
                .path(path.clone())
                .build()
                .await?;
            if proxy.uuid().await.ok().as_deref() == Some(IPV6_GUARD_UUID) {
                guard = Some(path);
                break;
            }
        }
        match (on, guard) {
            (true, None) => {
                let root = ObjectPath::from_static_str_unchecked("/");
                let options = HashMap::from([("persist", Variant::from("volatile"))]);
                nm.add_and_activate_connection2(ipv6_guard_settings(), &root, &root, options)
                    .await?;
            }
            (false, Some(path)) => nm.deactivate_connection(&path).await?,
            _ => {}
        }
        Ok(())
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

    /// Reads a tunnel back as a profile, keys included, for the editor and
    /// sharing. Keys are kept in memory only for the time they are needed.
    pub async fn read_profile(&self, tunnel: &Tunnel) -> zbus::Result<Profile> {
        let proxy = self.connection_proxy(tunnel).await?;
        let settings = proxy.get_settings().await?;
        match tunnel.kind {
            Kind::WireGuard => {
                let secrets = proxy.get_secrets("wireguard").await?;
                Ok(Profile::WireGuard(config_from_settings(
                    &settings, &secrets,
                )))
            }
            Kind::OpenVpn => {
                // A password kept by the desktop keyring is not NetworkManager's
                // to give: none is fine.
                let password = proxy
                    .get_secrets("vpn")
                    .await
                    .ok()
                    .and_then(|secrets| vpn_dict(&secrets, "secrets").remove("password"));
                Ok(Profile::OpenVpn(Box::new(openvpn::from_nm_data(
                    &vpn_data(&settings),
                    password,
                    |path| std::fs::read_to_string(path).ok(),
                ))))
            }
        }
    }

    /// Saves an edited tunnel. Only the fields the editor manages are replaced:
    /// any other setting of the profile (routes, permissions…) is kept.
    pub async fn update(&self, tunnel: &Tunnel, profile: &Profile, name: &str) -> zbus::Result<()> {
        let config = match profile {
            Profile::WireGuard(config) => config,
            Profile::OpenVpn(config) => return self.update_openvpn(tunnel, config, name).await,
        };
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

    /// OpenVPN: the plugin settings are all the editor's, so the `vpn`
    /// section is replaced as a whole; the rest of the profile is kept.
    async fn update_openvpn(
        &self,
        tunnel: &Tunnel,
        config: &OvpnConfig,
        name: &str,
    ) -> zbus::Result<()> {
        let paths = openvpn::store_files(&tunnel.uuid, config).map_err(io_error)?;
        let proxy = self.connection_proxy(tunnel).await?;
        let mut current = proxy.get_settings().await?;
        let new = openvpn_settings(config, &paths, name, &tunnel.uuid);
        for section in ["connection", "vpn"] {
            let target = current.entry(section.to_owned()).or_default();
            if section == "vpn" {
                target.clear();
            }
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
        profile: &Profile,
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
        let settings = NmSettingsProxy::new(&self.conn).await?;
        match profile {
            Profile::WireGuard(config) => {
                settings
                    .add_connection(to_nm_settings(config, &unique, &ifname, &uuid))
                    .await?;
            }
            Profile::OpenVpn(config) => {
                // NetworkManager reads the certificates from files.
                let paths = openvpn::store_files(&uuid, config).map_err(io_error)?;
                let added = settings
                    .add_connection(openvpn_settings(config, &paths, &unique, &uuid))
                    .await;
                if let Err(err) = added {
                    if let Err(cleanup) = openvpn::remove_files(&uuid) {
                        glib::g_warning!(
                            crate::LOG_DOMAIN,
                            "Cannot remove certificates: {cleanup}"
                        );
                    }
                    return Err(err);
                }
            }
        }
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

    /// Yields NetworkManager's reason each time a connection ends.
    pub async fn end_reasons(&self) -> zbus::Result<impl Stream<Item = u32> + use<>> {
        let rule = MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender(NM_SERVICE)
            .interface("org.freedesktop.NetworkManager.Connection.Active")
            .member("StateChanged")
            .build()?;
        let stream = MessageStream::for_match_rule(rule, &self.conn, None).await?;
        Ok(stream.filter_map(|message| {
            // NMActiveConnectionState: 4 is "deactivated".
            let reason = message
                .ok()
                .and_then(|message| message.body().deserialize::<(u32, u32)>().ok())
                .and_then(|(state, reason)| (state == 4).then_some(reason));
            std::future::ready(reason)
        }))
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

/// Fixed identity of the IPv6 guard, so it is found again after a restart.
const IPV6_GUARD_UUID: &str = "6b7e0d9c-4f3a-4c1e-9a52-7d1f3e8b2a64";
const IPV6_GUARD_NAME: &str = "Tunnelbird IPv6 protection";
const IPV6_GUARD_INTERFACE: &str = "tunnelbird-v6";

/// True when the connected tunnel carries all IPv4 traffic but not IPv6:
/// IPv6 would then reach the internet outside the VPN, revealing the real
/// address (most OpenVPN servers and some WireGuard® files are IPv4 only).
pub fn needs_ipv6_guard(tunnels: &[Tunnel]) -> bool {
    tunnels
        .iter()
        .any(|t| t.state == TunnelState::Connected && t.full_tunnel && !t.carries_ipv6)
}

fn ipv6_guard_settings() -> HashMap<&'static str, HashMap<&'static str, Variant<'static>>> {
    let unreachable = |dest: &str| {
        HashMap::from([
            ("dest", Variant::from(dest.to_owned())),
            ("prefix", Variant::from(1u32)),
            ("type", Variant::from("unreachable")),
        ])
    };
    // A unique local address: the interface needs one to hold the routes.
    let address = HashMap::from([
        ("address", Variant::from("fd8a:7f7e:6c1d:42::2")),
        ("prefix", Variant::from(64u32)),
    ]);
    HashMap::from([
        (
            "connection",
            HashMap::from([
                ("id", Variant::from(IPV6_GUARD_NAME)),
                ("uuid", Variant::from(IPV6_GUARD_UUID)),
                ("type", Variant::from("dummy")),
                ("interface-name", Variant::from(IPV6_GUARD_INTERFACE)),
                ("autoconnect", Variant::from(false)),
            ]),
        ),
        (
            "ipv4",
            HashMap::from([("method", Variant::from("disabled"))]),
        ),
        (
            "ipv6",
            HashMap::from([
                ("method", Variant::from("manual")),
                ("address-data", Variant::from(vec![address])),
                (
                    "route-data",
                    Variant::from(vec![unreachable("::"), unreachable("8000::")]),
                ),
            ]),
        ),
    ])
}

fn net_to_string(net: &IpNet) -> String {
    format!("{}/{}", net.addr, net.prefix)
}

/// Which kind of tunnel a profile is, or `None` for anything else.
fn tunnel_kind(settings: &Settings) -> Option<Kind> {
    match get_str(settings, "connection", "type")?.as_str() {
        "wireguard" => Some(Kind::WireGuard),
        "vpn"
            if get_str(settings, "vpn", "service-type").as_deref()
                == Some(openvpn::SERVICE_TYPE) =>
        {
            Some(Kind::OpenVpn)
        }
        _ => None,
    }
}

/// The OpenVPN plugin's settings (`vpn.data`).
fn vpn_data(settings: &Settings) -> HashMap<String, String> {
    vpn_dict(settings, "data")
}

/// A string map of the `vpn` section (`data` or `secrets`).
fn vpn_dict(settings: &Settings, key: &str) -> HashMap<String, String> {
    settings
        .get("vpn")
        .and_then(|vpn| vpn.get(key))
        .and_then(|value| value.try_clone().ok())
        .and_then(|value| HashMap::<String, String>::try_from(value).ok())
        .unwrap_or_default()
}

fn get_bool(settings: &Settings, section: &str, key: &str) -> bool {
    settings
        .get(section)
        .and_then(|s| s.get(key))
        .and_then(|v| v.try_clone().ok())
        .and_then(|v| bool::try_from(v).ok())
        .unwrap_or(false)
}

/// An OpenVPN profile for NetworkManager's plugin, in the layout
/// `nm-connection-editor` writes. Addresses, routes and DNS come from the server.
fn openvpn_settings<'a>(
    config: &'a OvpnConfig,
    paths: &openvpn::Paths,
    name: &'a str,
    uuid: &'a str,
) -> HashMap<&'a str, HashMap<&'a str, Variant<'a>>> {
    let connection = HashMap::from([
        ("id", Variant::from(name)),
        ("uuid", Variant::from(uuid)),
        ("type", Variant::from("vpn")),
        ("autoconnect", Variant::from(false)),
    ]);
    let data: HashMap<String, String> = openvpn::to_nm_data(config, paths)
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect();
    let mut vpn = HashMap::from([
        ("service-type", Variant::from(openvpn::SERVICE_TYPE)),
        ("data", Variant::from(data)),
    ]);
    if config.user_pass && !config.password.is_empty() {
        let secrets = HashMap::from([("password".to_owned(), config.password.clone())]);
        vpn.insert("secrets", Variant::from(secrets));
    }
    let auto = || HashMap::from([("method", Variant::from("auto"))]);
    HashMap::from([
        ("connection", connection),
        ("vpn", vpn),
        ("ipv4", auto()),
        ("ipv6", auto()),
    ])
}

/// A file error (writing certificates) reported like a D-Bus one.
fn io_error(err: std::io::Error) -> zbus::Error {
    zbus::Error::Failure(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tunnel(state: TunnelState, full_tunnel: bool, carries_ipv6: bool) -> Tunnel {
        Tunnel {
            path: "/t".try_into().unwrap(),
            uuid: "u".to_owned(),
            name: "Stockholm".to_owned(),
            kind: Kind::OpenVpn,
            interface: "tun0".to_owned(),
            endpoint: None,
            addresses: Vec::new(),
            full_tunnel,
            carries_ipv6,
            provider: None,
            state,
            active_path: None,
        }
    }

    #[test]
    fn ipv6_is_blocked_only_around_an_ipv4_only_full_tunnel() {
        use TunnelState::*;
        // IPv4 only, all traffic: IPv6 would leak.
        assert!(needs_ipv6_guard(&[tunnel(Connected, true, false)]));
        // IPv6 through the tunnel too: nothing to block.
        assert!(!needs_ipv6_guard(&[tunnel(Connected, true, true)]));
        // Split tunnel: the rest of the traffic is meant to go outside.
        assert!(!needs_ipv6_guard(&[tunnel(Connected, false, false)]));
        // Not connected (yet): the network is left alone.
        assert!(!needs_ipv6_guard(&[tunnel(Connecting, true, false)]));
        assert!(!needs_ipv6_guard(&[tunnel(Disconnected, true, false)]));
        assert!(!needs_ipv6_guard(&[]));
    }
}
