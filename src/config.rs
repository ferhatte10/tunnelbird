// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

//! Parser for tunnel configuration files in the `wg-quick` `.conf` format.
//!
//! VPN providers, routers and scripts all write this format slightly
//! differently, so the parser is lenient where it is safe to be:
//! byte order mark, Windows line endings, `#` or `;` comments (also at the end
//! of a line), any key case or spacing (`AllowedIPs`, `allowed_ips`,
//! `Allowed IPs`), quoted values, lists split by commas or spaces, repeated
//! keys and sections. It stays strict on what would break the tunnel: keys,
//! addresses and the server endpoint are validated, with the line number.
//!
//! Keys that only `wg-quick` understands (`PostUp` scripts, `Table`…) are
//! collected in [`WgConfig::ignored`] so the user is told, not silently ignored.
//! Files of another kind (OpenVPN, obfuscated variants, ZIP bundles) get a
//! dedicated message instead of a generic parse error.

use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use gettextrs::gettext;

/// Tunnel type shown to the user, like a provider name: it says what kind of
/// file was imported, never names the app. Plain text only (see docs/TRADEMARK.md).
pub const TUNNEL_TYPE: &str = "WireGuard®";

/// An IP network such as `10.0.0.2/32`.
#[derive(Debug, Clone, PartialEq)]
pub struct IpNet {
    pub addr: IpAddr,
    pub prefix: u8,
}

/// The `[Interface]` section: the local side of the tunnel.
#[derive(Debug, Default, Clone)]
pub struct Interface {
    pub private_key: String,
    pub addresses: Vec<IpNet>,
    pub dns_servers: Vec<IpAddr>,
    /// Non-IP entries of the `DNS` key are search domains, as in `wg-quick`.
    pub dns_search: Vec<String>,
    pub listen_port: Option<u16>,
    pub mtu: Option<u32>,
    pub fwmark: Option<u32>,
}

/// A `[Peer]` section: usually the VPN server.
#[derive(Debug, Default, Clone)]
pub struct Peer {
    pub public_key: String,
    pub preshared_key: Option<String>,
    /// `host:port` or `[ipv6]:port`, as written in the file.
    pub endpoint: Option<String>,
    pub allowed_ips: Vec<IpNet>,
    pub persistent_keepalive: Option<u32>,
}

#[derive(Debug, Default, Clone)]
pub struct WgConfig {
    pub interface: Interface,
    pub peers: Vec<Peer>,
    /// Keys present in the file but not supported (e.g. `PostUp`), sorted and deduplicated.
    pub ignored: Vec<String>,
}

#[derive(Debug)]
pub struct ParseError {
    /// 1-based line number, when the error is tied to a line.
    pub line: Option<usize>,
    pub kind: ErrorKind,
}

#[derive(Debug)]
pub enum ErrorKind {
    NoInterface,
    NoPrivateKey,
    NoPeer,
    PeerWithoutPublicKey,
    InvalidKey(String),
    InvalidValue {
        key: String,
        value: String,
    },
    InvalidEndpoint(String),
    UnknownSection(String),
    KeyOutsideSection,
    NotKeyValue,
    /// An OpenVPN profile (`.ovpn`): another protocol.
    OpenVpn,
    /// Obfuscation settings (AmneziaWG `Jc`, `S1`, `H1`…) the system tunnel cannot use.
    Obfuscated(Vec<String>),
    /// A ZIP archive, as some providers ship several configurations at once.
    Archive,
}

