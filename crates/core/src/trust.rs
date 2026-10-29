// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

//! Trusted networks: connect a tunnel automatically on networks the user does
//! not trust (café or hotel Wi-Fi), never on their own (home, office).
//!
//! The rule is only applied when the computer joins a network: if the user
//! then disconnects by hand, Tunnelbird respects it until the next network.
//! It never disconnects anything by itself.
//!
//! A Wi-Fi name is not unique: anyone can create a hotspot named like your
//! home network ("evil twin"). The optional strict check also requires the
//! access point's hardware address (BSSID) to be one the user trusted. It is
//! pinned when trusting the network you are on, or learned the first time you
//! join a network trusted from the saved list.

/// What the computer is connected to, as far as trust is concerned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Network {
    /// NetworkManager profile UUID: identifies "joining a new network".
    pub uuid: String,
    pub kind: NetworkKind,
    /// Wi-Fi name, when `kind` is `Wifi`.
    pub ssid: Option<String>,
    /// Hardware address of the Wi-Fi access point (`AA:BB:CC:DD:EE:FF`).
    pub bssid: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkKind {
    Wifi,
    Wired,
    /// Mobile broadband, Bluetooth tethering…: treated like an untrusted Wi-Fi.
    Other,
}

/// An access point the user trusts, for the strict check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pin {
    pub bssid: String,
    pub ssid: String,
}

/// The user's choices, from the preferences file.
#[derive(Debug, Clone, Default)]
pub struct Policy {
    pub enabled: bool,
    /// UUID of the tunnel to connect.
    pub tunnel: String,
    pub trusted_ssids: Vec<String>,
    pub trust_wired: bool,
    /// Also check the access point hardware address of trusted Wi-Fi networks.
    pub strict: bool,
    pub pins: Vec<Pin>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Trusted network: do nothing.
    Stay,
    /// Untrusted network: connect the chosen tunnel.
    Connect,
    /// Trusted Wi-Fi joined for the first time with the strict check on:
    /// remember this access point, then stay.
    Pin(Pin),
}

impl Policy {
    /// What to do when the computer joins `network`.
    pub fn decide(&self, network: &Network) -> Decision {
        if !self.enabled || self.tunnel.is_empty() {
            return Decision::Stay;
        }
        let trusted = match network.kind {
            NetworkKind::Wired => self.trust_wired,
            NetworkKind::Other => false,
            NetworkKind::Wifi => match &network.ssid {
                Some(ssid) if self.trusted_ssids.contains(ssid) => {
                    return self.check_access_point(ssid, network.bssid.as_deref());
                }
                _ => false,
            },
        };
        if trusted {
            Decision::Stay
        } else {
            Decision::Connect
        }
    }

    fn check_access_point(&self, ssid: &str, bssid: Option<&str>) -> Decision {
        if !self.strict {
            return Decision::Stay;
        }
        // Without the hardware address, the network cannot be verified.
        let Some(bssid) = bssid.map(normalize_bssid).filter(|b| !b.is_empty()) else {
            return Decision::Connect;
        };
        let mut pinned = self.pins.iter().filter(|pin| pin.ssid == ssid).peekable();
        if pinned.peek().is_none() {
            return Decision::Pin(Pin {
                bssid,
                ssid: ssid.to_owned(),
            });
        }
        if pinned.any(|pin| pin.bssid == bssid) {
            Decision::Stay
        } else {
            Decision::Connect
        }
    }

    /// Number of access points pinned for `ssid`.
    pub fn pins_for(&self, ssid: &str) -> usize {
        self.pins.iter().filter(|pin| pin.ssid == ssid).count()
    }

    /// True when `bssid` is a pinned access point of `ssid`.
    pub fn is_pinned(&self, ssid: &str, bssid: &str) -> bool {
        let bssid = normalize_bssid(bssid);
        self.pins
            .iter()
            .any(|pin| pin.ssid == ssid && pin.bssid == bssid)
    }
}

/// `aa:bb:…` and `AA:BB:…` are the same access point.
pub fn normalize_bssid(bssid: &str) -> String {
    bssid.trim().to_ascii_uppercase()
}

