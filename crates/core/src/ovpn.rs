// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

//! OpenVPN profiles (`.ovpn`): parser and writer.
//!
//! The parser keeps what providers actually use (servers, certificates,
//! encryption, user name and password) and lists the rest in
//! [`OvpnConfig::ignored`], like the WireGuard® parser. How a system runs the
//! profile is the app's business (on Linux, NetworkManager's plugin).
//!
//! Certificates and keys may be inline (`<ca>…</ca>`) or separate files
//! (`ca ca.crt`); [`OvpnConfig::resolve_files`] loads the latter.

use std::fmt;
use std::path::Path;

use crate::i18n::gettext;

/// Tunnel type shown to the user.
pub const TUNNEL_TYPE: &str = "OpenVPN";
/// OpenVPN's port when a profile gives none.
pub const DEFAULT_PORT: u16 = 1194;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Proto {
    #[default]
    Udp,
    Tcp,
}

impl Proto {
    /// `udp`, `tcp-client`… as written in a profile.
    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "udp" | "udp4" | "udp6" => Some(Proto::Udp),
            "tcp" | "tcp4" | "tcp6" | "tcp-client" | "tcp4-client" | "tcp6-client" => {
                Some(Proto::Tcp)
            }
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Proto::Udp => "udp",
            Proto::Tcp => "tcp",
        }
    }
}

/// A `remote` line: one server to try.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remote {
    pub host: String,
    pub port: Option<u16>,
    pub proto: Option<Proto>,
}

/// Where a certificate or key comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// The content itself (inline block, or a file already read).
    Inline(String),
    /// A file name written in the profile, not read yet.
    File(String),
}

