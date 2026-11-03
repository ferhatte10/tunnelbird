# Tunnelbird

A lightweight, modern VPN client for the Linux desktop, with a native macOS
version on its way ([see below](#macos)).
Import the `.conf` (WireGuard®) or `.ovpn` (OpenVPN) file from your VPN
provider or your own server, then connect in one click.

> Tunnelbird is a **client only**: it does not create or manage VPN servers.

## Features

- Import `.conf` and `.ovpn` files (button, drag and drop, or
  `tunnelbird file.conf`), recognised by their content, as written by VPN
  providers, routers and scripts: lenient on formatting, precise errors with
  line numbers, clear messages for obfuscated variants and ZIP bundles
- OpenVPN profiles with inline certificates or separate certificate files
  (select them with the profile), user name and password kept by NetworkManager
  or by the desktop keyring; if the system lacks NetworkManager's OpenVPN
  plugin, Tunnelbird shows the command that installs it for your distribution
- Shows the tunnel type (WireGuard® or OpenVPN) and recognises well-known providers (Mullvad, Proton VPN, Surfshark, NordVPN,
  Cloudflare WARP, IVPN, AzireVPN, AirVPN, Windscribe, PIA) and shows their name
- Connect and disconnect with a switch; only one tunnel at a time (connecting
  another one disconnects the current one)
- Address inside the VPN, public IP address, flag, country and city once connected
- Live traffic graph and totals for the active tunnel
- Warns when the server stops answering while the system still says
  “connected”, and when a tunnel drops on its own
- Blocks IPv6 outside a full tunnel that carries only IPv4, so a website cannot
  see your real address through IPv6
- Preview and edit before importing; edit existing tunnels; export as a `.conf`
  or `.ovpn` file, or a WireGuard® tunnel as a QR code to scan with a phone
- Top bar icon and menu: switch VPN, live traffic with a mini graph, location,
  open the window, open the preferences or quit. Closing the window keeps Tunnelbird running there
- Automatic connection on untrusted networks (café or hotel Wi-Fi), never on
  your trusted Wi-Fi networks, picked from the ones your computer knows; an
  optional strict check also verifies the router hardware against look-alike
  hotspots; optional start at login, hidden in the top bar
- Preferences: light, dark or system style; interface language; background mode
- Clear error messages (invalid file, refused permission, unreachable server)
- Logs of each tunnel, live, with the reason a connection failed or dropped
- English and French, ready for more languages
- Light and dark themes, follows the system style

These are the features of the Linux app. The macOS app has the same ones, in a
menu bar app (see [macOS](#macos)).

## How it works

On Linux, Tunnelbird never runs as root. It hands tunnels to **NetworkManager** over D-Bus,
which creates the tunnel interface, sets routes and DNS, and asks for
permission through polkit when needed. Imported tunnels therefore also appear in
your system network settings and in `nmcli`.

On macOS there is no NetworkManager: a small helper that runs as root makes the
tunnels, and the app only talks to it.

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for details.

## Privacy

When a tunnel carrying all your traffic connects, Tunnelbird asks
[ifconfig.co](https://ifconfig.co) (over HTTPS, through the tunnel) for the
public IP address and its approximate location, to show where you appear to be.
This single request reveals nothing beyond what any website sees. Nothing else
leaves your computer. Partial tunnels are not looked up.

## Requirements (Linux)

Full matrix (processors, distributions, desktops, tunnel files, what was
tested on which machine): [docs/COMPATIBILITY.md](docs/COMPATIBILITY.md).

- Linux 5.6 or later
- NetworkManager 1.16 or later (installed by default on Ubuntu, Fedora, Debian desktop, Mint, Manjaro…)
- GTK 4.14, libadwaita 1.5 and libsoup 3 or later
- For OpenVPN profiles: NetworkManager's OpenVPN plugin
  (`network-manager-openvpn-gnome` on Ubuntu/Debian,
  `NetworkManager-openvpn-gnome` on Fedora, installed by default on their
  desktops; `networkmanager-openvpn` on Arch)
- `iso-codes` (optional) for country names in your language
- For the top bar icon on GNOME: the AppIndicator extension (enabled by default
  on Ubuntu). KDE Plasma supports it natively. Without it, closing the window quits

## Build from source (Linux)

Ubuntu / Debian:

```sh
sudo apt install libgtk-4-dev libadwaita-1-dev libsoup-3.0-dev gettext desktop-file-utils
```

Fedora:

```sh
sudo dnf install gtk4-devel libadwaita-devel libsoup3-devel gettext desktop-file-utils
```

Arch Linux:

```sh
sudo pacman -S gtk4 libadwaita libsoup3 gettext desktop-file-utils
```

Then, with a [Rust toolchain](https://rustup.rs):

```sh
make              # release build
sudo make install # installs to /usr/local
```

During development, `cargo run` is enough.

## Packages

- **Flatpak** (Flathub: Fedora, Mint, Debian, Arch, KDE…): [build-aux/flatpak](build-aux/flatpak/README.md)
- **Snap** (Ubuntu App Center): [snap](snap/README.md)

What comes next (one-line installer, APT/DNF repositories, website): [docs/ROADMAP.md](docs/ROADMAP.md).

## macOS

A native macOS app (SwiftUI, in `apple/`) exists and runs on a Mac with Apple
silicon, but it is **not released yet**: the website still says “coming soon”.
What was really run, and on which machine, is in
[docs/COMPATIBILITY.md](docs/COMPATIBILITY.md).

- A menu bar app and a main window with the features above: tunnel list,
  import, editor, export, QR code, public IP and location, live traffic, server
  health, logs of each tunnel, trusted networks, notifications, update notice.
  English and French, light and dark.
- WireGuard® and OpenVPN profiles. Nothing is bought: no App Store, no paid
  Apple account. The app will be a `.dmg` downloaded from the website, with
  OpenVPN inside.
- A helper that runs as root (a launchd daemon) makes the tunnels. The app
  installs it from its first screen, once, and macOS asks for the
  administrator's password. It never connects anything by itself: the app
  decides and asks it.
- The app is not signed with a paid account, so macOS blocks its first launch of
  each version: open System Settings › Privacy & Security and choose “Open
  Anyway” (when macOS offers “Done” and “Move to Trash”, choose “Done”).
- macOS 13 Ventura or later. Only macOS 27.2 on Apple silicon was run; Intel is
  not built yet.

Build it on a Mac, with the free command line tools, Rust and Homebrew
(`brew install openvpn`):

```sh
apple/build.sh        # apple/build/Tunnelbird.app
apple/make-dmg.sh     # apple/build/Tunnelbird-VERSION-ARCH.dmg
```

The helper's log is `/Library/Application Support/Tunnelbird/service.log`.
Everything else (decisions, how to run and test, how to remove it):
[docs/MACOS.md](docs/MACOS.md). Windows is planned, on the same core.

## Troubleshooting

- **Debug logs**: `G_MESSAGES_DEBUG=tunnelbird tunnelbird`
- **Logs of a desktop launch**: `journalctl --user -t tunnelbird`
- **Force a language**: `LANGUAGE=fr tunnelbird`
- **Settings ignored on import**: `PostUp`, `PreDown`, `Table`, `SaveConfig` are
  `wg-quick` features that NetworkManager does not run. Tunnelbird lists them
  after the import.

## Author

Created by **Ferhat Saidoun** — [ferhatsn.fr](https://ferhatsn.fr) · [GitHub](https://github.com/ferhatte10)

The app icon combines the red and blue triangle, Ferhat's personal mark, with a
padlock and a swallow leaving the tunnel. Icons live in `data/icons/hicolor`.

## License

Tunnelbird is free software, released under the
[GNU General Public License v3.0 or later](LICENSE).

## Contributing

Contributions and translations are welcome: see [CONTRIBUTING.md](CONTRIBUTING.md)
and [docs/TRANSLATING.md](docs/TRANSLATING.md).

## Naming and branding

Tunnelbird has its own name and identity. Contributors: read
[docs/TRADEMARK.md](docs/TRADEMARK.md) before touching names, icons or texts.
