# Architecture

Tunnelbird is a Cargo workspace with two crates. It holds no privilege and
stores no secret itself.

- `crates/core` (`tunnelbird-core`): the logic every system shares, pure and
  unit-tested: the `.conf` and `.ovpn` parsers, providers, trusted networks,
  server health, versions. No GTK, no D-Bus, no file system: the Windows and
  macOS apps planned in `docs/plans/later.md` will use it as it is.
- the root crate (`tunnelbird`): the Linux app, GTK/libadwaita and
  NetworkManager. `main.rs` imports the core modules at the crate root, so
  `crate::config` and the like read the same in both crates.

```
 .conf / .ovpn ──► profile.rs ──► nm.rs ──D-Bus──► NetworkManager ──► kernel WireGuard
                  (config.rs,     (convert,          (polkit,           or OpenVPN plugin
                   ovpn.rs)        activate)          routes, DNS)       (tunnel interface)
                                    │
 window.rs ◄── signals ─────────────┘
 (GTK UI)  ◄── traffic.rs ◄── /sys/class/net/<iface>/statistics
```

## Modules

| File | Role | Depends on |
|---|---|---|
| `src/main.rs` | Entry point: gettext, app actions, panic logger | everything |
| `crates/core/src/config.rs` | Parses `.conf` files into `WgConfig`, leniently (see below). Pure logic, unit-tested | — |
| `crates/core/src/provider.rs` | Recognises well-known VPN providers from server, DNS and profile name. Unit-tested | — |
| `crates/core/src/profile.rs` | A tunnel file of either kind, recognised by its content | `config`, `ovpn` |
| `crates/core/src/ovpn.rs` | OpenVPN `.ovpn` parser and writer. Unit-tested | — |
| `src/openvpn.rs` | NetworkManager's OpenVPN plugin settings, certificate files, plugin detection and install command. Unit-tested | `ovpn` |
| `src/nm.rs` | NetworkManager D-Bus client: list, import, activate, deactivate, delete, watch changes (WireGuard and OpenVPN) | `config`, `ovpn`, `profile` |
| `src/geo.rs` | Public IP and location from ifconfig.co (libsoup), flag emoji, localized country names from `iso-codes` | — |
| `src/settings.rs` | Preferences file (`~/.config/tunnelbird/settings.ini`): style, language, background mode, trusted networks, update checks | — |
| `src/preferences.rs` | Preferences dialog; restarts the app to apply a new language | `settings` |
| `src/tray.rs` | Top bar icon (`org.kde.StatusNotifierItem`) and its menu (`com.canonical.dbusmenu`) on the session bus | — |
| `crates/core/src/health.rs` | Tells a working tunnel from a silent server, from packet counters. Unit-tested | — |
| `src/editor.rs` | Preview before import and editing; validates through `config::parse` | `config`, `share` |
| `src/share.rs` | Export as `.conf` (mode 0600) and QR code (`qrcode` crate, drawn with Cairo) | `config` |
| `crates/core/src/trust.rs` | Trusted networks rule: when joining which network connects which tunnel. Unit-tested | — |
| `src/traffic.rs` | Reads interface byte counters, draws the live graph with Cairo | — |
| `crates/core/src/version.rs` | Version comparison, daily check timing, release notes from the metainfo. Unit-tested | — |
| `src/update.rs` | How the app was installed, new version checks (`latest.json`, Flatpak portal), restart after an update. Unit-tested | `geo`, `portal`, `version` |
| `src/demo.rs` | Demo mode (`TUNNELBIRD_DEMO`): made-up tunnels and traffic for store screenshots, isolated settings | `config`, `settings` |
| `src/window.rs` | Builds the window, reacts to user actions and NetworkManager signals | all of the above |

## Key decisions

**NetworkManager instead of `wg-quick`.** Bringing up an interface needs root.
Calling `wg-quick` would mean `sudo` prompts or a privileged helper to secure.
NetworkManager already runs as root, checks permissions with polkit, handles
DNS through systemd-resolved, and is present on almost every desktop distro.

