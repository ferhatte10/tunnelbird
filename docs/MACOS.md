# Tunnelbird on macOS: hand-off

The macOS version is built and runs on a Mac with Apple silicon; it is **not
released**. This page is what is needed to run, test and continue it. Read it
with `docs/plans/later.md` (decisions), `docs/PROTOCOL.md` (interface ↔ service
messages) and `docs/COMPATIBILITY.md` (what was really run).

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
| Shared logic: `.conf`/`.ovpn` parsers, providers, trusted networks, server health, versions, protocol, tunnel plan | `crates/core` | Tested (39 tests) |
| Service logic: requests, one tunnel at a time, storage, Unix socket | `crates/service/src/{service,store,transport}.rs` | Tested on Linux with a fake backend |
| macOS backend: BoringTun on a `utun`, addresses/routes/DNS with `ifconfig`/`route`/`networksetup`, counters from `netstat`, OpenVPN through `openvpn`, launchd daemon | `crates/service/src/macos/` | Compiled and run on macOS 27.2, Apple silicon: WireGuard® connects and disconnects with DNS and routes restored. OpenVPN, launchd and Intel not run yet |
| Menu bar app (SwiftUI) | `apple/` (`apple/build.sh`, no Xcode needed) | The features of the Linux app (README): tunnel list, import, field editor, export, QR code, public IP and location, live traffic, server health, logs of each tunnel, preferences, trusted networks, notifications, update notice, About window; English and French. Run with the real service on macOS 27.2, Apple silicon: what was seen is in COMPATIBILITY.md, which also lists what is only written |
| `.dmg` with the app, the helper and its install step | `apple/make-dmg.sh`; the app installs, updates and removes the service with the administrator's password | Built and installed on a Mac, including “Open Anyway” on a downloaded copy. Built by hand: no CI yet |

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

Notes from the first real runs (what was seen, and what is only reasoned):

- `boringtun` 0.7.1 `DeviceHandle::new("utunN", …)` and its control socket
  `/var/run/wireguard/utunN.sock`; the UAPI text comes from
  `tunnelbird_core::plan::WgPlan::uapi` (tested against the protocol).
- `ifconfig utunN inet A/32 A alias` for IPv4 point-to-point addresses.
- DNS through `networksetup` on every network service, restored on
  disconnect (`macos/net.rs`, `Journal`). A crash leaves it in place, so it is
  written down first and the next daemon start undoes it (run with
  `kill -9`: DNS on all services and the scoped route came back). SIGTERM and Ctrl+C restore it at once
  (`restore_network_on_stop`; not observed apart from a restart).
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
- OpenVPN and its libraries come from Homebrew at build time
  (`apple/bundle-openvpn.sh`) and are copied to `/Library/Tunnelbird` at installation
  (root only, and no space in the path: OpenVPN cannot run a quoted script). A release service only runs that copy: anyone can write in an app
  under `/Applications`, and Homebrew's files belong to the user.
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

## Build and install

```sh
apple/make-dmg.sh    # apple/build/Tunnelbird-VERSION-ARCH.dmg, needs Homebrew's openvpn
```

Open it, drop the app on Applications, start it: it offers to install the
service (administrator's password). To remove everything:

```sh
sudo "/Library/Application Support/Tunnelbird/tunnelbird-service" --uninstall-daemon
sudo rm -rf "/Library/Application Support/Tunnelbird"    # also the stored tunnels and keys
rm -rf /Applications/Tunnelbird.app
```

The app's version in `apple/Resources/Info.plist` must equal the service's in
`crates/service/Cargo.toml` (`build.sh` checks): a difference offers the user to
update the service, which an update of the app does not reach.

## Still to do

1. Release: for now the `.dmg` is built on the Mac. CI building it
   (`docs/plans/ci-and-release.md`, “The macOS `.dmg`”) and a published
   `latest.json` with the macOS download, so the update notice works.
2. The website's macOS tab with the “Open Anyway” screenshots, once a `.dmg` is
   published, then `AVAILABLE` in `website/src/lib/links.ts`. macOS first shows
   “Move to Trash” as its main button, next to “Done”: the page must say to
   choose “Done”, then Privacy & Security.
3. Intel: needs `rustup`, Intel builds of OpenVPN's libraries, and a Mac to test.
4. What `docs/COMPATIBILITY.md` marks as not run: macOS 13 to 26, a switch to
   Ethernet, the IPv6 block with a WireGuard® file carrying only IPv4, the
   service stopped normally with a tunnel up, start at login, notifications, a
   QR code scanned by a phone, the update notice.
5. The cause of the missing IPv6 through a WireGuard® tunnel.
