// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

//! A WireGuard® tunnel turned into what a system applies: decoded keys,
//! addresses, routes and DNS, ready for the Windows driver (WireGuardNT) or
//! the macOS userspace tunnel (BoringTun, configured through the UAPI text
//! protocol). Linux does not need it: NetworkManager takes the profile as is.
//!
//! Pure logic, unit-tested here, so the system-specific code only has to apply
//! it (and only that part needs the target machine to be checked).

use std::fmt::Write;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;

use crate::config::{IpNet, WgConfig};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WgPlan {
    pub private_key: [u8; 32],
    pub listen_port: Option<u16>,
    pub mtu: Option<u32>,
    pub addresses: Vec<IpNet>,
    pub dns_servers: Vec<IpAddr>,
    pub dns_search: Vec<String>,
    pub peers: Vec<PeerPlan>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerPlan {
    pub public_key: [u8; 32],
    pub preshared_key: Option<[u8; 32]>,
    pub endpoint: Option<Endpoint>,
    pub allowed_ips: Vec<IpNet>,
    pub keepalive: Option<u16>,
}

/// A server address as written (`vpn.example.com:51820`, `[2001:db8::1]:51820`);
/// the system resolves the name when connecting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    pub host: String,
    pub port: u16,
}

impl Endpoint {
    pub fn parse(text: &str) -> Option<Self> {
        let (host, port) = match text.strip_prefix('[') {
            Some(rest) => rest.split_once("]:")?,
            None => text.rsplit_once(':')?,
        };
        let port = port.parse().ok().filter(|p| *p != 0)?;
        (!host.is_empty()).then(|| Self {
            host: host.to_owned(),
            port,
        })
    }