**Own parser instead of `nmcli connection import`.** NetworkManager has no
D-Bus method to import a `.conf`: `nmcli` parses it on the client side. Doing
the same in `config.rs` keeps Tunnelbird working inside a Flatpak sandbox, where
`nmcli` is not available, and gives precise error messages with line numbers.

**Lenient parsing.** Provider files differ in small ways: byte order mark,
Windows line endings, `;` comments, key spelling (`allowed_ips`), quoted values,
space-separated lists, host bits in `AllowedIPs`, IPv6 endpoints in brackets.
`config.rs` accepts all of these and stays strict on keys, addresses and the
endpoint. Files of another kind (AmneziaWG obfuscation, ZIP bundles) get a
dedicated message. Long file names become valid, distinct interface
names (readable prefix + short hash), and `nm::import` adds a suffix if a
profile already uses the same interface.

**OpenVPN through NetworkManager's plugin.** `profile.rs` recognises a file by
its content and sends `.ovpn` profiles to `ovpn.rs`, which keeps what providers
use (servers, certificates, encryption, `auth-user-pass`) and lists the rest as
ignored. NetworkManager has no D-Bus import for `.ovpn` either (its importer is
in the plugin's C library), so the profile becomes the plugin's `vpn.data`
string map, in the layout `nm-connection-editor` writes. The plugin wants
certificate *files*: they are written to `~/.cert/nm-openvpn/tunnelbird-<uuid>/`
(mode 0600), the folder `nm-connection-editor` uses, because SELinux on Fedora
lets OpenVPN read certificates there and nowhere else in the home folder (the
Snap uses its common data folder instead). A password typed in the editor is
stored by NetworkManager like WireGuard keys; left empty, the desktop asks for
it at the first connection and its keyring keeps it. An OpenVPN interface is
only known while connected: `nm.rs` finds the device carrying the active
connection's IP configuration. If the plugin is missing, the activation error
is recognised and `ovpn::plugin_install_command` gives the command for the
distribution (read from `os-release`, also from inside a Flatpak or a Snap).

**Provider detection.** Only public hints from the providers' own files:
server domain or address range, internal DNS server, then words of the profile
name. Names only, never logos. A self-hosted server shows no label.

**Profile layout.** `nm::to_nm_settings` produces the same settings as
`nmcli connection import type wireguard`. When `DNS` is set and the tunnel
carries all traffic (`AllowedIPs` contains `/0`), the VPN DNS servers become
exclusive (`dns-search ~`, `dns-priority -50`), like `wg-quick`.

**Secrets.** Private and preshared keys are sent to NetworkManager with flags
`0`: NetworkManager stores them in `/etc/NetworkManager/system-connections`,
readable by root only. Tunnelbird keeps no copy.

**State updates.** Tunnelbird subscribes to NetworkManager signals (profile
added/removed/updated, active connection state, `ActiveConnections`). Bursts of
signals are grouped into one refresh (150 ms debounce). If signals cannot be
watched, it falls back to polling every 3 seconds.

**Traffic.** Counters come from `/sys/class/net/<iface>/statistics`, readable
without privilege. The WireGuard interface is created on connection, so the
counters are also the totals since the connection started. The graph keeps 60
samples (one per second).

**One tunnel at a time.** Two active tunnels would fight over routes and DNS.
When the user connects a tunnel, `Ui::toggle` first deactivates every other
active WireGuard tunnel (even one started outside Tunnelbird), then activates
the new one. If a deactivation fails, the new connection is not attempted.

**Public IP and location.** Only for full tunnels: a partial tunnel does not
change the public IP, so the card shows the VPN address and says so instead.
The lookup waits 1 s for routes and DNS to settle, retries once after 3 s, and
is discarded if the user switched tunnel meanwhile (`GeoState` is keyed by
tunnel UUID). Country names come from the system `iso-codes` database and its
`iso_3166-1` translations, with the service's English name as fallback.

**Top bar icon.** Implemented on the zbus already used for NetworkManager,
instead of a tray crate that would bring a second zbus and async runtime. zbus
serves the two interfaces from its own thread: they read a `TrayModel` behind a
mutex, filled by the window, and send clicks back to the GTK thread through a
channel. When no tray host answers (GNOME without the AppIndicator extension),
the app keeps the usual "closing the window quits" behaviour. Known limit: if
the tray host restarts (GNOME Shell reload), the icon reappears only after
restarting Tunnelbird.

**Background mode.** With the top bar icon available and "Run in Background"
on, closing the window hides it and an `ApplicationHoldGuard` keeps the app
alive; a notification says so once per session. Errors from actions started in
the menu are also sent as notifications, since the window may be hidden.

**Language switch.** The chosen language is applied through `LANGUAGE` before
gettext starts. Labels already on screen cannot change, so Preferences offers a
restart: `main` re-executes the binary after the app has released its D-Bus name.

**Silent server.** The interface is "connected" as soon as it exists, even if
the server never answers. `health.rs` watches packet counters: packets sent
during 15 s with none received means the server is not responding (status in
red on the card, top bar text, one notification). The kernel does not count
keepalives, so an idle tunnel stays "unknown", never "broken".

**Unexpected drops.** A tunnel that goes from connected to disconnected
without this app asking (tracked in `user_stopped`) raises a high-priority
notification: the traffic is no longer protected.

**Trusted networks.** On every NetworkManager change, `Ui::check_network`
looks up the physical connection (Wi-Fi, cable, mobile), ignoring tunnels,
which may hold the default route. The rule applies only when the network
profile changes ("joining"), so a manual disconnection is respected until the
next network; nothing is ever disconnected automatically. Settings live in the
preferences file; the Wi-Fi list is written in the GLib list syntax (no
`set_string_list` binding in glib-rs). Start at login writes a freedesktop
autostart entry launching `--background` (window hidden, top bar icon only;
the window shows anyway if no top bar host is available).

**Strict Wi-Fi check (option).** A Wi-Fi name can be copied ("evil twin"),
so the strict check also requires the access point hardware address (BSSID,
read from the device's active access point). Trusting the current network pins
its BSSID; a network trusted from the saved list pins the first access point
it is joined through. Unknown access points of a trusted name count as
untrusted; Preferences then offer to trust that access point (new router,
repeater). Pins are stored as `BSSID SSID` entries.

## Updates

Tunnelbird never replaces its own files (`src/update.rs`). It works out how it
was installed and says how the new version arrives: the Flatpak portal
(`CreateUpdateMonitor`, then “Update”), snapd by itself, the system updates for
the website's APT/DNF repository (an explanation with the command), the website
download page for a package installed by hand, nothing for a source build.
The last two read `https://tunnelbird.ferhatsn.fr/latest.json` at most once a
day (switch in Preferences). A version installed while the app runs (the
binary on disk is “deleted”, the snap revision changed, the portal's local
commit differs) shows “Restart”: the restart re-executes the new binary, or
asks the Flatpak portal to spawn the latest version. The first start of a new
version offers “What's New”, read from the metainfo's newest `<release>`.

Testing from a source build: `TUNNELBIRD_INSTALL=package` (or `apt`, `dnf`,
`flatpak`, `snap`) pretends another install method, and
`TUNNELBIRD_UPDATE_URL` points the check to another `latest.json`.

## Screenshots

`make screenshots` runs the app in demo mode (made-up tunnels with
documentation addresses, settings in a temporary folder, no NetworkManager
call) inside an invisible GNOME Shell started with its own D-Bus session,
runtime folder and settings (GNOME defaults, no distribution overrides). GNOME
Shell captures the window with its frame and shadow, in English and French.

## Error handling

- No `unwrap`/`expect` on runtime data: every failure is a `Result`.
- `Ui::report_error` logs with GLib (`journalctl`) **and** shows a toast.
- Activation failures that happen after NetworkManager accepted the request
  are detected by `Ui::detect_failed_connections`.
- A panic hook logs any crash to the journal before the default report.

## Internationalisation

English is the source language. Strings are wrapped in `gettext()`, and
placeholders use named braces (`{name}`) so translators can reorder them.
`build.rs` compiles `po/*.po` with `msgfmt`. See [TRANSLATING.md](TRANSLATING.md).
