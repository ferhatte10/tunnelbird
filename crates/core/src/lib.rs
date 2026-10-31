// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

//! Tunnelbird's core: what every version of the app shares, whatever the
//! system and the toolkit. Pure logic only: no window, no network call, no
//! system service; each app (GTK on Linux, later Windows and macOS) brings
//! those. Everything here is unit-tested (`cargo test -p tunnelbird-core`).
//!
//! - [`config`]   parses and writes WireGuard® `.conf` tunnels;
//! - [`ovpn`]     parses and writes OpenVPN `.ovpn` profiles;
//! - [`profile`]  either kind of tunnel file, recognised by its content;
//! - [`provider`] recognises well-known VPN providers;
//! - [`trust`]    decides when to connect automatically (trusted networks);
//! - [`health`]   tells a working tunnel from one whose server stays silent;
//! - [`plan`]     a WireGuard® tunnel as Windows and macOS apply it;
//! - [`version`]  compares versions and reads release notes;
//! - [`protocol`] messages between a native interface and the tunnel service
//!   (Windows, macOS).
//!
//! User-visible messages go through gettext, in the app's text domain (see
//! [`i18n`]: optional, for systems without gettext).

pub mod config;
pub mod health;
pub mod i18n;
pub mod logs;
pub mod ovpn;
pub mod plan;
pub mod profile;
pub mod protocol;
pub mod provider;
pub mod trust;
pub mod version;
