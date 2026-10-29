// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

//! The Linux side of OpenVPN profiles (parsed by `tunnelbird_core::ovpn`):
//! NetworkManager runs OpenVPN through its plugin (`network-manager-openvpn`),
//! so a profile becomes the plugin's settings, the `vpn.data` string map, and
//! its certificates become files the plugin can read ([`store_files`]).
//! Also: is the plugin installed, and which command installs it.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use crate::ovpn::{OvpnConfig, Proto, Remote, Source, ensure_newline, key_direction};

/// NetworkManager's name for its OpenVPN plugin.
pub const SERVICE_TYPE: &str = "org.freedesktop.NetworkManager.openvpn";

/// Paths of the certificate and key files written by [`store_files`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Paths {
    pub ca: Option<String>,
    pub cert: Option<String>,
    pub key: Option<String>,
    pub tls_auth: Option<String>,
    pub tls_crypt: Option<String>,
    pub tls_crypt_v2: Option<String>,
}

/// The plugin's settings (`vpn.data`), in the layout `nm-connection-editor`
/// writes. Secrets (the password) go to `vpn.secrets` separately.
pub fn to_nm_data(config: &OvpnConfig, paths: &Paths) -> BTreeMap<&'static str, String> {
    let mut data = BTreeMap::new();
    let remotes: Vec<String> = config
        .remotes
        .iter()
        .map(|remote| {
            let proto = remote.proto.unwrap_or(config.proto);
            format!(
                "{}:{}:{}",
                remote.host,
                config.remote_port(remote),
                proto.name()
            )
        })
        .collect();
    data.insert("remote", remotes.join(", "));
    if config.proto == Proto::Tcp {
        data.insert("proto-tcp", "yes".to_owned());
    }
    data.insert(
        "dev-type",
        if config.tap { "tap" } else { "tun" }.to_owned(),
    );
    let has_cert = paths.cert.is_some() && paths.key.is_some();
    let connection_type = match (config.user_pass, has_cert) {
        (true, true) => "password-tls",
        (true, false) => "password",
        _ => "tls",
    };
    data.insert("connection-type", connection_type.to_owned());
    if config.user_pass {
        data.insert("username", config.username.clone());
        // 0 = stored by NetworkManager (readable by root only), like WireGuard
        // keys; 1 = kept by the desktop keyring, asked at the first connection.
        let flags = if config.password.is_empty() { "1" } else { "0" };
        data.insert("password-flags", flags.to_owned());
    }
    if has_cert && config.key_encrypted() {
        data.insert("cert-pass-flags", "1".to_owned());
    }
    let mut path = |key: &'static str, value: &Option<String>| {
        if let Some(value) = value {
            data.insert(key, value.clone());
        }
    };
    path("ca", &paths.ca);
    path("cert", &paths.cert);
    path("key", &paths.key);
    path("ta", &paths.tls_auth);
    path("tls-crypt", &paths.tls_crypt);
    path("tls-crypt-v2", &paths.tls_crypt_v2);
    if paths.tls_auth.is_some()
        && let Some(direction) = config.key_direction
    {
        data.insert("ta-dir", direction.to_string());
    }
    let mut text = |key: &'static str, value: &Option<String>| {
        if let Some(value) = value {
            data.insert(key, value.clone());
        }
    };
    text("cipher", &config.cipher);
    text("data-ciphers", &config.data_ciphers);
    text("auth", &config.auth);
    text("remote-cert-tls", &config.remote_cert_tls);
    text("tls-version-min", &config.tls_version_min);
    text("compress", &config.compress);
    if let Some(mode) = &config.comp_lzo {
        // The plugin's `no-by-default` is OpenVPN's `comp-lzo no`.
        let mode = if mode == "no" { "no-by-default" } else { mode };
        data.insert("comp-lzo", mode.to_owned());
    }
    if let Some((name, kind)) = &config.verify_x509_name {
        data.insert("verify-x509-name", format!("{kind}:{name}"));
    }
    let numbers = [
        ("tunnel-mtu", config.tun_mtu),
        ("mssfix", config.mssfix),
        ("reneg-seconds", config.reneg_sec),
        ("ping", config.ping),
        ("ping-restart", config.ping_restart),
    ];
    for (key, value) in numbers {
        if let Some(value) = value {
            data.insert(key, value.to_string());
        }
    }
    if config.remote_random {
        data.insert("remote-random", "yes".to_owned());
    }
    data
}

