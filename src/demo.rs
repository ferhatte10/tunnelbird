// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

//! Demo mode, for store screenshots and the website: `TUNNELBIRD_DEMO=en`
//! (or `fr`) shows made-up tunnels, traffic and location instead of
//! NetworkManager's. Adding `TUNNELBIRD_SCREENSHOTS=<folder>` takes the
//! screenshots by itself, then quits (see `make screenshots`).
//!
//! Nothing real is touched: no NetworkManager call, no top bar icon, no network
//! request (unless `TUNNELBIRD_UPDATE_URL` asks to test update notices), and
//! the settings live in a throwaway folder. Addresses come from
//! the ranges reserved for documentation (RFC 5737, `example.com`), Wi-Fi
//! hardware addresses are locally administered ones.

use std::path::Path;
use std::time::Instant;

use adw::prelude::*;
use base64::Engine;
use gettextrs::gettext;
use gtk::{gio, glib};

use crate::config::{self, WgConfig};
use crate::geo::Location;
use crate::logs::{Events, Severity, Source};
use crate::nm::{Tunnel, TunnelState};
use crate::profile::Kind;
use crate::settings::{Settings, Style};
use crate::traffic::{Counters, Sample};
use crate::trust::{Network, NetworkKind, Pin, Policy};

/// Screenshots are rendered at twice the window size, for HiDPI screens.
const SCALE: f32 = 2.0;
const CONNECTED: &str = "00000000-0000-4000-8000-000000000001";
/// Samples already in the graph when the demo starts.
pub const HISTORY: u32 = 60;

/// True when the demo mode was asked for.
pub fn enabled() -> bool {
    std::env::var_os("TUNNELBIRD_DEMO").is_some()
}

/// Points the settings to a fresh temporary folder filled with demo
/// preferences, so the real ones are neither shown nor changed.
/// Must run first in `main`, before any thread exists.
pub fn prepare() {
    let dir = settings_dir();
    // SAFETY: called at the very start of `main`, before any other thread exists.
    unsafe { std::env::set_var("XDG_CONFIG_HOME", &dir) };
    let language = match std::env::var("TUNNELBIRD_DEMO").as_deref() {
        Ok("fr") => "fr",
        _ => "en",
    };
    let settings = Settings {
        style: Style::System,
        language: language.to_owned(),
        last_version: env!("CARGO_PKG_VERSION").to_owned(),
        trust: Policy {
            enabled: true,
            tunnel: CONNECTED.to_owned(),
            trusted_ssids: vec!["Home".to_owned(), "Office".to_owned()],
            trust_wired: true,
            strict: true,
            pins: vec![
                Pin {
                    bssid: "02:00:00:00:00:01".to_owned(),
                    ssid: "Home".to_owned(),
                },
                Pin {
                    bssid: "02:00:00:00:00:02".to_owned(),
                    ssid: "Home".to_owned(),
                },
            ],
        },
        ..Settings::default()
    };
    if let Err(err) = settings.save() {
        glib::g_warning!(crate::LOG_DOMAIN, "Cannot write the demo settings: {err}");
    }
}

/// Removes the demo settings folder, once the app has quit.
pub fn cleanup() {
    let dir = settings_dir();
    if let Err(err) = std::fs::remove_dir_all(&dir) {
        glib::g_warning!(crate::LOG_DOMAIN, "Cannot remove {}: {err}", dir.display());
    }
}

fn settings_dir() -> std::path::PathBuf {
    glib::tmp_dir().join(format!("tunnelbird-demo-{}", std::process::id()))
}

/// `(name, kind, endpoint, addresses, all traffic?)` of each demo tunnel.
const TUNNELS: [(&str, Kind, &str, &str, bool); 4] = [
    (
        "Amsterdam",
        Kind::WireGuard,
        "203.0.113.10:51820",
        "10.66.0.2/32, fd00:66::2/128",
        true,
    ),
    (
        "Home Server",
        Kind::WireGuard,
        "vpn.example.com:51820",
        "10.10.0.2/32",
        false,
    ),
    ("Office", Kind::OpenVpn, "198.51.100.7:1194", "", false),
    (
        "Tokyo",
        Kind::WireGuard,
        "203.0.113.80:51820",
        "10.67.0.9/32",
        true,
    ),
];