impl Source {
    pub fn inline(&self) -> Option<&str> {
        match self {
            Source::Inline(text) => Some(text),
            Source::File(_) => None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OvpnConfig {
    pub remotes: Vec<Remote>,
    /// Default port and protocol for `remote` lines that do not give theirs.
    pub port: Option<u16>,
    pub proto: Proto,
    /// `tap` (Ethernet bridge) instead of the usual `tun`.
    pub tap: bool,
    pub ca: Option<Source>,
    pub cert: Option<Source>,
    pub key: Option<Source>,
    pub tls_auth: Option<Source>,
    /// `key-direction`, or the second argument of `tls-auth`.
    pub key_direction: Option<u8>,
    pub tls_crypt: Option<Source>,
    pub tls_crypt_v2: Option<Source>,
    /// `auth-user-pass`: the server asks for a user name and a password.
    pub user_pass: bool,
    /// Never in the file: typed in the editor.
    pub username: String,
    pub password: String,
    pub cipher: Option<String>,
    pub data_ciphers: Option<String>,
    pub auth: Option<String>,
    pub remote_cert_tls: Option<String>,
    /// `verify-x509-name <name> [type]`, as (name, type).
    pub verify_x509_name: Option<(String, String)>,
    pub tls_version_min: Option<String>,
    pub comp_lzo: Option<String>,
    pub compress: Option<String>,
    /// `redirect-gateway`: all traffic through the VPN. Servers often push it
    /// instead, so `false` only means “decided by the server”.
    pub redirect_gateway: bool,
    pub tun_mtu: Option<u32>,
    pub mssfix: Option<u32>,
    pub reneg_sec: Option<u32>,
    pub ping: Option<u32>,
    pub ping_restart: Option<u32>,
    pub remote_random: bool,
    /// Directives present but not supported (scripts, proxies…), sorted and deduplicated.
    pub ignored: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OvpnError {
    NoRemote,
    NoCa,
    /// Neither a client certificate nor `auth-user-pass`.
    NoCredentials,
    MissingValue {
        line: usize,
        directive: String,
    },
    InvalidValue {
        line: usize,
        directive: String,
        value: String,
    },
    Unsupported {
        line: usize,
        directive: String,
    },
    UnclosedBlock(String),
    /// A certificate or key file the profile refers to, not found.
    MissingFile(String),
}

impl fmt::Display for OvpnError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let at = |line: &usize| gettext("line {number}").replace("{number}", &line.to_string());
        let text = match self {
            OvpnError::NoRemote => gettext("The profile has no server (remote line)"),
            OvpnError::NoCa => gettext("The profile has no server certificate (ca)"),
            OvpnError::NoCredentials => gettext(
                "The profile has neither a client certificate nor a user name and password (auth-user-pass)",
            ),
            OvpnError::MissingValue { line, directive } => format!(
                "{} ({})",
                gettext("{directive} needs a value").replace("{directive}", directive),
                at(line)
            ),
            OvpnError::InvalidValue {
                line,
                directive,
                value,
            } => format!(
                "{} ({})",
                gettext("Invalid value for {key}: “{value}”")
                    .replace("{key}", directive)
                    .replace("{value}", value),
                at(line)
            ),
            OvpnError::Unsupported { line, directive } => format!(
                "{} ({})",
                gettext("“{directive}” is not supported yet: ask your provider for a profile with certificates in PEM format")
                    .replace("{directive}", directive),
                at(line)
            ),
            OvpnError::UnclosedBlock(tag) => {
                gettext("The <{tag}> block is never closed").replace("{tag}", tag)
            }
            OvpnError::MissingFile(name) => gettext(
                "The profile refers to the file “{file}”, which was not found. Select it together with the .ovpn file, or ask your provider for a profile with everything inside.",
            )
            .replace("{file}", name),
        };
        f.write_str(&text)
    }
}

impl std::error::Error for OvpnError {}

/// Directives that change nothing for a NetworkManager client: accepted silently.
const HARMLESS: &[&str] = &[
    "client",
    "nobind",
    "persist-key",
    "persist-tun",
    "resolv-retry",
    "verb",
    "mute",
    "pull",
    "tls-client",
    "auth-nocache",
    "explicit-exit-notify",
    "mute-replay-warnings",
    "float",
    "sndbuf",
    "rcvbuf",
    "fast-io",
    "remote-cert-ku",
    "remote-cert-eku",
    "ns-cert-type",
    "setenv",
    "ignore-unknown-option",
    "block-outside-dns",
    "route-delay",
    "connect-retry",
    "connect-retry-max",
    "server-poll-timeout",
    "push-peer-info",
    "allow-compression",
];

/// True when the text looks like an OpenVPN profile rather than a `.conf` tunnel.
pub fn looks_like(text: &str) -> bool {
    text.lines().map(str::trim).any(|line| {
        line == "client"
            || line.starts_with("remote ")
            || line.starts_with("proto ")
            || line.starts_with("<ca>")
            || line.starts_with("auth-user-pass")
    })
}

/// Parses the text of an `.ovpn` profile. File references are kept as
/// [`Source::File`]: call [`OvpnConfig::resolve_files`], then [`OvpnConfig::check`].
pub fn parse(text: &str) -> Result<OvpnConfig, OvpnError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut config = OvpnConfig::default();
    let mut lines = text.lines().enumerate();

    while let Some((index, raw)) = lines.next() {
        let line_no = index + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }

        // Inline block: <ca> … </ca>
        if let Some(tag) = line
            .strip_prefix('<')
            .and_then(|l| l.strip_suffix('>'))
            .filter(|tag| !tag.starts_with('/'))
        {
            let tag = tag.trim().to_ascii_lowercase();
            let closing = format!("</{tag}>");
            let mut body = Vec::new();
            loop {
                match lines.next() {
                    Some((_, l)) if l.trim().eq_ignore_ascii_case(&closing) => break,
                    Some((_, l)) => body.push(l.trim_end()),
                    None => return Err(OvpnError::UnclosedBlock(tag)),
                }
            }
            let content = Source::Inline(format!("{}\n", body.join("\n").trim()));
            match tag.as_str() {
                "ca" => config.ca = Some(content),
                "cert" => config.cert = Some(content),
                "key" => config.key = Some(content),
                "tls-auth" => config.tls_auth = Some(content),
                "tls-crypt" => config.tls_crypt = Some(content),
                "tls-crypt-v2" => config.tls_crypt_v2 = Some(content),
                "pkcs12" | "secret" => {
                    return Err(OvpnError::Unsupported {
                        line: line_no,
                        directive: tag,
                    });
                }
                _ => config.ignored.push(format!("<{tag}>")),
            }
            continue;
        }

        let words = split_words(line);
        let Some((first, args)) = words.split_first() else {
            continue;
        };
        let directive = first.trim_start_matches("--").to_ascii_lowercase();
        let arg = |n: usize| args.get(n).map(String::as_str);
        let required = |n: usize| {
            arg(n).ok_or_else(|| OvpnError::MissingValue {
                line: line_no,
                directive: directive.clone(),
            })
        };
        let invalid = |value: &str| OvpnError::InvalidValue {
            line: line_no,
            directive: directive.clone(),
            value: value.to_owned(),
        };
        let number = |n: usize| -> Result<u32, OvpnError> {
            let value = required(n)?;
            value.parse().map_err(|_| invalid(value))
        };
        let port = |value: &str| value.parse::<u16>().map_err(|_| invalid(value));

        match directive.as_str() {
            "remote" => {
                let host = required(0)?.to_owned();
                let port = arg(1).map(port).transpose()?;
                let proto = match arg(2) {
                    Some(value) => Some(Proto::parse(value).ok_or_else(|| invalid(value))?),
                    None => None,
                };
                config.remotes.push(Remote { host, port, proto });
            }
            "port" | "rport" => config.port = Some(port(required(0)?)?),
            "proto" => {
                let value = required(0)?;
                config.proto = Proto::parse(value).ok_or_else(|| invalid(value))?;
            }
            "dev" | "dev-type" => config.tap = required(0)?.to_ascii_lowercase().starts_with("tap"),
            "ca" => config.ca = Some(Source::File(required(0)?.to_owned())),
            "cert" => config.cert = Some(Source::File(required(0)?.to_owned())),
            "key" => config.key = Some(Source::File(required(0)?.to_owned())),
            "tls-auth" => {
                // `tls-auth [inline] 1`: the key may be in a <tls-auth> block.
                let file = required(0)?;
                if file != "[inline]" {
                    config.tls_auth = Some(Source::File(file.to_owned()));
                }
                if let Some(direction) = arg(1) {
                    config.key_direction =
                        Some(key_direction(direction).ok_or_else(|| invalid(direction))?);
                }
            }
            "key-direction" => {
                let value = required(0)?;
                config.key_direction = Some(key_direction(value).ok_or_else(|| invalid(value))?);
            }
            "tls-crypt" => {
                let file = required(0)?;
                if file != "[inline]" {
                    config.tls_crypt = Some(Source::File(file.to_owned()));
                }
            }
            "tls-crypt-v2" => {
                let file = required(0)?;
                if file != "[inline]" {
                    config.tls_crypt_v2 = Some(Source::File(file.to_owned()));
                }
            }
            // The optional file holds the password in clear: never read.
            "auth-user-pass" => config.user_pass = true,
            "cipher" => config.cipher = Some(required(0)?.to_owned()),
            "data-ciphers" | "ncp-ciphers" => config.data_ciphers = Some(required(0)?.to_owned()),
            "auth" => config.auth = Some(required(0)?.to_owned()),
            "remote-cert-tls" => config.remote_cert_tls = Some(required(0)?.to_owned()),
            "verify-x509-name" => {
                let name = required(0)?.to_owned();
                let kind = arg(1).unwrap_or("subject").to_owned();
                config.verify_x509_name = Some((name, kind));
            }
            "tls-version-min" => config.tls_version_min = Some(required(0)?.to_owned()),
            "comp-lzo" => config.comp_lzo = Some(arg(0).unwrap_or("yes").to_owned()),
            "compress" => config.compress = Some(arg(0).unwrap_or("yes").to_owned()),
            "redirect-gateway" => config.redirect_gateway = true,
            "tun-mtu" => config.tun_mtu = Some(number(0)?),
            "mssfix" => config.mssfix = arg(0).map(|_| number(0)).transpose()?,
            "reneg-sec" => config.reneg_sec = Some(number(0)?),
            "ping" => config.ping = Some(number(0)?),
            "ping-restart" => config.ping_restart = Some(number(0)?),
            "keepalive" => {
                config.ping = Some(number(0)?);
                config.ping_restart = Some(number(1)?);
            }
            "remote-random" => config.remote_random = true,
            "pkcs12" | "secret" => {
                return Err(OvpnError::Unsupported {
                    line: line_no,
                    directive,
                });
            }
            known if HARMLESS.contains(&known) => {}
            _ => config.ignored.push(directive),
        }
    }
    config.ignored.sort();
    config.ignored.dedup();
    Ok(config)
}