impl ParseError {
    /// The message without the line number, for places that point to the field instead.
    pub fn message(&self) -> String {
        match &self.kind {
            ErrorKind::NoInterface => gettext("The file has no [Interface] section"),
            ErrorKind::NoPrivateKey => gettext("The [Interface] section has no PrivateKey"),
            ErrorKind::NoPeer => gettext("The file has no [Peer] section"),
            ErrorKind::PeerWithoutPublicKey => gettext("A [Peer] section has no PublicKey"),
            ErrorKind::InvalidKey(key) => gettext("Invalid key for {key}").replace("{key}", key),
            ErrorKind::InvalidValue { key, value } => gettext("Invalid value for {key}: “{value}”")
                .replace("{key}", key)
                .replace("{value}", value),
            ErrorKind::InvalidEndpoint(value) => gettext(
                "Invalid server address “{value}”: expected host:port, for example vpn.example.com:51820",
            )
            .replace("{value}", value),
            ErrorKind::UnknownSection(name) => {
                gettext("Unknown section [{name}]").replace("{name}", name)
            }
            ErrorKind::KeyOutsideSection => gettext("Setting found outside of a section"),
            ErrorKind::NotKeyValue => gettext("Line is not of the form Key = Value"),
            ErrorKind::OpenVpn => gettext(
                "This is an OpenVPN profile. Tunnelbird does not support this protocol yet: ask your provider for a .conf tunnel file instead.",
            ),
            ErrorKind::Obfuscated(keys) => gettext(
                "This file uses obfuscation settings ({keys}) that the system cannot apply. Download a standard configuration from your provider.",
            )
            .replace("{keys}", &keys.join(", ")),
            ErrorKind::Archive => gettext(
                "This is a ZIP archive. Extract it first, then import the .conf files it contains.",
            ),
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let msg = self.message();
        match self.line {
            Some(line) => {
                let line = gettext("line {number}").replace("{number}", &line.to_string());
                write!(f, "{msg} ({line})")
            }
            None => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for ParseError {}

enum Section {
    None,
    Interface,
    Peer,
}

/// Obfuscation keys added by AmneziaWG. The kernel tunnel rejects them, so a
/// file that needs them cannot work and is refused with a clear message.
const OBFUSCATION_KEYS: &[&str] = &[
    "jc", "jmin", "jmax", "s1", "s2", "s3", "s4", "h1", "h2", "h3", "h4", "i1", "i2", "i3", "i4",
    "i5", "j1", "j2", "j3", "itime",
];

/// Parses the raw bytes of a configuration file. Never panics: any problem is a [`ParseError`].
pub fn parse_bytes(bytes: &[u8]) -> Result<WgConfig, ParseError> {
    if bytes.starts_with(b"PK\x03\x04") {
        return Err(ParseError {
            line: None,
            kind: ErrorKind::Archive,
        });
    }
    // Keys and values are ASCII: a stray non-UTF-8 byte in a comment must not
    // reject an otherwise valid file.
    parse(&String::from_utf8_lossy(bytes))
}

/// Parses the text of a configuration file. Never panics: any problem is a [`ParseError`].
pub fn parse(text: &str) -> Result<WgConfig, ParseError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut config = WgConfig::default();
    let mut section = Section::None;
    let mut has_interface = false;
    let mut obfuscation = Vec::new();

    for (index, raw) in text.lines().enumerate() {
        let line_no = index + 1;
        let err = |kind| ParseError {
            line: Some(line_no),
            kind,
        };
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }

        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            match normalize_key(name).as_str() {
                "interface" => {
                    section = Section::Interface;
                    has_interface = true;
                }
                "peer" => {
                    section = Section::Peer;
                    config.peers.push(Peer::default());
                }
                _ => return Err(err(ErrorKind::UnknownSection(name.trim().to_owned()))),
            }
            continue;
        }

        let Some((raw_key, raw_value)) = line.split_once('=') else {
            if !has_interface && looks_like_openvpn(text) {
                return Err(ParseError {
                    line: None,
                    kind: ErrorKind::OpenVpn,
                });
            }
            return Err(err(ErrorKind::NotKeyValue));
        };
        let key = raw_key.trim();
        let value = unquote(raw_value.trim());
        let normalized = normalize_key(key);
        let invalid = || {
            err(ErrorKind::InvalidValue {
                key: key.to_owned(),
                value: value.to_owned(),
            })
        };

        if OBFUSCATION_KEYS.contains(&normalized.as_str()) {
            obfuscation.push(key.to_owned());
            continue;
        }

        match section {
            Section::None => return Err(err(ErrorKind::KeyOutsideSection)),
            Section::Interface => {
                let iface = &mut config.interface;
                match normalized.as_str() {
                    "privatekey" => iface.private_key = check_key(key, value, line_no)?,
                    "address" => {
                        for item in split_list(value) {
                            iface.addresses.push(parse_net(item).ok_or_else(invalid)?);
                        }
                    }
                    "dns" => {
                        for item in split_list(value) {
                            match parse_ip(item) {
                                Some(ip) => iface.dns_servers.push(ip),
                                None => iface.dns_search.push(item.to_owned()),
                            }
                        }
                    }
                    "listenport" => iface.listen_port = Some(value.parse().map_err(|_| invalid())?),
                    "mtu" => iface.mtu = Some(value.parse().map_err(|_| invalid())?),
                    "fwmark" => iface.fwmark = parse_fwmark(value).ok_or_else(invalid)?,
                    _ => config.ignored.push(key.to_owned()),
                }
            }
            Section::Peer => {
                let Some(peer) = config.peers.last_mut() else {
                    return Err(err(ErrorKind::KeyOutsideSection));
                };
                match normalized.as_str() {
                    "publickey" => peer.public_key = check_key(key, value, line_no)?,
                    "presharedkey" => peer.preshared_key = Some(check_key(key, value, line_no)?),
                    "endpoint" => {
                        check_endpoint(value)
                            .ok_or_else(|| err(ErrorKind::InvalidEndpoint(value.to_owned())))?;
                        peer.endpoint = Some(value.to_owned());
                    }
                    "allowedips" => {
                        for item in split_list(value) {
                            let net = parse_net(item).ok_or_else(invalid)?;
                            peer.allowed_ips.push(network_of(net));
                        }
                    }
                    "persistentkeepalive" => {
                        peer.persistent_keepalive = match value {
                            "off" | "0" => None,
                            v => Some(v.parse().map_err(|_| invalid())?),
                        }
                    }
                    _ => config.ignored.push(key.to_owned()),
                }
            }
        }
    }

    let fail = |kind| Err(ParseError { line: None, kind });
    if !obfuscation.is_empty() {
        obfuscation.sort();
        obfuscation.dedup();
        return fail(ErrorKind::Obfuscated(obfuscation));
    }
    if !has_interface {
        return fail(if looks_like_openvpn(text) {
            ErrorKind::OpenVpn
        } else {
            ErrorKind::NoInterface
        });
    }
    if config.interface.private_key.is_empty() {
        return fail(ErrorKind::NoPrivateKey);
    }
    if config.peers.is_empty() {
        return fail(ErrorKind::NoPeer);
    }
    if config.peers.iter().any(|p| p.public_key.is_empty()) {
        return fail(ErrorKind::PeerWithoutPublicKey);
    }
    config.ignored.sort();
    config.ignored.dedup();
    Ok(config)
}

impl WgConfig {
    /// The whole configuration in the `.conf` format, keys included: for
    /// export and QR codes. Keys that were ignored on import are not written.
    pub fn to_conf(&self) -> String {
        let iface = &self.interface;
        let mut lines = vec![
            "[Interface]".to_owned(),
            format!("PrivateKey = {}", iface.private_key),
        ];
        if !iface.addresses.is_empty() {
            lines.push(format!("Address = {}", join_nets(&iface.addresses)));
        }
        let dns: Vec<String> = iface
            .dns_servers
            .iter()
            .map(ToString::to_string)
            .chain(iface.dns_search.iter().cloned())
            .collect();
        if !dns.is_empty() {
            lines.push(format!("DNS = {}", dns.join(", ")));
        }
        if let Some(port) = iface.listen_port {
            lines.push(format!("ListenPort = {port}"));
        }
        if let Some(mtu) = iface.mtu {
            lines.push(format!("MTU = {mtu}"));
        }
        if let Some(mark) = iface.fwmark {
            lines.push(format!("FwMark = {mark}"));
        }
        for peer in &self.peers {
            lines.push(String::new());
            lines.push(peer.to_conf());
        }
        lines.push(String::new());
        lines.join("\n")
    }

