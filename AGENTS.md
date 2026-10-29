# Guide for AI coding assistants

Tunnelbird is a GTK4/libadwaita VPN **client** written in Rust. It drives
NetworkManager over D-Bus and never runs as root. It does not manage servers.

## Read first

1. `docs/ARCHITECTURE.md` — modules, data flow and design decisions.
2. `CONTRIBUTING.md` — code rules (no panics, gettext, GNOME HIG).
3. `docs/TRADEMARK.md` — own identity: no third-party VPN name or logo in the UI,
   texts, names or icons (technical identifiers in code are fine).

## Keep up to date

- `docs/COMPATIBILITY.md` when a requirement changes or something gets tested
  (the website copies it; only write “Tested” for what was really run).
- `docs/PROTOCOL.md` with `crates/core/src/protocol.rs`.

## Map

- `website/` — the project's site (SvelteKit, static), see `website/README.md`
  and `website/DESIGN.md`.

- `crates/core/` — `tunnelbird-core`, logic shared with future Windows/macOS
  apps: no GTK, no D-Bus, no file system, everything unit-tested.
  - `config.rs` — lenient `.conf` parser; `ovpn.rs` — `.ovpn` parser/writer;
    `profile.rs` picks the parser from the file's content.
  - `provider.rs` — provider detection table; add providers there, with a test.
  - `trust.rs`, `health.rs`, `version.rs` — trusted networks, server health,
    versions.
  - `protocol.rs` — interface ↔ tunnel service messages for Windows/macOS
    (`docs/PROTOCOL.md`); renaming a field is a breaking change.
- `src/openvpn.rs` — NetworkManager's OpenVPN settings and certificate files.
- `crates/service/` — tunnel service for Windows/macOS: request handling,
  storage and Unix socket tested with a fake backend (`--fake`); `windows/`
  and `macos/` backends written but never run on those systems
  (`docs/WINDOWS.md`, `docs/MACOS.md`: first-run steps and AI prompts).
- `crates/core/src/plan.rs` — a WireGuard® tunnel as Windows/macOS apply it
  (keys, routes with the server bypass, DNS, UAPI text).
- `src/nm.rs` — D-Bus proxies and `WgConfig` → NetworkManager settings.
- `src/geo.rs` — public IP/location lookup (libsoup + serde), flags, country names.
- `src/traffic.rs` — `/sys` counters and the Cairo graph.
- `src/window.rs` — UI; all errors go through `Ui::report_error`.
- `src/settings.rs` / `src/preferences.rs` — preferences file and dialog.
- `src/tray.rs` — top bar icon + menu (StatusNotifierItem + dbusmenu, own zbus code).
- `src/update.rs` — install method, update notices, “What's New”.
- `src/demo.rs` — demo mode for screenshots (`make screenshots`); never real data.
- `po/` — translations; `po/POTFILES` lists the files with strings.

## Conventions to keep

- zbus 6: variant types are `zbus::Variant` / `zbus::OwnedVariant`
  (formerly `zvariant::Value`).
- UI is built in Rust code, no `.ui`/Blueprint files.
- Async work runs on the GTK main loop with `glib::spawn_future_local`.
- New user-visible strings: English, `gettext()`, named `{placeholders}`, then
  add them to `po/fr.po`.
- Logging: `glib::g_warning!(LOG_DOMAIN, …)` for failures, `g_debug!` for details.

## Commands

```sh
cargo run
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```
