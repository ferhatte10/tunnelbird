# Compatibility

What Tunnelbird runs on, what it connects to, and how sure we are. This page is
the source for the website's compatibility section: keep it accurate, and only
write **Tested** for what was actually run.

| Mark | Meaning |
|---|---|
| **Tested** | Run on a real machine and checked |
| **Expected** | Meets every requirement below, not tried yet |
| **Planned** | On the roadmap, not available |
| **No** | Not supported, with the reason |

## Linux

### Processors

| Architecture | Also called | `.deb` / `.rpm` | Self-contained package | From source |
|---|---|---|---|---|
| x86_64 | amd64, Intel / AMD 64-bit | Planned (CI) | Written (CI), started on Ubuntu 26.04 only | **Tested** |
| aarch64 | arm64 (Raspberry Pi 4/5 64-bit, ARM laptops, ARM servers) | Planned (CI) | Written (CI), never run | Expected |
| others (riscv64, ppc64le, armv7…) | | No | No | Expected: pure Rust + GTK, untested |

### Requirements

| Component | Minimum | Why |
|---|---|---|
| NetworkManager | 1.16 | Native WireGuard® profiles; Tunnelbird drives it over D-Bus |
| Linux kernel | 5.6 | WireGuard® built into the kernel (older kernels need the `wireguard` module) |
| GTK | 4.14 | Interface (native packages; the self-contained package brings its own) |
| libadwaita | 1.5 | Interface (same) |
| libsoup | 3.0 | Public IP and location lookup, update check |
| OpenVPN plugin of NetworkManager | 1.10 recommended | Only for `.ovpn` profiles. Packages: `network-manager-openvpn-gnome` (Debian, Ubuntu), `NetworkManager-openvpn-gnome` (Fedora), `networkmanager-openvpn` (Arch). Tunnelbird shows the command if it is missing |
| `iso-codes` | any | Optional: country names in your language |

### Distributions

There is no store: everything installs with the one-line script on the website.
On Ubuntu, Debian, Mint and Fedora it adds Tunnelbird's repository (native
packages, GTK 4.14 and libadwaita 1.5 from the system). Elsewhere it installs a
self-contained package that brings GTK and libadwaita; it is built on Ubuntu
24.04, so the system's C library must be as recent (glibc 2.39). Either way
NetworkManager must manage the network.

| Distribution | Native package | Self-contained package | Notes |
|---|---|---|---|
| Ubuntu 26.04 LTS | **Tested** (source build, `make install`) | Planned | Reference machine, see below |
| Ubuntu 24.04 LTS | Expected (oldest target: GTK 4.14, libadwaita 1.5) | Expected (installed by the script in a container) | The CI builds on it (`docs/plans/ci-and-release.md`) |
| Linux Mint 22, Pop!_OS 24.04, elementary OS 8, Zorin OS (24.04 base) | Expected (Ubuntu 24.04 packages) | Planned | |
| Debian 13 “trixie” | Expected | Expected (installed by the script in a container) | |
| Debian 12 “bookworm” | No (GTK 4.8) | No (glibc 2.36) | Not supported |
| Fedora 41 and later | Expected | Expected (installed by the script in a container, Fedora 42) | SELinux: OpenVPN certificates go to `~/.cert/nm-openvpn`, where it allows them |
| openSUSE Tumbleweed | From source | Planned | |
| Arch, Manjaro, EndeavourOS | From source (AUR later) | Expected (installed by the script in an Arch container) | OpenVPN plugin not installed by default |
| RHEL / AlmaLinux / Rocky 10 | Not packaged | Planned | |
| Systems without NetworkManager (servers using systemd-networkd or netplan only, Alpine…) | No | No | Tunnelbird hands tunnels to NetworkManager |
| WSL, ChromeOS Linux container | No | No | No NetworkManager-managed network |

### Desktops

The window works everywhere GTK 4 does, on **Wayland and X11**. The top bar
icon uses the StatusNotifierItem standard:

| Desktop | Window | Top bar icon |
|---|---|---|
| GNOME (Ubuntu) | **Tested** (GNOME 50, Wayland) | **Tested** with the AppIndicator extension, enabled by default on Ubuntu |
| GNOME (Fedora, Debian, Arch…) | Expected | Needs the “AppIndicator and KStatusNotifierItem Support” extension; without it, closing the window quits |
| KDE Plasma | Expected | Expected (native support) |
| Cinnamon, Budgie, MATE, Xfce | Expected | Expected where a StatusNotifierItem host runs (Xfce: `xfce4-statusnotifier-plugin`) |

### Tunnel files