    /// True when all traffic goes through the tunnel (`AllowedIPs = 0.0.0.0/0` or `::/0`).
    pub fn is_full_tunnel(&self) -> bool {
        self.peers
            .iter()
            .flat_map(|p| &p.allowed_ips)
            .any(|net| net.prefix == 0)
    }
}

impl Peer {
    /// The peer written back in the `.conf` format (used to keep extra servers when editing).
    pub fn to_conf(&self) -> String {
        let mut lines = vec![
            "[Peer]".to_owned(),
            format!("PublicKey = {}", self.public_key),
        ];
        if let Some(psk) = &self.preshared_key {
            lines.push(format!("PresharedKey = {psk}"));
        }
        if let Some(endpoint) = &self.endpoint {
            lines.push(format!("Endpoint = {endpoint}"));
        }
        if !self.allowed_ips.is_empty() {
            lines.push(format!("AllowedIPs = {}", join_nets(&self.allowed_ips)));
        }
        if let Some(seconds) = self.persistent_keepalive {
            lines.push(format!("PersistentKeepalive = {seconds}"));
        }
        lines.join("\n")
    }
}

/// `10.0.0.2/32, fd00::2/128`
pub fn join_nets(nets: &[IpNet]) -> String {
    nets.iter()
        .map(|n| format!("{}/{}", n.addr, n.prefix))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Valid Linux network interface name (15 bytes max, `wg-quick` character set),
/// derived from the file name. Long names, common with provider files such as
/// `mullvad-fr-par-wg-001.conf`, keep a readable start plus a short hash of the
/// full name, so two similar files do not end up with the same interface.
pub fn interface_name(stem: &str) -> String {
    const MAX: usize = 15;
    let clean: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "_=+.-".contains(c) {
                c
            } else {
                '-'
            }
        })
        .collect();
    if clean.is_empty() || clean == "." || clean == ".." {
        return "wg0".to_owned();
    }
    if clean.len() <= MAX {
        return clean;
    }
    let hash = fnv1a(stem.as_bytes());
    format!("{}-{:04x}", &clean[..MAX - 5], hash & 0xffff)
}

