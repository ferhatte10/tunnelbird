# Guide for AI coding assistants

Tunnelbird is a GTK4/libadwaita VPN **client** written in Rust. It drives
NetworkManager over D-Bus and never runs as root. It does not manage servers.

## Read first

1. `docs/ARCHITECTURE.md` — modules, data flow and design decisions.
2. `CONTRIBUTING.md` — code rules (no panics, gettext, GNOME HIG).
3. `docs/TRADEMARK.md` — own identity: no third-party VPN name or logo in the UI,
   texts, names or icons (technical identifiers in code are fine).

## Map

- `src/config.rs` — lenient `.conf` parser (pure, unit-tested: `cargo test`).
- `src/provider.rs` — provider detection table; add providers there, with a test.
- `src/nm.rs` — D-Bus proxies and `WgConfig` → NetworkManager settings.
- `src/geo.rs` — public IP/location lookup (libsoup + serde), flags, country names.
- `src/traffic.rs` — `/sys` counters and the Cairo graph.
- `src/window.rs` — UI; all errors go through `Ui::report_error`.
- `src/settings.rs` / `src/preferences.rs` — preferences file and dialog.
- `src/tray.rs` — top bar icon + menu (StatusNotifierItem + dbusmenu, own zbus code).
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
cargo clippy --all-targets -- -D warnings
cargo test
```