    /// `host:port`, brackets around an IPv6 address, for name resolution.
    pub fn to_lookup(&self) -> String {
        if self.host.contains(':') {
            format!("[{}]:{}", self.host, self.port)
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

impl WgPlan {
    pub fn from_config(config: &WgConfig) -> Result<Self, String> {
        let peers = config
            .peers
            .iter()
            .map(|peer| {
                Ok(PeerPlan {
                    public_key: key(&peer.public_key)?,
                    preshared_key: peer.preshared_key.as_deref().map(key).transpose()?,
                    endpoint: peer.endpoint.as_deref().and_then(Endpoint::parse),
                    allowed_ips: peer.allowed_ips.clone(),
                    keepalive: peer
                        .persistent_keepalive
                        .map(|s| u16::try_from(s).unwrap_or(u16::MAX)),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(Self {
            private_key: key(&config.interface.private_key)?,
            listen_port: config.interface.listen_port,
            mtu: config.interface.mtu,
            addresses: config.interface.addresses.clone(),
            dns_servers: config.interface.dns_servers.clone(),
            dns_search: config.interface.dns_search.clone(),
            peers,
        })
    }

    /// True when all traffic goes through the tunnel.
    pub fn full_tunnel(&self) -> bool {
        self.peers
            .iter()
            .flat_map(|p| &p.allowed_ips)
            .any(|net| net.prefix == 0)
    }

    /// True when all IPv4 goes through the tunnel but IPv6 does not: IPv6 would
    /// leave outside the VPN and reveal the real address, so it must be blocked.
    pub fn leaves_ipv6_outside(&self) -> bool {
        let mut default_routes = self
            .peers
            .iter()
            .flat_map(|p| &p.allowed_ips)
            .filter(|net| net.prefix == 0);
        let (mut ipv4, mut ipv6) = (false, false);
        for net in &mut default_routes {
            match net.addr {
                IpAddr::V4(_) => ipv4 = true,
                IpAddr::V6(_) => ipv6 = true,
            }
        }
        ipv4 && !ipv6
    }

    /// Routes to add through the tunnel. A default route (`/0`) becomes its
    /// two halves (`0.0.0.0/1` + `128.0.0.0/1`, `::/1` + `8000::/1`), as
    /// `wg-quick` does: they win over the system's default route without
    /// replacing it, so it comes back by itself when the tunnel goes down.
    /// The server's own address must then go through the old gateway
    /// ([`Self::bypass`]).
    pub fn routes(&self) -> Vec<IpNet> {
        let mut routes = Vec::new();
        for net in self.peers.iter().flat_map(|p| &p.allowed_ips) {
            let pieces = match (net.addr, net.prefix) {
                (IpAddr::V4(_), 0) => vec![
                    net_v4(Ipv4Addr::new(0, 0, 0, 0), 1),
                    net_v4(Ipv4Addr::new(128, 0, 0, 0), 1),
                ],
                (IpAddr::V6(_), 0) => vec![
                    net_v6(Ipv6Addr::UNSPECIFIED, 1),
                    net_v6(Ipv6Addr::new(0x8000, 0, 0, 0, 0, 0, 0, 0), 1),
                ],
                _ => vec![net.clone()],
            };
            for piece in pieces {
                if !routes.contains(&piece) {
                    routes.push(piece);
                }
            }
        }
        routes
    }

    /// Host routes that must keep using the physical network: the servers'
    /// resolved addresses, when all traffic goes through the tunnel.
    pub fn bypass(&self, resolved: &[Option<SocketAddr>]) -> Vec<IpNet> {
        if !self.full_tunnel() {
            return Vec::new();
        }
        resolved
            .iter()
            .flatten()
            .map(|addr| IpNet {
                addr: addr.ip(),
                prefix: if addr.is_ipv4() { 32 } else { 128 },
            })
            .collect()
    }

    /// The configuration in WireGuard's cross-platform UAPI text protocol
    /// (`set=1`), as BoringTun and wireguard-go read it on their control
    /// socket. `resolved` holds each peer's server address, in order.
    pub fn uapi(&self, resolved: &[Option<SocketAddr>]) -> String {
        let mut out = String::from("set=1\n");
        let _ = writeln!(out, "private_key={}", hex(&self.private_key));
        if let Some(port) = self.listen_port {
            let _ = writeln!(out, "listen_port={port}");
        }
        out.push_str("replace_peers=true\n");
        for (index, peer) in self.peers.iter().enumerate() {
            let _ = writeln!(out, "public_key={}", hex(&peer.public_key));
            if let Some(psk) = &peer.preshared_key {
                let _ = writeln!(out, "preshared_key={}", hex(psk));
            }
            if let Some(Some(addr)) = resolved.get(index) {
                let _ = writeln!(out, "endpoint={addr}");
            }
            if let Some(seconds) = peer.keepalive {
                let _ = writeln!(out, "persistent_keepalive_interval={seconds}");
            }
            out.push_str("replace_allowed_ips=true\n");
            for net in &peer.allowed_ips {
                let _ = writeln!(out, "allowed_ip={}/{}", net.addr, net.prefix);
            }
        }
        out.push('\n');
        out
    }
}

fn key(text: &str) -> Result<[u8; 32], String> {
    BASE64
        .decode(text.trim())
        .ok()
        .and_then(|bytes| <[u8; 32]>::try_from(bytes).ok())
        .ok_or_else(|| "Invalid key".to_owned())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

fn net_v4(addr: Ipv4Addr, prefix: u8) -> IpNet {
    IpNet {
        addr: IpAddr::V4(addr),
        prefix,
    }
}

fn net_v6(addr: Ipv6Addr, prefix: u8) -> IpNet {
    IpNet {
        addr: IpAddr::V6(addr),
        prefix,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config;

    const KEY_A: &str = "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=";
    const KEY_B: &str = "AgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgI=";

    fn plan(allowed: &str) -> WgPlan {
        let text = format!(
            "[Interface]\nPrivateKey = {KEY_A}\nAddress = 10.8.0.2/32, fd00::2/128\nDNS = 1.1.1.1, home.lan\n[Peer]\nPublicKey = {KEY_B}\nEndpoint = vpn.example.com:51820\nAllowedIPs = {allowed}\nPersistentKeepalive = 25\n"
        );
        WgPlan::from_config(&config::parse(&text).expect("valid")).expect("plan")
    }

    #[test]
    fn keys_and_settings_are_decoded() {
        let plan = plan("10.8.0.0/24");
        assert_eq!(plan.private_key, [1; 32]);
        assert_eq!(plan.peers[0].public_key, [2; 32]);
        assert_eq!(
            plan.peers[0].endpoint,
            Some(Endpoint {
                host: "vpn.example.com".to_owned(),
                port: 51820
            })
        );
        assert_eq!(plan.dns_search, vec!["home.lan"]);
        assert!(!plan.full_tunnel());
    }

    #[test]
    fn default_routes_are_split_and_the_server_bypasses_the_tunnel() {
        let plan = plan("0.0.0.0/0, ::/0");
        let routes: Vec<String> = plan
            .routes()
            .iter()
            .map(|n| format!("{}/{}", n.addr, n.prefix))
            .collect();
        assert_eq!(routes, ["0.0.0.0/1", "128.0.0.0/1", "::/1", "8000::/1"]);
        let server: SocketAddr = "203.0.113.10:51820".parse().expect("address");
        assert_eq!(
            plan.bypass(&[Some(server)]),
            vec![IpNet {
                addr: server.ip(),
                prefix: 32
            }]
        );
        assert!(self::plan("10.0.0.0/8").bypass(&[Some(server)]).is_empty());
    }

    #[test]
    fn ipv6_is_left_outside_only_by_a_full_ipv4_tunnel() {
        assert!(self::plan("0.0.0.0/0").leaves_ipv6_outside());
        assert!(!self::plan("0.0.0.0/0, ::/0").leaves_ipv6_outside());
        assert!(!self::plan("::/0").leaves_ipv6_outside());
        assert!(!self::plan("10.0.0.0/8").leaves_ipv6_outside());
    }

    #[test]
    fn uapi_text_matches_the_protocol() {
        let server: SocketAddr = "203.0.113.10:51820".parse().expect("address");
        let text = plan("10.8.0.0/24").uapi(&[Some(server)]);
        assert_eq!(
            text,
            format!(
                "set=1\nprivate_key={}\nreplace_peers=true\npublic_key={}\nendpoint=203.0.113.10:51820\npersistent_keepalive_interval=25\nreplace_allowed_ips=true\nallowed_ip=10.8.0.0/24\n\n",
                "01".repeat(32),
                "02".repeat(32)
            )
        );
    }

    #[test]
    fn ipv6_endpoints_keep_their_brackets() {
        let endpoint = Endpoint::parse("[2001:db8::1]:443").expect("valid");
        assert_eq!(endpoint.host, "2001:db8::1");
        assert_eq!(endpoint.to_lookup(), "[2001:db8::1]:443");
        assert!(Endpoint::parse("vpn.example.com").is_none());
    }
}