fn fnv1a(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811c_9dc5, |hash, byte| {
        (hash ^ u32::from(*byte)).wrapping_mul(0x0100_0193)
    })
}

/// Removes a `#` or `;` comment. Neither character is valid inside a value.
fn strip_comment(line: &str) -> &str {
    match line.find(['#', ';']) {
        Some(index) => &line[..index],
        None => line,
    }
}

/// `AllowedIPs`, `allowed_ips` and `Allowed IPs` all become `allowedips`.
fn normalize_key(key: &str) -> String {
    key.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

fn unquote(value: &str) -> &str {
    ["\"", "'"]
        .iter()
        .find_map(|q| value.strip_prefix(q).and_then(|v| v.strip_suffix(q)))
        .unwrap_or(value)
        .trim()
}

/// Lists are comma-separated in the reference format; some tools use spaces.
fn split_list(value: &str) -> impl Iterator<Item = &str> {
    value
        .split([',', ' ', '\t'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// An IP address, also accepting the `[::1]` bracket form.
fn parse_ip(value: &str) -> Option<IpAddr> {
    let value = value
        .strip_prefix('[')
        .and_then(|v| v.strip_suffix(']'))
        .unwrap_or(value);
    value.parse().ok()
}

/// Parses `addr/prefix`; a bare address gets a host prefix (/32 or /128).
fn parse_net(value: &str) -> Option<IpNet> {
    let (addr, prefix) = match value.split_once('/') {
        Some((addr, prefix)) => (parse_ip(addr)?, Some(prefix.parse::<u8>().ok()?)),
        None => (parse_ip(value)?, None),
    };
    let max = if addr.is_ipv4() { 32 } else { 128 };
    let prefix = prefix.unwrap_or(max);
    (prefix <= max).then_some(IpNet { addr, prefix })
}

/// Clears host bits (`10.0.0.5/24` → `10.0.0.0/24`), as the kernel does for
/// allowed IPs; NetworkManager is stricter and may reject the original form.
fn network_of(net: IpNet) -> IpNet {
    let addr = match net.addr {
        IpAddr::V4(ip) => {
            let mask = u32::MAX
                .checked_shl(32 - u32::from(net.prefix))
                .unwrap_or(0);
            IpAddr::V4(Ipv4Addr::from(u32::from(ip) & mask))
        }
        IpAddr::V6(ip) => {
            let mask = u128::MAX
                .checked_shl(128 - u32::from(net.prefix))
                .unwrap_or(0);
            IpAddr::V6(Ipv6Addr::from(u128::from(ip) & mask))
        }
    };
    IpNet {
        addr,
        prefix: net.prefix,
    }
}

/// `host:port` or `[ipv6]:port`, with a valid port and a non-empty host.
fn check_endpoint(value: &str) -> Option<()> {
    let (host, port) = match value.strip_prefix('[') {
        Some(rest) => {
            let (host, port) = rest.split_once("]:")?;
            host.parse::<Ipv6Addr>().ok()?;
            (host, port)
        }
        None => value.rsplit_once(':')?,
    };
    let valid_host = !host.is_empty() && !host.contains(char::is_whitespace);
    (valid_host && port.parse::<u16>().is_ok_and(|p| p != 0)).then_some(())
}

/// `off`, a decimal or a `0x` hexadecimal mark. Outer `None` means invalid.
fn parse_fwmark(value: &str) -> Option<Option<u32>> {
    if value == "off" {
        return Some(None);
    }
    let mark = match value.strip_prefix("0x") {
        Some(hex) => u32::from_str_radix(hex, 16).ok()?,
        None => value.parse().ok()?,
    };
    Some(Some(mark))
}

/// A tunnel key is 32 bytes encoded in base64.
fn check_key(name: &str, value: &str, line: usize) -> Result<String, ParseError> {
    match BASE64.decode(value) {
        Ok(bytes) if bytes.len() == 32 => Ok(value.to_owned()),
        _ => Err(ParseError {
            line: Some(line),
            kind: ErrorKind::InvalidKey(name.to_owned()),
        }),
    }
}

/// Typical OpenVPN directives, to explain why an `.ovpn` file is refused.
fn looks_like_openvpn(text: &str) -> bool {
    text.lines().map(str::trim).any(|line| {
        line == "client"
            || line.starts_with("remote ")
            || line.starts_with("proto ")
            || line.starts_with("<ca>")
            || line.starts_with("auth-user-pass")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY_A: &str = "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=";
    const KEY_B: &str = "AgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgI=";
    const KEY_C: &str = "AwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwM=";

    fn kind(text: &str) -> ErrorKind {
        match parse(text) {
            Ok(_) => panic!("expected an error"),
            Err(err) => err.kind,
        }
    }

    #[test]
    fn mullvad_style_file() {
        let text = format!(
            "[Interface]\n# Device: Happy Otter\nPrivateKey = {KEY_A}\nAddress = 10.64.12.34/32,fc00:bbbb:bbbb:bb01::1:c22/128\nDNS = 10.64.0.1\n\n[Peer]\nPublicKey = {KEY_B}\nAllowedIPs = 0.0.0.0/0,::0/0\nEndpoint = 185.65.134.82:51820\n"
        );
        let config = parse(&text).expect("valid");
        assert_eq!(config.interface.addresses.len(), 2);
        assert!(config.is_full_tunnel());
        assert_eq!(
            config.peers[0].endpoint.as_deref(),
            Some("185.65.134.82:51820")
        );
    }

    #[test]
    fn proton_style_file_with_comments_and_preshared_key() {
        let text = format!(
            "# Key for laptop\n# Bouncing = 3\n# NetShield = 1\n[Interface]\nPrivateKey = {KEY_A}\nAddress = 10.2.0.2/32\nDNS = 10.2.0.1\n[Peer]\n# FR#42\nPublicKey = {KEY_B}\nPresharedKey = {KEY_C}\nAllowedIPs = 0.0.0.0/0\nEndpoint = 146.70.194.2:51820\n"
        );
        let config = parse(&text).expect("valid");
        assert_eq!(config.peers[0].preshared_key.as_deref(), Some(KEY_C));
        assert_eq!(
            config.interface.dns_servers,
            vec![IpAddr::from([10, 2, 0, 1])]
        );
    }

    #[test]
    fn windows_bom_crlf_quotes_and_odd_key_spelling() {
        let text = format!(
            "\u{feff}[interface]\r\nprivate_key = \"{KEY_A}\" ; generated\r\nAddress=10.0.0.2/24\r\nDNS = 1.1.1.1 1.0.0.1, home.lan\r\n[PEER]\r\nPublic Key = {KEY_B}\r\nAllowed IPs = 10.0.0.5/24\r\nAllowedIPs = 192.168.1.0/24\r\nEndpoint = [2001:db8::1]:51820\r\nPersistentKeepalive = 25\r\n"
        );
        let config = parse(&text).expect("valid");
        let peer = &config.peers[0];
        // Host bits are cleared and repeated keys accumulate.
        assert_eq!(
            peer.allowed_ips[0],
            IpNet {
                addr: IpAddr::from([10, 0, 0, 0]),
                prefix: 24
            }
        );
        assert_eq!(peer.allowed_ips.len(), 2);
        assert_eq!(config.interface.dns_servers.len(), 2);
        assert_eq!(config.interface.dns_search, vec!["home.lan".to_owned()]);
        assert_eq!(peer.persistent_keepalive, Some(25));
    }

    #[test]
    fn wg_quick_only_keys_are_reported() {
        let text = format!(
            "[Interface]\nPrivateKey = {KEY_A}\nAddress = 10.0.0.2/32\nTable = off\nPostUp = iptables -A FORWARD\nSaveConfig = true\n[Peer]\nPublicKey = {KEY_B}\nAllowedIPs = 10.0.0.0/24\nEndpoint = vpn.example.com:51820\n"
        );
        let config = parse(&text).expect("valid");
        assert_eq!(config.ignored, vec!["PostUp", "SaveConfig", "Table"]);
        assert!(!config.is_full_tunnel());
    }

    #[test]
    fn several_peers() {
        let text = format!(
            "[Interface]\nPrivateKey = {KEY_A}\n[Peer]\nPublicKey = {KEY_B}\nAllowedIPs = 10.1.0.0/16\nEndpoint = a.example.com:51820\n[Peer]\nPublicKey = {KEY_C}\nAllowedIPs = 10.2.0.0/16\nEndpoint = b.example.com:443\n"
        );
        assert_eq!(parse(&text).expect("valid").peers.len(), 2);
    }

    #[test]
    fn clear_errors_for_other_formats() {
        assert!(matches!(
            kind("client\ndev tun\nproto udp\nremote vpn.example.com 1194\n<ca>\n</ca>\n"),
            ErrorKind::OpenVpn
        ));
        let amnezia = format!(
            "[Interface]\nPrivateKey = {KEY_A}\nJc = 4\nJmin = 40\nH1 = 1\n[Peer]\nPublicKey = {KEY_B}\nEndpoint = x.example.com:51820\n"
        );
        assert!(matches!(kind(&amnezia), ErrorKind::Obfuscated(keys) if keys.len() == 3));
        assert!(matches!(
            parse_bytes(b"PK\x03\x04rest of zip")
                .map(|_| ())
                .map_err(|e| e.kind),
            Err(ErrorKind::Archive)
        ));
    }

    #[test]
    fn invalid_values_point_to_the_line() {
        let text = format!(
            "[Interface]\nPrivateKey = {KEY_A}\n[Peer]\nPublicKey = {KEY_B}\nEndpoint = vpn.example.com\n"
        );
        let err = parse(&text).expect_err("missing port");
        assert_eq!(err.line, Some(5));
        assert!(matches!(err.kind, ErrorKind::InvalidEndpoint(_)));
        assert!(matches!(
            kind("[Interface]\nPrivateKey = <your key>\n"),
            ErrorKind::InvalidKey(_)
        ));
    }

    #[test]
    fn export_reads_back_identically() {
        let text = format!(
            "[Interface]\nPrivateKey = {KEY_A}\nAddress = 10.2.0.2/32, fd00::2/128\nDNS = 10.2.0.1, home.lan\nMTU = 1380\n[Peer]\nPublicKey = {KEY_B}\nPresharedKey = {KEY_C}\nAllowedIPs = 0.0.0.0/0, ::/0\nEndpoint = vpn.example.com:51820\nPersistentKeepalive = 25\n"
        );
        let first = parse(&text).expect("valid");
        let again = parse(&first.to_conf()).expect("exported file is valid");
        assert_eq!(again.to_conf(), first.to_conf());
        assert_eq!(again.peers[0].preshared_key.as_deref(), Some(KEY_C));
        assert_eq!(again.interface.mtu, Some(1380));
    }

    #[test]
    fn interface_names_are_valid_and_distinct() {
        assert_eq!(interface_name("homelab"), "homelab");
        assert_eq!(interface_name("my vpn"), "my-vpn");
        let a = interface_name("mullvad-fr-par-wg-001");
        let b = interface_name("mullvad-fr-par-wg-002");
        assert!(a.len() <= 15 && b.len() <= 15);
        assert_ne!(a, b);
        assert_eq!(interface_name(""), "wg0");
    }
}
