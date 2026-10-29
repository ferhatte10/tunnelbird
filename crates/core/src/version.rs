// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

//! Version numbers and release notes, for update notices on every system.

/// Seconds between two checks for a new version.
pub const CHECK_INTERVAL: i64 = 24 * 60 * 60;

/// True when `candidate` is a newer version than `current` (`0.10.0` > `0.9.1`).
pub fn is_newer(candidate: &str, current: &str) -> bool {
    version_parts(candidate) > version_parts(current)
}

/// `v1.2.3-rc1` → `[1, 2, 3]`; missing or unreadable parts count as 0.
fn version_parts(version: &str) -> [u64; 3] {
    let mut parts = [0; 3];
    let core = version.trim().trim_start_matches('v');
    let core = core.split(['-', '+']).next().unwrap_or_default();
    for (slot, part) in parts.iter_mut().zip(core.split('.')) {
        *slot = part.parse().unwrap_or(0);
    }
    parts
}

/// True when the last check is old enough (or in the future: clock changed).
pub fn check_due(last_check: i64, now: i64) -> bool {
    now < last_check || now - last_check >= CHECK_INTERVAL
}

/// Version and notes (AppStream markup) of the newest release in a store
/// listing (`metainfo.xml`).
pub fn newest_release(metainfo: &str) -> Option<(String, String)> {
    let release = &metainfo[metainfo.find("<release ")?..];
    let release = &release[..release.find("</release>")?];
    let version = release.split("version=\"").nth(1)?.split('"').next()?;
    let notes = release
        .split("<description>")
        .nth(1)?
        .split("</description>")
        .next()?;
    let notes = notes
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    Some((version.to_owned(), notes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_numerically() {
        assert!(is_newer("0.10.0", "0.9.1"));
        assert!(is_newer("v1.0.0", "0.99.99"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.1.0-rc1", "0.1.0"));
        assert!(!is_newer("garbage", "0.1.0"));
    }

    #[test]
    fn checks_once_a_day() {
        assert!(check_due(0, CHECK_INTERVAL));
        assert!(!check_due(1000, 1000 + CHECK_INTERVAL - 1));
        // The clock went back: check rather than wait for a date in the future.
        assert!(check_due(5000, 10));
    }

    #[test]
    fn whats_new_comes_from_the_newest_release() {
        let metainfo = r#"<releases>
            <release version="0.2.0" date="2026-11-20">
              <description>
                <p>Faster.</p>
                <ul><li>Fixed a bug</li></ul>
              </description>
            </release>
            <release version="0.1.0"><description><p>First.</p></description></release>
          </releases>"#;
        assert_eq!(
            newest_release(metainfo),
            Some((
                "0.2.0".to_owned(),
                "<p>Faster.</p>\n<ul><li>Fixed a bug</li></ul>".to_owned()
            ))
        );
    }
}
