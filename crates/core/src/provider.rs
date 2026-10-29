// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

//! Recognises well-known VPN providers from a tunnel's settings, so the list
//! can show "Mullvad", "Proton VPN"… next to a tunnel. Names only, no logos:
//! those are other companies' trademarks.
//!
//! Hints are public facts found in the providers' own configuration files:
//! server domains, server address ranges, internal DNS servers, and the usual
//! file names. A self-hosted server matches nothing and shows no label.
//! To add a provider, append an entry to [`PROVIDERS`] and a test below.

/// What is known about a tunnel when looking for its provider.
pub struct Hints<'a> {
    /// Profile name, usually the imported file name (`mullvad-fr-par-wg-001`).
    pub name: &'a str,
    /// Server address as written in the profile (`host:port` or `[ipv6]:port`).
    pub endpoint: Option<&'a str>,
    /// DNS servers pushed by the tunnel.
    pub dns: &'a [String],
}

struct Provider {
    name: &'static str,
    /// Server host names end with one of these domains.
    domains: &'static [&'static str],
    /// Server IP addresses start with one of these prefixes.
    endpoint_prefixes: &'static [&'static str],
    /// The tunnel uses one of these internal DNS servers.
    dns: &'static [&'static str],
    /// A word of the profile name starts with one of these.
    name_hints: &'static [&'static str],
}

const PROVIDERS: &[Provider] = &[
    Provider {
        name: "Mullvad",
        domains: &["mullvad.net"],
        endpoint_prefixes: &[],
        dns: &["10.64.0.1"],
        name_hints: &["mullvad"],
    },
    Provider {
        name: "Proton VPN",
        domains: &["protonvpn.net", "protonvpn.com", "proton.me"],
        endpoint_prefixes: &[],
        dns: &["10.2.0.1"],
        name_hints: &["proton", "protonvpn"],
    },
    Provider {
        name: "Surfshark",
        domains: &["surfshark.com"],
        endpoint_prefixes: &[],
        dns: &["162.252.172.57", "149.154.159.92"],
        name_hints: &["surfshark"],
    },
    Provider {
        name: "NordVPN",
        domains: &["nordvpn.com"],
        endpoint_prefixes: &[],
        dns: &["103.86.96.100", "103.86.99.100"],
        name_hints: &["nordvpn", "nordlynx"],
    },
    Provider {
        name: "Cloudflare WARP",
        domains: &["cloudflareclient.com"],
        endpoint_prefixes: &["162.159.192.", "162.159.193.", "162.159.195."],
        dns: &[],
        name_hints: &["warp", "wgcf"],
    },
    Provider {
        name: "IVPN",
        domains: &["ivpn.net"],
        endpoint_prefixes: &[],
        dns: &[],
        name_hints: &["ivpn"],
    },
    Provider {
        name: "AzireVPN",
        domains: &["azirevpn.net"],
        endpoint_prefixes: &[],
        dns: &[],
        name_hints: &["azire", "azirevpn"],
    },
    Provider {
        name: "AirVPN",
        domains: &["airdns.org", "airvpn.org"],
        endpoint_prefixes: &[],
        dns: &[],
        name_hints: &["airvpn"],
    },
    Provider {
        name: "Windscribe",
        domains: &["windscribe.com"],
        endpoint_prefixes: &[],
        dns: &[],
        name_hints: &["windscribe"],
    },
    Provider {
        name: "Private Internet Access",
        domains: &["privacy.network", "privateinternetaccess.com"],
        endpoint_prefixes: &[],
        dns: &[],
        name_hints: &["pia"],
    },
];

/// Provider name, or `None` for a self-hosted or unknown server. Server
/// details win over the profile name, which the user may have changed.
pub fn detect(hints: &Hints) -> Option<&'static str> {
    let host = hints
        .endpoint
        .map(endpoint_host)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let by_server = PROVIDERS.iter().find(|p| {
        p.domains
            .iter()
            .any(|d| host == *d || host.ends_with(&format!(".{d}")))
            || p.endpoint_prefixes
                .iter()
                .any(|prefix| host.starts_with(prefix))
    });
    let by_dns = || {
        PROVIDERS.iter().find(|p| {
            hints
                .dns
                .iter()
                .any(|server| p.dns.contains(&server.as_str()))
        })
    };
    let by_name = || {
        let name = hints.name.to_ascii_lowercase();
        let words: Vec<&str> = name.split(|c: char| !c.is_ascii_alphanumeric()).collect();
        PROVIDERS.iter().find(|p| {
            p.name_hints
                .iter()
                .any(|hint| words.iter().any(|word| word.starts_with(hint)))
        })
    };
    by_server.or_else(by_dns).or_else(by_name).map(|p| p.name)
}

/// `vpn.example.com:51820` → `vpn.example.com`, `[2001:db8::1]:51820` → `2001:db8::1`.
fn endpoint_host(endpoint: &str) -> &str {
    match endpoint.strip_prefix('[') {
        Some(rest) => rest.split(']').next().unwrap_or(rest),
        None => endpoint.rsplit_once(':').map_or(endpoint, |(host, _)| host),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detect_with(name: &str, endpoint: Option<&str>, dns: &[&str]) -> Option<&'static str> {
        let dns: Vec<String> = dns.iter().map(|s| (*s).to_owned()).collect();
        detect(&Hints {
            name,
            endpoint,
            dns: &dns,
        })
    }

    #[test]
    fn recognises_providers_from_their_files() {
        assert_eq!(
            detect_with("fr-par-001", Some("185.65.134.82:51820"), &["10.64.0.1"]),
            Some("Mullvad")
        );
        assert_eq!(
            detect_with("laptop-FR-42", Some("146.70.194.2:51820"), &["10.2.0.1"]),
            Some("Proton VPN")
        );
        assert_eq!(
            detect_with("de", Some("de-fra.prod.surfshark.com:51820"), &[]),
            Some("Surfshark")
        );
        assert_eq!(
            detect_with(
                "wgcf-profile",
                Some("engage.cloudflareclient.com:2408"),
                &[]
            ),
            Some("Cloudflare WARP")
        );
        assert_eq!(
            detect_with("x", Some("162.159.192.1:2408"), &[]),
            Some("Cloudflare WARP")
        );
        assert_eq!(
            detect_with("mullvad-se-got-wg-101", Some("193.138.218.1:51820"), &[]),
            Some("Mullvad")
        );
    }

    #[test]
    fn self_hosted_and_lookalike_names_stay_unlabelled() {
        assert_eq!(
            detect_with("homelab", Some("vpn.ferhatsn.fr:51820"), &["172.18.0.2"]),
            None
        );
        // "olympia" contains "pia" but is not a word starting with it.
        assert_eq!(detect_with("olympia", Some("10.0.0.1:51820"), &[]), None);
        assert_eq!(
            detect_with("notmullvad.net", Some("evil-mullvad.net:1"), &[]),
            None
        );
    }
}
