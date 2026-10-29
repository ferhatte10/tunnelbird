// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

//! Reads an `.ovpn` profile the way Tunnelbird does and prints it back as
//! Tunnelbird exports it, or the error the app would show. Certificate files
//! the profile refers to are read next to it.
//!
//! `cargo run -p tunnelbird-core --example ovpn -- profile.ovpn > exported.ovpn`

use std::path::Path;
use std::process::ExitCode;

use tunnelbird_core::ovpn;

fn main() -> ExitCode {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: ovpn <profile.ovpn>");
        return ExitCode::from(2);
    };
    let folder = Path::new(&path).parent().unwrap_or(Path::new("."));
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) => {
            eprintln!("{path}: {err}");
            return ExitCode::FAILURE;
        }
    };
    let checked = ovpn::parse(&text).and_then(|mut config| {
        config.resolve_files(|name| std::fs::read_to_string(folder.join(name)).ok())?;
        config.check()?;
        Ok(config)
    });
    match checked {
        Ok(config) => {
            if !config.ignored.is_empty() {
                eprintln!("ignored: {}", config.ignored.join(", "));
            }
            print!("{}", config.to_ovpn());
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}
