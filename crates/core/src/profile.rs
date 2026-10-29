// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

//! A tunnel profile of any supported kind: a WireGuard® `.conf` tunnel
//! ([`config`]) or an OpenVPN `.ovpn` profile ([`ovpn`]). Imported files are
//! recognised by their content, never by their extension.

use crate::config::{self, WgConfig};
use crate::ovpn::{self, OvpnConfig};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    #[default]
    WireGuard,
    OpenVpn,
}

impl Kind {
    /// The type shown to the user, like a provider name (see docs/TRADEMARK.md).
    pub fn label(self) -> &'static str {
        match self {
            Kind::WireGuard => config::TUNNEL_TYPE,
            Kind::OpenVpn => ovpn::TUNNEL_TYPE,
        }
    }

    /// Phone apps scan WireGuard® tunnels from a QR code; OpenVPN has no such
    /// standard.
    pub fn has_qr_code(self) -> bool {
        self == Kind::WireGuard
    }

    /// File extension used when exporting.
    pub fn extension(self) -> &'static str {
        match self {
            Kind::WireGuard => "conf",
            Kind::OpenVpn => "ovpn",
        }
    }
}

#[derive(Debug, Clone)]
pub enum Profile {
    WireGuard(WgConfig),
    /// Boxed: much larger than a WireGuard® tunnel (certificates).
    OpenVpn(Box<OvpnConfig>),
}

impl Profile {
    pub fn kind(&self) -> Kind {
        match self {
            Profile::WireGuard(_) => Kind::WireGuard,
            Profile::OpenVpn(_) => Kind::OpenVpn,
        }
    }

    /// Settings of the file that Tunnelbird does not apply.
    pub fn ignored(&self) -> &[String] {
        match self {
            Profile::WireGuard(config) => &config.ignored,
            Profile::OpenVpn(config) => &config.ignored,
        }
    }

    /// The file's text, as exported.
    pub fn to_text(&self) -> String {
        match self {
            Profile::WireGuard(config) => config.to_conf(),
            Profile::OpenVpn(config) => config.to_ovpn(),
        }
    }
}

/// Parses an imported file of either kind. OpenVPN certificate files it refers
/// to are not read yet (see [`OvpnConfig::resolve_files`]).
pub fn parse_bytes(bytes: &[u8]) -> Result<Profile, String> {
    let text = String::from_utf8_lossy(bytes);
    let wireguard = text
        .lines()
        .any(|line| line.trim().eq_ignore_ascii_case("[interface]"));
    if !wireguard && !bytes.starts_with(b"PK\x03\x04") && ovpn::looks_like(&text) {
        return ovpn::parse(&text)
            .map(|config| Profile::OpenVpn(Box::new(config)))
            .map_err(|err| err.to_string());
    }
    config::parse_bytes(bytes)
        .map(Profile::WireGuard)
        .map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_are_recognised_by_content() {
        let ovpn = b"client\ndev tun\nremote vpn.example.com 1194\n<ca>\nX\n</ca>\n";
        assert!(matches!(parse_bytes(ovpn), Ok(Profile::OpenVpn(_))));
        let conf = b"[Interface]\nPrivateKey = AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=\n[Peer]\nPublicKey = AgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgI=\nEndpoint = a.example.com:51820\nAllowedIPs = 0.0.0.0/0\n";
        assert!(matches!(parse_bytes(conf), Ok(Profile::WireGuard(_))));
        assert!(parse_bytes(b"PK\x03\x04zip").is_err());
    }
}