pub fn tunnels() -> Vec<Tunnel> {
    TUNNELS
        .iter()
        .enumerate()
        .map(|(index, (name, kind, endpoint, addresses, full))| {
            let connected = index == 0;
            Tunnel {
                path: format!("/demo/{index}").try_into().unwrap_or_default(),
                uuid: format!("00000000-0000-4000-8000-00000000000{}", index + 1),
                name: (*name).to_owned(),
                kind: *kind,
                interface: config::interface_name(name),
                endpoint: Some((*endpoint).to_owned()),
                addresses: addresses
                    .split(", ")
                    .filter(|a| !a.is_empty())
                    .map(str::to_owned)
                    .collect(),
                full_tunnel: *full,
                carries_ipv6: *full,
                provider: None,
                state: if connected {
                    TunnelState::Connected
                } else {
                    TunnelState::Disconnected
                },
                active_path: connected.then(|| "/demo/active".try_into().unwrap_or_default()),
            }
        })
        .collect()
}

/// What the logs show for the connected tunnel: a first attempt that failed,
/// then the connection. Each line is (minutes ago, source, severity, text).
pub fn fill_logs(events: &mut Events) {
    use Severity::{Error, Info, Warning};
    use Source::{App, NetworkManager as Nm};
    let now = glib::real_time();
    let failed = gettext("Could not connect: {reason}")
        .replace("{reason}", &gettext("the server did not answer in time"));
    let lines = [
        (42, App, Info, gettext("Connecting…")),
        (
            41,
            Nm,
            Warning,
            "device (amsterdam): no answer from 198.51.100.24:51820".to_owned(),
        ),
        (41, App, Error, failed),
        (12, App, Info, gettext("Connecting…")),
        (
            12,
            Nm,
            Info,
            "device (amsterdam): state change: config -> ip-config (reason 'none')".to_owned(),
        ),
        (
            12,
            Nm,
            Info,
            "policy: set 'Amsterdam' (amsterdam) as default for IPv4 routing and DNS".to_owned(),
        ),
        (
            12,
            Nm,
            Info,
            "device (amsterdam): Activation: successful, device activated.".to_owned(),
        ),
        (12, App, Info, gettext("Connected")),
    ];
    for (minutes, source, severity, text) in lines {
        let time = now - i64::from(minutes) * 60_000_000;
        events.push_at(CONNECTED, time, source, severity, text);
    }
}

pub fn location() -> Location {
    Location {
        ip: "203.0.113.42".to_owned(),
        country: Some("Netherlands".to_owned()),
        country_iso: Some("NL".to_owned()),
        city: Some("Amsterdam".to_owned()),
        asn_org: None,
    }
}

/// The Wi-Fi the computer is on, and the ones saved on it.
pub fn network() -> Network {
    Network {
        uuid: "demo-wifi".to_owned(),
        kind: NetworkKind::Wifi,
        ssid: Some("Home".to_owned()),
        bssid: Some("02:00:00:00:00:01".to_owned()),
    }
}

pub fn saved_wifi() -> Vec<String> {
    ["Home", "Office", "Station Wi-Fi", "Hotel Guest"]
        .map(str::to_owned)
        .to_vec()
}

/// A complete tunnel file, for the import preview and the QR code. The keys
/// are readable text, not real keys.
pub fn config(full: bool) -> WgConfig {
    let key = |text: &str| base64::engine::general_purpose::STANDARD.encode(text);
    let (address, endpoint, allowed) = if full {
        (
            "10.66.0.2/32, fd00:66::2/128",
            "203.0.113.10:51820",
            "0.0.0.0/0, ::/0",
        )
    } else {
        (
            "10.10.0.2/32",
            "vpn.example.com:51820",
            "10.10.0.0/24, 192.168.50.0/24",
        )
    };
    let text = format!(
        "[Interface]\nPrivateKey = {}\nAddress = {address}\nDNS = 10.10.0.1\n\n\
         [Peer]\nPublicKey = {}\nAllowedIPs = {allowed}\nEndpoint = {endpoint}\nPersistentKeepalive = 25\n",
        key("Tunnelbird demo private key 0001"),
        key("Tunnelbird demo public key 00001"),
    );
    config::parse(&text).unwrap_or_default()
}

