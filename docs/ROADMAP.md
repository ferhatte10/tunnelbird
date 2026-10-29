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

## 4. Store submissions

- Flathub: switch the manifest source to the git tag, add screenshots to the
  metainfo, open the request on `flathub/flathub`.
- Snap Store: register `tunnelbird`, build on snapcraft.io from GitHub, request
  auto-connection of `network-manager` and `network-observe` on the forum.

## Later

- OpenVPN profiles (through NetworkManager's OpenVPN plugin).
- Other languages through Weblate.
- macOS and Windows clients on top of a shared Rust core.
