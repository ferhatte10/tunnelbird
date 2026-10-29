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

| Architecture | Also called | Flatpak (Flathub) | Snap | `.deb` / `.rpm` | From source |
|---|---|---|---|---|---|
| x86_64 | amd64, Intel / AMD 64-bit | **Tested** (local build) | Expected | Planned (CI) | **Tested** |
| aarch64 | arm64 (Raspberry Pi 4/5 64-bit, ARM laptops, ARM servers) | Expected (Flathub builds it) | Expected | Planned (CI) | Expected |
| others (riscv64, ppc64le, armv7…) | | No (not built by Flathub for us) | No | No | Expected: pure Rust + GTK, untested |

### Requirements

| Component | Minimum | Why |
|---|---|---|
| NetworkManager | 1.16 | Native WireGuard® profiles; Tunnelbird drives it over D-Bus |
| Linux kernel | 5.6 | WireGuard® built into the kernel (older kernels need the `wireguard` module) |
| GTK | 4.14 | Interface (native packages; the Flatpak and the Snap bring their own) |
| libadwaita | 1.5 | Interface (same) |
| libsoup | 3.0 | Public IP and location lookup, update check |
| OpenVPN plugin of NetworkManager | 1.10 recommended | Only for `.ovpn` profiles. Packages: `network-manager-openvpn-gnome` (Debian, Ubuntu), `NetworkManager-openvpn-gnome` (Fedora), `networkmanager-openvpn` (Arch). Tunnelbird shows the command if it is missing |
| `iso-codes` | any | Optional: country names in your language |

### Distributions

Native packages need GTK 4.14 and libadwaita 1.5; anything older uses the
Flatpak, which brings them. Any distribution where NetworkManager manages the
network works through the Flatpak.

| Distribution | Native package | Flatpak | Notes |
|---|---|---|---|
| Ubuntu 26.04 LTS | **Tested** (source build, `make install`) | **Tested** | Reference machine, see below |
| Ubuntu 24.04 LTS | Expected (oldest target: GTK 4.14, libadwaita 1.5) | Expected | The CI will build on it (`docs/plans/ci-and-release.md`) |
| Linux Mint 22, Pop!_OS 24.04, elementary OS 8, Zorin OS (24.04 base) | Expected (Ubuntu 24.04 packages) | Expected | |
| Debian 13 “trixie” | Expected | Expected | |
| Debian 12 “bookworm” | No (GTK 4.8) | Expected | Use the Flatpak |
| Fedora 41 and later | Expected | Expected | SELinux: OpenVPN certificates go to `~/.cert/nm-openvpn`, where it allows them |
| openSUSE Tumbleweed | From source | Expected | |
| Arch, Manjaro, EndeavourOS | From source (AUR later) | Expected | OpenVPN plugin not installed by default |
| RHEL / AlmaLinux / Rocky 10 | Not packaged | Expected | |
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
| OpenVPN `.ovpn` | **Tested** against a real server: parsing and export (see `build-aux/test-server`); connection through NetworkManager still to test | Certificates inline or as separate files, user name and password, `tls-auth`, `tls-crypt`, `tls-crypt-v2`, UDP and TCP, several servers |
| OpenVPN with PKCS#12 (`.p12`) or static keys | No (yet) | A clear message asks for PEM certificates |
| ZIP bundles | No | A message asks to extract them first |

### Reference machine

Everything marked **Tested** on Linux was run on:

- Ubuntu 26.04.1 LTS, x86_64, Linux 7.0
- GNOME Shell 50.1 on Wayland
- NetworkManager 1.54.3, OpenVPN plugin 1.12.5, OpenVPN 2.7.0
- GTK 4.22.4, libadwaita 1.9.1, libsoup 3.6.6
- Flatpak 1.16.6, GNOME 50 runtime

## Windows and macOS

Native apps on top of the same core (`crates/core`) and a tunnel service
(`crates/service`, protocol in [PROTOCOL.md](PROTOCOL.md)); see
`docs/plans/later.md`.

| System | Status | Targets |
|---|---|---|
| Windows | Planned. The service's logic compiles for Windows (x86_64) and is tested; the driver backend and the interface are not written | Windows 10 and 11, x64 and ARM64; WireGuard® through WireGuardNT, OpenVPN through ovpn-dco-win; interface in WinUI 3 |
| macOS | Planned. Same: the service compiles for macOS (Apple silicon) and is tested | macOS 13 Ventura and later, Apple silicon and Intel; interface in SwiftUI (menu bar app) |

Distribution on both: an installer from the website, no store, no paid
account (see the plan for what this means for the first launch).