/// Rebuilds a profile from the plugin's settings; `read` loads the
/// certificate files back (for the editor and the export).
pub fn from_nm_data(
    data: &HashMap<String, String>,
    password: Option<String>,
    read: impl Fn(&str) -> Option<String>,
) -> OvpnConfig {
    let text = |key: &str| data.get(key).filter(|v| !v.is_empty()).cloned();
    let number = |key: &str| text(key).and_then(|v| v.parse::<u32>().ok());
    let file = |key: &str| {
        text(key).map(|path| match read(&path) {
            Some(content) => Source::Inline(content),
            None => Source::File(path),
        })
    };
    let proto = if text("proto-tcp").as_deref() == Some("yes") {
        Proto::Tcp
    } else {
        Proto::Udp
    };
    let remotes = text("remote")
        .unwrap_or_default()
        .split([',', ' '])
        .filter(|item| !item.is_empty())
        .map(|item| {
            let mut parts = item.split(':');
            Remote {
                host: parts.next().unwrap_or_default().to_owned(),
                port: parts.next().and_then(|p| p.parse().ok()),
                proto: parts.next().and_then(Proto::parse),
            }
        })
        .collect();
    let connection_type = text("connection-type").unwrap_or_default();
    OvpnConfig {
        remotes,
        port: text("port").and_then(|v| v.parse().ok()),
        proto,
        tap: text("dev-type").as_deref() == Some("tap"),
        ca: file("ca"),
        cert: file("cert"),
        key: file("key"),
        tls_auth: file("ta"),
        key_direction: text("ta-dir").as_deref().and_then(key_direction),
        tls_crypt: file("tls-crypt"),
        tls_crypt_v2: file("tls-crypt-v2"),
        user_pass: connection_type.starts_with("password"),
        username: text("username").unwrap_or_default(),
        password: password.unwrap_or_default(),
        cipher: text("cipher"),
        data_ciphers: text("data-ciphers"),
        auth: text("auth"),
        remote_cert_tls: text("remote-cert-tls"),
        verify_x509_name: text("verify-x509-name").and_then(|value| {
            let (kind, name) = value.split_once(':')?;
            Some((name.to_owned(), kind.to_owned()))
        }),
        tls_version_min: text("tls-version-min"),
        comp_lzo: text("comp-lzo").map(|mode| {
            if mode == "no-by-default" {
                "no".to_owned()
            } else {
                mode
            }
        }),
        compress: text("compress"),
        redirect_gateway: false,
        tun_mtu: number("tunnel-mtu"),
        mssfix: number("mssfix"),
        reneg_sec: number("reneg-seconds"),
        ping: number("ping"),
        ping_restart: number("ping-restart"),
        remote_random: text("remote-random").as_deref() == Some("yes"),
        ignored: Vec::new(),
    }
}

/// Folder holding a tunnel's certificates and keys.
///
/// NetworkManager's OpenVPN runs as root but under SELinux on Fedora, where it
/// may only read certificates in `~/.cert` (the folder `nm-connection-editor`
/// uses too): elsewhere in the home folder it is denied. In a Snap the home
/// folder's hidden folders are out of reach, so the snap's common data folder
/// is used (Ubuntu, AppArmor only). The Flatpak gets `~/.cert/nm-openvpn`.
pub fn files_dir(uuid: &str) -> PathBuf {
    match std::env::var_os("SNAP_USER_COMMON") {
        Some(common) => Path::new(&common).join("openvpn").join(uuid),
        None => gtk::glib::home_dir()
            .join(".cert")
            .join("nm-openvpn")
            .join(format!("tunnelbird-{uuid}")),
    }
}

/// Writes the inline certificates and keys of `config` for NetworkManager,
/// readable by the user (and root) only.
pub fn store_files(uuid: &str, config: &OvpnConfig) -> std::io::Result<Paths> {
    use std::io::Write;
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};

    let dir = files_dir(uuid);
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&dir)?;
    let write = |name: &str, source: &Option<Source>| -> std::io::Result<Option<String>> {
        let Some(text) = source.as_ref().and_then(Source::inline) else {
            return Ok(None);
        };
        let path = dir.join(name);
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&path)?;
        file.write_all(ensure_newline(text).as_bytes())?;
        Ok(Some(path.to_string_lossy().into_owned()))
    };
    Ok(Paths {
        ca: write("ca.pem", &config.ca)?,
        cert: write("cert.pem", &config.cert)?,
        key: write("key.pem", &config.key)?,
        tls_auth: write("tls-auth.key", &config.tls_auth)?,
        tls_crypt: write("tls-crypt.key", &config.tls_crypt)?,
        tls_crypt_v2: write("tls-crypt-v2.key", &config.tls_crypt_v2)?,
    })
}

/// Removes a deleted tunnel's certificates. A missing folder is fine.
pub fn remove_files(uuid: &str) -> std::io::Result<()> {
    match std::fs::remove_dir_all(files_dir(uuid)) {
        Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(err),
        _ => Ok(()),
    }
}

