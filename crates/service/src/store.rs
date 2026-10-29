// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

//! Tunnels on disk: one JSON file per tunnel in a folder only the service can
//! read (`/Library/Application Support/Tunnelbird` on macOS, folder 0700,
//! files 0600).
//!
//! Windows will encrypt each file with DPAPI under `%ProgramData%\Tunnelbird`
//! (see docs/PROTOCOL.md); until that is written, the service refuses to
//! store anything there rather than keep keys in clear text.

use std::path::PathBuf;

use crate::service::{Store, Stored};

pub struct FileStore {
    dir: PathBuf,
}

impl FileStore {
    pub fn open(dir: PathBuf) -> Result<Self, String> {
        create_private_dir(&dir).map_err(|err| format!("{}: {err}", dir.display()))?;
        Ok(Self { dir })
    }

    fn path(&self, id: &str) -> Result<PathBuf, String> {
        // Identifiers are UUIDs chosen by the service; refuse anything else so
        // a request can never name a file outside the folder.
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
            return Err(format!("Invalid tunnel identifier {id}"));
        }
        Ok(self.dir.join(format!("{id}.json")))
    }
}

impl Store for FileStore {
    fn load(&self) -> Result<Vec<Stored>, String> {
        let entries =
            std::fs::read_dir(&self.dir).map_err(|err| format!("{}: {err}", self.dir.display()))?;
        let mut tunnels = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|ext| ext != "json") {
                continue;
            }
            // A damaged file is reported and skipped: the others stay usable.
            match std::fs::read(&path)
                .map_err(|err| err.to_string())
                .and_then(|bytes| {
                    serde_json::from_slice::<Stored>(&bytes).map_err(|err| err.to_string())
                }) {
                Ok(tunnel) => tunnels.push(tunnel),
                Err(err) => eprintln!("Skipping {}: {err}", path.display()),
            }
        }
        tunnels.sort_by_key(|t| t.name.to_lowercase());
        Ok(tunnels)
    }

    fn save(&mut self, tunnel: &Stored) -> Result<(), String> {
        let path = self.path(&tunnel.id)?;
        let json = serde_json::to_vec_pretty(tunnel).map_err(|err| err.to_string())?;
        write_private(&path, &json).map_err(|err| format!("{}: {err}", path.display()))
    }

    fn delete(&mut self, id: &str) -> Result<(), String> {
        let path = self.path(id)?;
        match std::fs::remove_file(&path) {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => {
                Err(format!("{}: {err}", path.display()))
            }
            _ => Ok(()),
        }
    }
}

#[cfg(unix)]
fn create_private_dir(dir: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
}

#[cfg(unix)]
fn write_private(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    // Written next to the target, then renamed: a crash never leaves half a file.
    let partial = path.with_extension("json.partial");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&partial)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    std::fs::rename(partial, path)
}

#[cfg(not(unix))]
fn create_private_dir(_: &std::path::Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "encrypted storage (DPAPI) is not written yet",
    ))
}

#[cfg(not(unix))]
fn write_private(_: &std::path::Path, _: &[u8]) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "encrypted storage (DPAPI) is not written yet",
    ))
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    #[test]
    fn tunnels_survive_a_restart_and_stay_private() {
        let dir = std::env::temp_dir().join(format!("tunnelbird-store-{}", std::process::id()));
        let tunnel = Stored {
            id: "0f1e2d3c-0000-4000-8000-000000000001".to_owned(),
            name: "Home".to_owned(),
            text: "[Interface]".to_owned(),
            login: None,
        };
        let mut store = FileStore::open(dir.clone()).expect("folder created");
        store.save(&tunnel).expect("saved");
        let mode = std::fs::metadata(dir.join(format!("{}.json", tunnel.id)))
            .expect("file")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        assert_eq!(
            FileStore::open(dir.clone()).expect("reopened").load(),
            Ok(vec![tunnel.clone()])
        );
        assert!(
            store
                .save(&Stored {
                    id: "../escape".to_owned(),
                    ..tunnel.clone()
                })
                .is_err()
        );
        store.delete(&tunnel.id).expect("deleted");
        assert_eq!(store.load(), Ok(Vec::new()));
        std::fs::remove_dir_all(dir).expect("cleanup");
    }
}