| File | Support | Details |
|---|---|---|
| WireGuard® `.conf` (`wg-quick` format, from providers, routers, wg-easy…) | **Tested** | Lenient reading, several peers, IPv4 and IPv6, DNS and search domains. `PostUp`/`Table`/scripts are listed as ignored |
| AmneziaWG (obfuscated WireGuard®) | No | The kernel tunnel cannot apply obfuscation; a clear message says so |
| OpenVPN `.ovpn` | **Tested** against a real server (see `build-aux/test-server`): parsing, export and connection through NetworkManager, with certificates and password, with a password only, and with separate certificate files | Certificates inline or as separate files, user name and password, `tls-auth`, `tls-crypt`, `tls-crypt-v2`, UDP and TCP, several servers |
| OpenVPN with PKCS#12 (`.p12`) or static keys | No (yet) | A clear message asks for PEM certificates |
| ZIP bundles | No | A message asks to extract them first |

### Behaviour

| Item | Status | Details |
|---|---|---|
| IPv6 leak protection (a full tunnel that carries only IPv4) | **Tested** | With an OpenVPN server carrying only IPv4: IPv6 is blocked while connected, and the location shown is the server's. Not run: Flatpak, Snap, a WireGuard® file without `::/0` |
| Logs of a tunnel | Partly tested | The window and Save, by hand. Expected: Copy, the reason of a failed connection (wrong password), the notice in a Flatpak or a Snap, where the system journal cannot be read |

### Reference machine

Everything marked **Tested** on Linux was run on:

- Ubuntu 26.04.1 LTS, x86_64, Linux 7.0
- GNOME Shell 50.1 on Wayland
- NetworkManager 1.54.3, OpenVPN plugin 1.12.5, OpenVPN 2.7.0
- GTK 4.22.4, libadwaita 1.9.1, libsoup 3.6.6

## Windows and macOS

Native apps on top of the same core (`crates/core`) and a tunnel service
(`crates/service`, protocol in [PROTOCOL.md](PROTOCOL.md)); see
`docs/plans/later.md`.

| System | Status | Targets |
|---|---|---|
| Windows | Planned. The tunnel service (WireGuardNT, ovpn-dco-win, named pipe, encrypted storage, Windows service) is written and compiles for Windows x64, never run yet; the interface is not written (docs/WINDOWS.md) | Windows 10 and 11, x64 and ARM64; WireGuard® through WireGuardNT, OpenVPN through ovpn-dco-win; interface in WinUI 3 |
| macOS | Built and run on a Mac, not released yet: see below. The website says “coming soon” | macOS 13 Ventura and later; interface in SwiftUI (menu bar app); a `.dmg` from the website |

### macOS

Everything marked **Tested** was run on a MacBook Air, Apple silicon, macOS 27.2,
with the app installed from its `.dmg` built on that Mac.

| Item | Status | Details |
|---|---|---|
| Apple silicon | **Tested** | macOS 27.2 |
| Intel | Built (CI, `.dmg` x86_64), never run | Needs a real Intel Mac to test |
| macOS 13 to 26 | Expected | The target is 13; only 27.2 was run |
| WireGuard® tunnels | **Tested** | Routes, DNS, public IP, disconnection, Wi-Fi change, a website on the server's own address |
| OpenVPN tunnels | **Tested** | Certificates inline and as separate files, user name and password, routes and DNS pushed by the server, with the OpenVPN inside the app |
| IPv6 leak protection | **Tested** with OpenVPN | Expected with a WireGuard® file carrying only IPv4: not run |
| IPv6 through a WireGuard® tunnel | Unknown | No answer in the one test: cause not found yet |
| Service crash | **Tested** | After `kill -9` the next start restores DNS and routes, and launchd restarts it |
| Service stopped normally, with a tunnel up | Expected | Restores at once; not observed separately |
| Install from the `.dmg`, from the app | **Tested** | With the administrator's password |
| “Open Anyway” on a downloaded `.dmg` | **Tested** | With a `.dmg` marked as downloaded: macOS blocks the app, then System Settings › Privacy & Security opens it. Each user goes through it, no paid Apple account |
| Switch to another interface (Ethernet) | Expected | Written, not run: no cable at hand |
| Restart of the Mac | **Tested** | The service starts by itself; the network is untouched |
| Start at login, notifications, QR code scanned by a phone, update notice | Expected | Written, not confirmed on a real run |
| `.dmg` built by CI | Planned | Built by hand on the Mac for now (`docs/plans/ci-and-release.md`) |

Distribution on both: an installer from the website, no store, no paid
account (see the plan for what this means for the first launch).