/// Words of a line; double quotes group words with spaces (`"My CA.crt"`).
fn split_words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for c in line.chars() {
        match c {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !current.is_empty() {
                    words.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

/// `0` or `1`, the two directions of a `tls-auth` key.
pub fn key_direction(value: &str) -> Option<u8> {
    match value {
        "0" => Some(0),
        "1" => Some(1),
        _ => None,
    }
}

impl OvpnConfig {
    /// Reads the certificate and key files the profile refers to, by file
    /// name, through `read` (selected files, then the profile's folder).
    pub fn resolve_files(
        &mut self,
        read: impl Fn(&str) -> Option<String>,
    ) -> Result<(), OvpnError> {
        for source in [
            &mut self.ca,
            &mut self.cert,
            &mut self.key,
            &mut self.tls_auth,
            &mut self.tls_crypt,
            &mut self.tls_crypt_v2,
        ]
        .into_iter()
        .flatten()
        {
            if let Source::File(name) = source {
                let text = read(name).ok_or_else(|| OvpnError::MissingFile(name.clone()))?;
                *source = Source::Inline(text);
            }
        }
        Ok(())
    }

    /// Base names of the files the profile refers to (`ca.crt`…).
    pub fn file_names(&self) -> Vec<String> {
        [
            &self.ca,
            &self.cert,
            &self.key,
            &self.tls_auth,
            &self.tls_crypt,
            &self.tls_crypt_v2,
        ]
        .into_iter()
        .flatten()
        .filter_map(|source| match source {
            Source::File(name) => Path::new(name)
                .file_name()
                .map(|base| base.to_string_lossy().into_owned()),
            Source::Inline(_) => None,
        })
        .collect()
    }

    /// Checks that the profile can connect: a server, the server's
    /// certificate, and a way to log in.
    pub fn check(&self) -> Result<(), OvpnError> {
        if self.remotes.is_empty() {
            return Err(OvpnError::NoRemote);
        }
        if self.ca.is_none() {
            return Err(OvpnError::NoCa);
        }
        if !self.user_pass && (self.cert.is_none() || self.key.is_none()) {
            return Err(OvpnError::NoCredentials);
        }
        Ok(())
    }

    /// The first server as `host:port`, for display and provider detection.
    pub fn endpoint(&self) -> Option<String> {
        let remote = self.remotes.first()?;
        Some(format!("{}:{}", remote.host, self.remote_port(remote)))
    }

    /// Port of a `remote` line, else the profile's, else OpenVPN's default.
    pub fn remote_port(&self, remote: &Remote) -> u16 {
        remote.port.or(self.port).unwrap_or(DEFAULT_PORT)
    }

    /// True when the client key is protected by a passphrase.
    pub fn key_encrypted(&self) -> bool {
        self.key
            .as_ref()
            .and_then(Source::inline)
            .is_some_and(|key| key.contains("ENCRYPTED"))
    }

    /// Writes the profile back as an `.ovpn` file, everything inline, so it
    /// works on its own (export, phone apps). The password is never written.
    pub fn to_ovpn(&self) -> String {
        let mut out = vec![
            "client".to_owned(),
            format!("dev {}", if self.tap { "tap" } else { "tun" }),
            format!("proto {}", self.proto.name()),
        ];
        if let Some(port) = self.port {
            out.push(format!("port {port}"));
        }
        for remote in &self.remotes {
            let mut line = format!("remote {} {}", remote.host, self.remote_port(remote));
            if let Some(proto) = remote.proto {
                line.push(' ');
                line.push_str(proto.name());
            }
            out.push(line);
        }
        if self.remote_random {
            out.push("remote-random".to_owned());
        }
        out.extend(
            [
                "resolv-retry infinite",
                "nobind",
                "persist-key",
                "persist-tun",
            ]
            .map(str::to_owned),
        );
        if self.user_pass {
            out.push("auth-user-pass".to_owned());
        }
        let mut option = |name: &str, value: &Option<String>| {
            if let Some(value) = value {
                out.push(format!("{name} {value}"));
            }
        };
        option("remote-cert-tls", &self.remote_cert_tls);
        option("cipher", &self.cipher);
        option("data-ciphers", &self.data_ciphers);
        option("auth", &self.auth);
        option("tls-version-min", &self.tls_version_min);
        option("comp-lzo", &self.comp_lzo);
        option("compress", &self.compress);
        let numbers = [
            ("tun-mtu", self.tun_mtu),
            ("mssfix", self.mssfix),
            ("reneg-sec", self.reneg_sec),
            ("ping", self.ping),
            ("ping-restart", self.ping_restart),
        ];
        for (name, value) in numbers {
            if let Some(value) = value {
                out.push(format!("{name} {value}"));
            }
        }
        if let Some((name, kind)) = &self.verify_x509_name {
            out.push(format!("verify-x509-name \"{name}\" {kind}"));
        }
        if self.redirect_gateway {
            out.push("redirect-gateway def1".to_owned());
        }
        if let Some(direction) = self.key_direction.filter(|_| self.tls_auth.is_some()) {
            out.push(format!("key-direction {direction}"));
        }
        let blocks = [
            ("ca", &self.ca),
            ("cert", &self.cert),
            ("key", &self.key),
            ("tls-auth", &self.tls_auth),
            ("tls-crypt", &self.tls_crypt),
            ("tls-crypt-v2", &self.tls_crypt_v2),
        ];
        for (tag, source) in blocks {
            if let Some(text) = source.as_ref().and_then(Source::inline) {
                out.push(format!("<{tag}>\n{}</{tag}>", ensure_newline(text)));
            }
        }
        out.join("\n") + "\n"
    }
}

/// Text ending with a line break, as PEM files do.
pub fn ensure_newline(text: &str) -> String {
    if text.ends_with('\n') {
        text.to_owned()
    } else {
        format!("{text}\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROVIDER: &str = "client
dev tun
proto udp
remote vpn.example.com 1194
remote 203.0.113.5 443 tcp
remote-random
resolv-retry infinite
nobind
persist-key
persist-tun
auth-user-pass
remote-cert-tls server
cipher AES-256-GCM
auth SHA512
verb 3
script-security 2
up /etc/openvpn/update-resolv-conf
key-direction 1
<ca>
-----BEGIN CERTIFICATE-----
MIIB
-----END CERTIFICATE-----
</ca>
<tls-auth>
-----BEGIN OpenVPN Static key V1-----
abcd
-----END OpenVPN Static key V1-----
</tls-auth>
";

    #[test]
    fn provider_profile_is_understood() {
        let config = parse(PROVIDER).expect("valid profile");
        assert_eq!(config.remotes.len(), 2);
        assert_eq!(config.remotes[1].proto, Some(Proto::Tcp));
        assert!(config.user_pass && config.remote_random);
        assert_eq!(config.key_direction, Some(1));
        assert_eq!(config.cipher.as_deref(), Some("AES-256-GCM"));
        assert_eq!(config.ignored, vec!["script-security", "up"]);
        assert!(config.check().is_ok());
        assert_eq!(config.endpoint().as_deref(), Some("vpn.example.com:1194"));
    }

    #[test]
    fn files_are_resolved_or_reported() {
        let text = "client\nremote vpn.example.com\nca \"My CA.crt\"\ncert me.crt\nkey me.key\n";
        let mut config = parse(text).expect("valid profile");
        assert_eq!(config.ca, Some(Source::File("My CA.crt".to_owned())));
        let missing = config
            .clone()
            .resolve_files(|name| (name != "me.key").then(|| "PEM".to_owned()));
        assert_eq!(missing, Err(OvpnError::MissingFile("me.key".to_owned())));
        config
            .resolve_files(|_| Some("PEM".to_owned()))
            .expect("all found");
        assert!(config.check().is_ok());
    }

    #[test]
    fn incomplete_profiles_are_refused_clearly() {
        assert_eq!(
            parse("client\n").and_then(|c| c.check()),
            Err(OvpnError::NoRemote)
        );
        assert_eq!(
            parse("remote a.example\n").and_then(|c| c.check()),
            Err(OvpnError::NoCa)
        );
        assert_eq!(
            parse("remote a.example\n<ca>\nX\n</ca>\n").and_then(|c| c.check()),
            Err(OvpnError::NoCredentials)
        );
        assert_eq!(
            parse("remote a.example\n<ca>\nX\n"),
            Err(OvpnError::UnclosedBlock("ca".to_owned()))
        );
        assert!(matches!(
            parse("pkcs12 bundle.p12\n"),
            Err(OvpnError::Unsupported { line: 1, .. })
        ));
        assert!(matches!(
            parse("remote a.example notaport\n"),
            Err(OvpnError::InvalidValue { line: 1, .. })
        ));
    }

    #[test]
    fn writing_and_reading_back_keeps_the_profile() {
        let config = parse(PROVIDER).expect("valid profile");
        let again = parse(&config.to_ovpn()).expect("written profile parses");
        assert_eq!(again.remotes, config.remotes);
        assert_eq!(again.ca, config.ca);
        assert_eq!(again.tls_auth, config.tls_auth);
        assert_eq!(again.key_direction, config.key_direction);
        assert!(again.ignored.is_empty());
    }
}
