# Roadmap

What is done lives in the README; this file lists what comes next, in order.

## 1. Public repository

- Push the project to GitHub (`ferhatte10/tunnelbird`), first tag `v0.1.0`.
- Public repository: GitHub Actions minutes are free, which the packaging
  below relies on.
- CI: `make check` (format, lints, tests, desktop and AppStream validation).

## 2. Easy installation, like other desktop apps

From the simplest to the most technical, all presented on the website:

1. **One command**:
   `curl -fsSL https://tunnelbird.ferhatsn.fr/install.sh | sh`
   - Detects the distribution and picks the best method: APT repository
     (Ubuntu, Debian, Mint, Pop!_OS), DNF repository (Fedora, RHEL-like),
     otherwise Flatpak from Flathub.
   - Shows each step, asks for the password only when needed, and never runs
     anything it did not announce. Readable before running (link on the site).
   - Also checks the requirements: NetworkManager running, Linux 5.6+.
2. **Packages to download**: `.deb` and `.rpm`, built by CI for amd64 and arm64
   (`cargo-deb`, `cargo-generate-rpm` or `nfpm`), attached to each GitHub release.
3. **Signed APT and DNF repositories** served by the website, so updates come
   with the system's own updates. One GPG key, published on the site.
4. **Stores**: Flathub (`build-aux/flatpak`) and Snap Store (`snap/`), with
   their buttons on the site.

## 3. Website: `tunnelbird.ferhatsn.fr`

- Hosted on the homelab Kubernetes cluster, like the portfolio
  (Argo CD, Traefik, TLS).
- Pages: presentation with screenshots, “Install” (steps per distribution and
  the one-line command), documentation, privacy, changelog.
- Serves `install.sh`, the APT/DNF repositories and the signing key.
- English first, French translation, same identity as the app (icon, colours).

## 4. Updates: every release reaches every user

Each new version must be announced in the app, whatever the install method,
and each method updates in its own way. Tunnelbird never replaces its own
binary: it tells the user what will happen and sends them to the right place.

### What happens per install method

| Installed with | Who installs the update | What Tunnelbird shows |
|---|---|---|
| Snap Store | snapd, automatically (several times a day) | “Tunnelbird updates itself”; once the new revision is in, “Restart to finish the update” |
| Flathub (Flatpak) | GNOME Software / Discover, automatically or on click; `flatpak update` | Banner with “Update” through the Flatpak update portal (`CreateUpdateMonitor`), then “Restart to finish the update” |
| `install.sh` → APT/DNF repository | The system updates (Software, Update Manager, `apt upgrade`, `dnf upgrade`) | “Arrives with your system updates”, button to open Software, and the command for terminal users |
| `.deb`/`.rpm` downloaded by hand | Nobody: no repository | Banner with a button to the website's download page (or to re-run `install.sh`, which adds the repository) |
| Built from source / `make run` | The developer | Discreet line only, no banner |

### How the app knows

- **A newer version exists**: once a day at most, fetch
  `https://tunnelbird.ferhatsn.fr/latest.json` (version, date, notes, minimum
  supported version). No personal data sent; can be turned off in Preferences.
  Flatpak installs use the update portal instead, which knows Flathub's state.
- **How it was installed**: `/.flatpak-info` (Flatpak), `$SNAP` (Snap), the
  package database (`dpkg -S`/`rpm -qf` on its own binary) for APT/DNF, else
  “manual”.
- **The update is installed but the old version still runs**: the binary on
  disk changed after the app started (or a new snap revision exists) → “Restart
  to finish the update”, with a Restart button (same mechanism as the language
  switch). Background mode makes this matter: the app may run for weeks.
- **After an update**: the version stored in the settings differs from the
  running one → “What's new in X”, once, from the metainfo release notes
  (also in About › What's New).
- **Top bar**: a line “Update available” in the menu, the same button.

### Release checklist (for each version)

1. Bump `Cargo.toml` and add a `<release>` with notes to the metainfo.
2. Tag `vX.Y.Z`: CI builds `.deb`/`.rpm`, publishes them to the APT/DNF
   repositories and the GitHub release, and updates `latest.json` on the site.
3. Flathub: update the manifest tag (pull request on the app's Flathub repo,
   or automatic with `x-checker-data`).
4. Snap: snapcraft.io builds from the tag; promote from `edge` to `stable`.

## 5. Store submissions

- Flathub: switch the manifest source to the git tag, add screenshots to the
  metainfo, open the request on `flathub/flathub`.
- Snap Store: register `tunnelbird`, build on snapcraft.io from GitHub, request
  auto-connection of `network-manager` and `network-observe` on the forum.

## Later

- OpenVPN profiles (through NetworkManager's OpenVPN plugin).
- Other languages through Weblate.
- macOS and Windows clients on top of a shared Rust core.
