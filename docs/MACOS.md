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

## What exists

| Piece | Where | State |
|---|---|---|
| Shared logic: `.conf`/`.ovpn` parsers, providers, trusted networks, server health, versions, protocol, tunnel plan | `crates/core` | Tested (36 tests) |
| Service logic: requests, one tunnel at a time, storage, Unix socket | `crates/service/src/{service,store,transport}.rs` | Tested on Linux with a fake backend |
| macOS backend: BoringTun on a `utun`, addresses/routes/DNS with `ifconfig`/`route`/`networksetup`, counters from `netstat`, OpenVPN through `openvpn`, launchd daemon | `crates/service/src/macos/` | Compiled and run on macOS 27.2, Apple silicon: WireGuard® connects and disconnects with DNS and routes restored. OpenVPN, launchd and Intel not run yet |
| Menu bar app (SwiftUI) | `apple/` (`apple/build.sh`, no Xcode needed) | First version: tunnel list, connect, live traffic, import, delete, menu bar. Run by hand with the real service on macOS 27.2, Apple silicon. Location, server health, logs, editor, export, QR code, preferences, trusted networks, notifications and update notices written; checked with the fake service only, apart from what the owner tried by hand |
| `.dmg` with the app, the helper and its install step | — | To write |

## First run, step by step

1. Install the tools: Xcode (free, App Store) or `xcode-select --install`,
   Rust (`curl https://sh.rustup.rs | sh`), Homebrew, then
   `brew install openvpn` (for OpenVPN profiles).
2. Get the code: `git clone https://github.com/ferhatte10/tunnelbird` then
   work from `main`.
3. Compile: `cargo build -p tunnelbird-service`.
4. Tests: `cargo test -p tunnelbird-core -p tunnelbird-service` (the root
   crate is the Linux GTK app: not for macOS).
5. Protocol check without touching the network:
   `cargo run -p tunnelbird-service -- --fake --socket /tmp/tb.sock`, then in
   another terminal `nc -U /tmp/tb.sock` and type
   `{"id":1,"request":"hello","version":2}` (see docs/PROTOCOL.md for more).
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
  disconnect (`macos/net.rs`, `Journal`). A crash leaves it in place, so it is
  written down first and the next daemon start undoes it (run with
  `kill -9`: DNS on all services and the scoped route came back). SIGTERM and Ctrl+C restore it at once
  (`restore_network_on_stop`, still to run).
- The Wi-Fi name: macOS hides it from apps without the location permission
  (`ipconfig getsummary` shows `<redacted>`). The app asks for it, for trusted
  networks only; without it every Wi-Fi counts as untrusted.
- With the Wi-Fi down, the network path stays on Ethernet-like interfaces
  (Thunderbolt) that lead nowhere: the app reads what is really in use from
  `scutil --nwi`, 3 seconds after the last change, for trusted networks.
- The system removes the scoped default route when its interface goes down:
  `Journal::scope_to` checks the routing table, not only its own notes.
- The command line tools 27.2 (Swift 6.4) broke the app's build: their SDK 27
  turns `@State` into a macro whose plugin only comes with Xcode.
  `apple/build.sh` uses SDK 26 from the same tools. Xcode would also fix it.
- Swift has no test runner with the command line tools alone (XCTest and Swift
  Testing both fail): logic stays in Rust, where it is tested, and the app asks
  the service (`health`, `trust-decision`).
- `--install-daemon` copies the program to a root-only folder: root must never
  run a file the user can change. Not `/Library/PrivilegedHelperTools/`: macOS
  27.2 killed it there (exit 137, launchd `OS_REASON_EXEC`), while the same file
  ran elsewhere. Only ad hoc signed program in that folder.
- OpenVPN sets its own routes and DNS (`dns-updown`), and takes them down only
  when stopped with SIGTERM: a first run with SIGKILL left a route behind. It
  also picks its own `utun`, read from its log. Connects and passes traffic on
  a real server, and a clean stop leaves routes and DNS as they were. After a
  SIGKILL, its DNS stays in the system's `Setup:` key: `networksetup
  -setdnsservers <service> Empty` clears it (run once by hand).
- OpenVPN outlives a daemon killed with `kill -9`, tunnel up (seen for 13 s);
  the next daemon start stops it and everything comes back (run). It might still
  die later on a closed log pipe, leaving its DNS behind: not seen, not ruled out.
- IPv6 leak: a full tunnel carrying only IPv4 left IPv6 on the real network, and
  location websites showed the real place (seen on a real OpenVPN run). Blocked
  as on Linux with `reject` routes on `::/1` and `8000::/1`
  (`Journal::block_ipv6`). Run with OpenVPN: `curl -6` fails, IPv4 passes, the
  routes go away on disconnect. Not run yet with a WireGuard® file carrying
  only IPv4.
- The tunnel's UDP sockets are bound to the physical interface (`IP_BOUND_IF`)
  instead of a host route to the server, so a website on the server's address
  also goes through the tunnel. BoringTun has no such option:
  `vendor/boringtun/PATCHES.md`. A bound socket also needs a default route
  scoped to its interface (`route add -ifscope`), or the tunnel's `0/1` and
  `128.0/1` routes make it fail with “Network is unreachable”: seen on a real
  run. `Journal::scope_to` adds it: run on a real tunnel, a website on the
  server's address opens through the tunnel. `follow_network` follows network
  changes: a switch to a phone's Wi-Fi hotspot and back kept the tunnel up, one
  ping lost. A switch to another interface (Ethernet) has not run: no cable at
  hand, and the rebinding it needs is only reasoned.
- The socket `/var/run/tunnelbird.sock` must be owned `root:admin`, mode 0660.

## Still to build

1. **SwiftUI menu bar app** (`apple/` folder, Swift package): connects to
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
