// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

//! Compiles the translations listed in `po/LINGUAS` into `.mo` files.
//!
//! - Development (`cargo run`): the `.mo` files land in `$OUT_DIR/locale` and the
//!   binary looks for them there.
//! - Packaging: set `LOCALEDIR` (e.g. `/usr/share/locale`) at build time and
//!   install the `.mo` files there (`make install` does both).
//!
//! When `msgfmt` (package `gettext`) is missing, the build still succeeds and
//! the app stays in English.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};

const DOMAIN: &str = "tunnelbird";

fn main() {
    println!("cargo:rerun-if-changed=po");
    println!("cargo:rerun-if-env-changed=LOCALEDIR");

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("cargo always sets OUT_DIR"));
    let dev_dir = out_dir.join("locale");
    let localedir = env::var("LOCALEDIR").unwrap_or_else(|_| dev_dir.display().to_string());
    println!("cargo:rustc-env=TUNNELBIRD_LOCALEDIR={localedir}");

    let linguas = fs::read_to_string("po/LINGUAS").unwrap_or_default();
    for lang in linguas
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        compile(lang, &dev_dir);
    }
}

fn compile(lang: &str, dev_dir: &Path) {
    let target = dev_dir.join(lang).join("LC_MESSAGES");
    if let Err(err) = fs::create_dir_all(&target) {
        println!("cargo:warning=cannot create {}: {err}", target.display());
        return;
    }
    let output = target.join(format!("{DOMAIN}.mo"));
    match Command::new("msgfmt")
        .args(["--check", "-o"])
        .arg(&output)
        .arg(format!("po/{lang}.po"))
        .status()
    {
        Ok(status) if status.success() => {}
        Ok(status) => println!("cargo:warning=msgfmt failed for po/{lang}.po ({status})"),
        Err(err) => {
            println!("cargo:warning=msgfmt not found ({err}): install gettext to get translations")
        }
    }
}