/// NetworkManager connection types that are a way to reach the internet.
/// Tunnels and virtual interfaces are not "a network you joined".
pub fn kind_of(nm_type: &str) -> Option<NetworkKind> {
    match nm_type {
        "802-11-wireless" => Some(NetworkKind::Wifi),
        "802-3-ethernet" => Some(NetworkKind::Wired),
        "gsm" | "cdma" | "bluetooth" | "pppoe" | "wimax" => Some(NetworkKind::Other),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME_AP: &str = "AA:BB:CC:00:11:22";

    fn wifi(ssid: &str, bssid: &str) -> Network {
        Network {
            uuid: format!("uuid-{ssid}"),
            kind: NetworkKind::Wifi,
            ssid: Some(ssid.to_owned()),
            bssid: Some(bssid.to_owned()),
        }
    }

    fn policy() -> Policy {
        Policy {
            enabled: true,
            tunnel: "tunnel-uuid".to_owned(),
            trusted_ssids: vec!["Freebox-Home".to_owned()],
            trust_wired: true,
            strict: false,
            pins: Vec::new(),
        }
    }

    fn strict() -> Policy {
        Policy {
            strict: true,
            pins: vec![Pin {
                bssid: HOME_AP.to_owned(),
                ssid: "Freebox-Home".to_owned(),
            }],
            ..policy()
        }
    }

    #[test]
    fn untrusted_wifi_connects_trusted_does_not() {
        assert_eq!(
            policy().decide(&wifi("Starbucks", "11:22:33:44:55:66")),
            Decision::Connect
        );
        assert_eq!(
            policy().decide(&wifi("Freebox-Home", "11:22:33:44:55:66")),
            Decision::Stay
        );
    }

    #[test]
    fn wired_follows_the_option_and_mobile_is_untrusted() {
        let wired = Network {
            uuid: "eth".to_owned(),
            kind: NetworkKind::Wired,
            ssid: None,
            bssid: None,
        };
        assert_eq!(policy().decide(&wired), Decision::Stay);
        assert_eq!(
            Policy {
                trust_wired: false,
                ..policy()
            }
            .decide(&wired),
            Decision::Connect
        );
        let mobile = Network {
            uuid: "4g".to_owned(),
            kind: NetworkKind::Other,
            ssid: None,
            bssid: None,
        };
        assert_eq!(policy().decide(&mobile), Decision::Connect);
    }

    #[test]
    fn nothing_happens_when_disabled_or_without_tunnel() {
        let cafe = wifi("Starbucks", "11:22:33:44:55:66");
        assert_eq!(
            Policy {
                enabled: false,
                ..policy()
            }
            .decide(&cafe),
            Decision::Stay
        );
        assert_eq!(
            Policy {
                tunnel: String::new(),
                ..policy()
            }
            .decide(&cafe),
            Decision::Stay
        );
    }

    #[test]
    fn strict_check_rejects_an_evil_twin() {
        assert_eq!(
            strict().decide(&wifi("Freebox-Home", "aa:bb:cc:00:11:22")),
            Decision::Stay
        );
        // Same name, other hardware: a hotspot pretending to be home.
        assert_eq!(
            strict().decide(&wifi("Freebox-Home", "66:55:44:33:22:11")),
            Decision::Connect
        );
    }

    #[test]
    fn strict_check_learns_the_first_access_point() {
        let fresh = Policy {
            strict: true,
            ..policy()
        };
        assert_eq!(
            fresh.decide(&wifi("Freebox-Home", "aa:bb:cc:00:11:22")),
            Decision::Pin(Pin {
                bssid: HOME_AP.to_owned(),
                ssid: "Freebox-Home".to_owned()
            })
        );
        let mut no_bssid = wifi("Freebox-Home", "");
        no_bssid.bssid = None;
        assert_eq!(fresh.decide(&no_bssid), Decision::Connect);
    }

    #[test]
    fn tunnels_are_not_networks() {
        assert_eq!(kind_of("wireguard"), None);
        assert_eq!(kind_of("vpn"), None);
        assert_eq!(kind_of("802-11-wireless"), Some(NetworkKind::Wifi));
    }
}