/// An OpenVPN profile as providers ship it, for the import preview. The
/// certificates are placeholders, not real ones.
pub fn ovpn_config() -> crate::ovpn::OvpnConfig {
    let text = "client\ndev tun\nproto udp\nremote vpn.example.com 1194\nremote 198.51.100.7 443 tcp\n\
        remote-random\nnobind\npersist-key\npersist-tun\nauth-user-pass\nremote-cert-tls server\n\
        data-ciphers AES-256-GCM:CHACHA20-POLY1305\nauth SHA256\nredirect-gateway def1\n\
        <ca>\n-----BEGIN CERTIFICATE-----\nDEMO\n-----END CERTIFICATE-----\n</ca>\n\
        <tls-crypt>\n-----BEGIN OpenVPN Static key V1-----\nDEMO\n-----END OpenVPN Static key V1-----\n</tls-crypt>\n";
    let mut config = crate::ovpn::parse(text).unwrap_or_default();
    config.username = "alex".to_owned();
    config
}

/// Traffic of a busy connection: a few megabytes per second with bursts.
pub fn sample(tick: u32) -> Sample {
    let t = f64::from(tick);
    let wave = (t / 5.0).sin() * 0.5 + (t / 2.3).sin() * 0.3 + (t * 1.7).sin() * 0.15;
    // Stays under 5 MB/s, so the graph keeps one scale.
    let burst = if tick % 23 < 4 { 2.0e6 } else { 0.0 };
    Sample {
        rx: 1.9e6 + wave * 1.1e6 + burst,
        tx: 2.4e5 + (t / 3.1).cos() * 1.2e5 + burst * 0.08,
    }
}

/// Interface counters growing with [`sample`] over real time (timers can run
/// late), as if connected for a while; packets too, so the server shows as
/// answering.
pub fn counters() -> Counters {
    thread_local! {
        static START: Instant = Instant::now();
    }
    let elapsed = START.with(Instant::elapsed).as_secs_f64();
    let whole = elapsed as u32;
    let (mut rx, mut tx) = (1.24e9, 1.86e8);
    for tick in HISTORY..HISTORY + whole {
        let sample = sample(tick);
        rx += sample.rx;
        tx += sample.tx;
    }
    let current = sample(HISTORY + whole);
    rx += current.rx * elapsed.fract();
    tx += current.tx * elapsed.fract();
    Counters {
        rx_bytes: rx as u64,
        tx_bytes: tx as u64,
        rx_packets: (elapsed * 900.0) as u64,
        tx_packets: (elapsed * 400.0) as u64,
    }
}

/// Saves the window as a screenshot. `build-aux/screenshots.sh` passes the
/// shell's own window capture (title bar, rounded corners and shadow, as
/// Flathub asks) in `TUNNELBIRD_CAPTURE_COMMAND`; without it, GTK draws the
/// window contents only, which is enough for a quick look.
pub async fn capture(widget: &impl IsA<gtk::Widget>, path: &Path) -> Result<(), String> {
    let Some(command) = std::env::var_os("TUNNELBIRD_CAPTURE_COMMAND") else {
        return render(widget, path);
    };
    // Never mistake an older file for this capture.
    let _ = std::fs::remove_file(path);
    let process = gio::Subprocess::newv(
        &[command.as_os_str(), path.as_os_str()],
        gio::SubprocessFlags::NONE,
    )
    .map_err(|err| err.to_string())?;
    process
        .wait_check_future()
        .await
        .map_err(|err| err.to_string())?;
    // The shell writes the file asynchronously.
    glib::timeout_future(std::time::Duration::from_millis(500)).await;
    if path.exists() {
        Ok(())
    } else {
        Err("the shell did not write the screenshot".to_owned())
    }
}

fn render(widget: &impl IsA<gtk::Widget>, path: &Path) -> Result<(), String> {
    let widget = widget.as_ref();
    let paintable = gtk::WidgetPaintable::new(Some(widget));
    let snapshot = gtk::Snapshot::new();
    snapshot.scale(SCALE, SCALE);
    paintable.snapshot(
        &snapshot,
        f64::from(widget.width()),
        f64::from(widget.height()),
    );
    let node = snapshot.to_node().ok_or("nothing to capture")?;
    let renderer = widget
        .native()
        .and_then(|native| native.renderer())
        .ok_or("the window is not shown")?;
    renderer
        .render_texture(&node, None)
        .save_to_png(path)
        .map_err(|err| err.to_string())
}