/// The command that installs NetworkManager's OpenVPN plugin, from the
/// system's `os-release` (`ID`, then `ID_LIKE`). `None` for unknown systems.
pub fn plugin_install_command(os_release: &str) -> Option<&'static str> {
    let field = |name: &str| {
        os_release.lines().find_map(|line| {
            let value = line.strip_prefix(name)?.strip_prefix('=')?;
            Some(value.trim_matches('"').to_ascii_lowercase())
        })
    };
    let ids = [field("ID"), field("ID_LIKE")]
        .into_iter()
        .flatten()
        .flat_map(|value| {
            value
                .split_whitespace()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        });
    for id in ids {
        let command = match id.as_str() {
            "ubuntu" | "debian" | "linuxmint" | "pop" | "elementary" | "zorin" => {
                "sudo apt install network-manager-openvpn-gnome"
            }
            "fedora" | "rhel" | "centos" | "rocky" | "almalinux" => {
                "sudo dnf install NetworkManager-openvpn-gnome"
            }
            "arch" | "manjaro" | "endeavouros" => "sudo pacman -S networkmanager-openvpn",
            "opensuse" | "opensuse-tumbleweed" | "opensuse-leap" | "suse" => {
                "sudo zypper install NetworkManager-openvpn-gnome"
            }
            _ => continue,
        };
        return Some(command);
    }
    None
}

/// The system's `os-release`, also from inside a Flatpak or a Snap.
pub fn host_os_release() -> String {
    [
        "/run/host/os-release",
        "/var/lib/snapd/hostfs/etc/os-release",
        "/etc/os-release",
        "/usr/lib/os-release",
    ]
    .iter()
    .find_map(|path| std::fs::read_to_string(path).ok())
    .unwrap_or_default()
}

/// Whether NetworkManager's OpenVPN plugin is installed, when this can be
/// seen (not from a Flatpak or a Snap: `None`, the connection will tell).
pub fn plugin_installed() -> Option<bool> {
    if crate::portal::in_flatpak() || std::env::var_os("SNAP").is_some() {
        return None;
    }
    let found = [
        "/usr/lib/NetworkManager/VPN/nm-openvpn-service.name",
        "/etc/NetworkManager/VPN/nm-openvpn-service.name",
        "/usr/lib64/NetworkManager/VPN/nm-openvpn-service.name",
    ]
    .iter()
    .any(|path| Path::new(path).exists());
    Some(found)
}

/// True when a NetworkManager error says the OpenVPN plugin is missing.
pub fn is_plugin_missing(message: &str) -> bool {
    (message.contains(SERVICE_TYPE) && message.contains("not installed"))
        || (message.contains("plugin") && message.contains("openvpn") && message.contains("not"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROVIDER: &str = "client
dev tun
proto udp
remote vpn.example.com 1194
remote 203.0.113.5 443 tcp
auth-user-pass
key-direction 1
<ca>
X
</ca>
<tls-auth>
Y
</tls-auth>
";

    #[test]
    fn networkmanager_settings_round_trip() {
        let mut config = crate::ovpn::parse(PROVIDER).expect("valid profile");
        config.username = "alice".to_owned();
        let paths = Paths {
            ca: Some("/certs/ca.pem".to_owned()),
            tls_auth: Some("/certs/tls-auth.key".to_owned()),
            ..Paths::default()
        };
        let data = to_nm_data(&config, &paths);
        assert_eq!(
            data["remote"],
            "vpn.example.com:1194:udp, 203.0.113.5:443:tcp"
        );
        assert_eq!(data["connection-type"], "password");
        assert_eq!(data["password-flags"], "1");
        assert_eq!(data["ta-dir"], "1");
        let data: HashMap<String, String> =
            data.into_iter().map(|(k, v)| (k.to_owned(), v)).collect();
        let files = HashMap::from([
            ("/certs/ca.pem", config.ca.clone()),
            ("/certs/tls-auth.key", config.tls_auth.clone()),
        ]);
        let back = from_nm_data(&data, None, |path| {
            files
                .get(path)
                .cloned()
                .flatten()
                .and_then(|s| s.inline().map(str::to_owned))
        });
        assert_eq!(back.remotes[0].port, Some(1194));
        assert_eq!(back.remotes[1].proto, Some(Proto::Tcp));
        assert_eq!(back.username, "alice");
        assert_eq!(back.ca, config.ca);
        assert_eq!(back.key_direction, Some(1));
    }

    #[test]
    fn plugin_command_follows_the_distribution() {
        assert_eq!(
            plugin_install_command("ID=ubuntu\nID_LIKE=debian\n"),
            Some("sudo apt install network-manager-openvpn-gnome")
        );
        assert_eq!(
            plugin_install_command("ID=\"linuxmint\"\n"),
            Some("sudo apt install network-manager-openvpn-gnome")
        );
        assert_eq!(
            plugin_install_command("ID=nobara\nID_LIKE=\"rhel centos fedora\"\n"),
            Some("sudo dnf install NetworkManager-openvpn-gnome")
        );
        assert_eq!(
            plugin_install_command("ID=cachyos\nID_LIKE=arch\n"),
            Some("sudo pacman -S networkmanager-openvpn")
        );
        assert_eq!(plugin_install_command("ID=gentoo\n"), None);
    }
}
