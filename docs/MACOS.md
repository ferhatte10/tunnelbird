# Tunnelbird on macOS: hand-off

Everything needed to continue the macOS version on a Mac, with or without an
AI assistant. Read with `docs/plans/later.md` (decisions), `docs/PROTOCOL.md`
(interface ↔ service messages) and `docs/COMPATIBILITY.md`.

## Decisions already taken (do not reopen)

- **No App Store, no paid Apple account, nothing paid**: the app is a `.dmg`
  downloaded from the website. So no NetworkExtension (it needs a paid
  entitlement): tunnels are made by a **root helper**, like `wg-quick` and
  Tunnelblick do. First launch of each version: System Settings › Privacy &
  Security › “Open Anyway” (shown on the website).
- **Native interface in SwiftUI**: a menu bar app (`MenuBarExtra`) plus a main
  window. All logic stays in Rust; the interface only shows and sends clicks.
- Targets: macOS 13 Ventura and later, Apple silicon and Intel.
- Identity: the Tunnelbird icon as is (never redrawn), English and French, no
  legal/trademark notice in the interface; WireGuard® only as the tunnel type.

## What exists (written on Linux, NOT YET RUN on a Mac)

| Piece | Where | State |
|---|---|---|
| Shared logic: `.conf`/`.ovpn` parsers, providers, trusted networks, server health, versions, protocol, tunnel plan | `crates/core` | Tested (36 tests) |
| Service logic: requests, one tunnel at a time, storage, Unix socket | `crates/service/src/{service,store,transport}.rs` | Tested on Linux with a fake backend |
| macOS backend: BoringTun on a `utun`, addresses/routes/DNS with `ifconfig`/`route`/`networksetup`, counters from `netstat`, OpenVPN through `openvpn`, launchd daemon | `crates/service/src/macos/` | Written, **never compiled for macOS** (no Apple C compiler on the Linux PC for `ring`) |
| Menu bar app (SwiftUI) | — | To write |
| `.dmg` with the app, the helper and its install step | — | To write |

## First run, step by step

1. Install the tools: Xcode (free, App Store) or `xcode-select --install`,
   Rust (`curl https://sh.rustup.rs | sh`), Homebrew, then
   `brew install openvpn` (for OpenVPN profiles).
2. Get the code: `git clone https://github.com/ferhatte10/tunnelbird` then
   `git switch feat/tunnelbird-v0.1` (or `main` once merged).
3. Compile: `cargo build -p tunnelbird-service`. Fix what the compiler says in
   `crates/service/src/macos/`: it was never compiled for macOS.
4. Tests: `cargo test -p tunnelbird-core -p tunnelbird-service` (the root
   crate is the Linux GTK app: not for macOS).
5. Protocol check without touching the network:
   `cargo run -p tunnelbird-service -- --fake --socket /tmp/tb.sock`, then in
   another terminal `nc -U /tmp/tb.sock` and type
   `{"id":1,"request":"hello","version":1}` (see docs/PROTOCOL.md for more).
6. Real tunnel, as root (it creates an interface and changes routes and DNS):
   `sudo ./target/debug/tunnelbird-service --daemon --socket /tmp/tb.sock`,
   then `hello`, `import` a WireGuard® file, `connect`. Check:
   `ifconfig utun10`, `netstat -rn`, `scutil --dns`, a website opens, the
   public IP changed; `disconnect`: routes and DNS are back as before.
7. Same with an OpenVPN profile. A test server can be started with
   `build-aux/test-server/aws.sh up` (see its README).
8. Once it works: `sudo ./target/release/tunnelbird-service --install-daemon`
   installs the launchd daemon (`/Library/LaunchDaemons/fr.ferhatsn.Tunnelbird.Service.plist`);
   `--uninstall-daemon` removes it.

Points to watch on the first real run (reasoned, not verified):

- `boringtun` 0.7.1 `DeviceHandle::new("utunN", …)` and its control socket
  `/var/run/wireguard/utunN.sock`; the UAPI text comes from
  `tunnelbird_core::plan::WgPlan::uapi` (tested against the protocol).
- `ifconfig utunN inet A/32 A alias` for IPv4 point-to-point addresses.
- DNS through `networksetup` on every network service, restored on
  disconnect (`macos/net.rs`, `Dns`): check a crash does not leave the
  tunnel's DNS in place (if it does, restore at daemon start).
- The socket `/var/run/tunnelbird.sock` must be owned `root:admin`, mode 0660.

## Still to build

1. **SwiftUI menu bar app** (`apple/` folder, Xcode project): connects to
   `/var/run/tunnelbird.sock`, sends the JSON lines of docs/PROTOCOL.md,
   shows the tunnel list, connect switch, live traffic (from `traffic`
   events), import (file picker → text → `import`), editor, preferences
   (trusted networks: the decision logic is in `tunnelbird_core::trust`, to
   call through a small C/Swift bridge or reimplement the few lines), update
   notice. Same texts as the Linux app (`po/fr.po` holds the French).
2. **Installer**: `.dmg` with `Tunnelbird.app`; at first launch the app asks
   for the administrator password once to copy the helper and run
   `--install-daemon` (AuthorizationExecuteWithPrivileges is deprecated:
   use an `osascript … with administrator privileges` call, or a `.pkg`).
3. Website: the macOS tab of the install section, with the “Open Anyway”
   screenshots.

## Prompt for an AI session on the Mac

Paste this in a Claude Code session opened in the cloned repository:

> You are continuing Tunnelbird, an open-source VPN client (Rust core,
> GTK app on Linux). Read `AGENTS.md`, `docs/MACOS.md`, `docs/PROTOCOL.md`,
> `docs/plans/later.md` and `docs/COMPATIBILITY.md` first. Decisions are
> final: no App Store, no paid Apple account, nothing paid; a root launchd
> helper (`crates/service`, `--daemon`) makes the tunnels with BoringTun and
> `openvpn`; the interface is a native SwiftUI menu bar app talking to the
> helper over `/var/run/tunnelbird.sock` with the JSON protocol. Start with
> “First run, step by step”: make `cargo build -p tunnelbird-service`
> compile on this Mac (the macOS code in `crates/service/src/macos/` was
> written on Linux and never compiled), then test with `--fake`, then a real
> tunnel as root, telling me each sudo command to run myself. Then build the
> SwiftUI app. Rules: English code and docs, French when talking to me, no
> commit without my explicit request, commits on a feature branch, never
> mention an AI in commits, never put the WireGuard logo or a legal notice in
> the interface, keep `docs/COMPATIBILITY.md` honest (“Tested” only when
> really run).
