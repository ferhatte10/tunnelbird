# Plan: continuous integration and the first release

Status: **plan only**. Nothing runs on GitHub until the Actions minutes are back
(or the repository is public, where hosted runners are free).

## Goals

- Every pull request proves the code is formatted, lint-free, tested, and that
  the desktop file and the store listing are valid (`make check`).
- Every tag `vX.Y.Z` produces, with no manual step: `.deb` and `.rpm` packages
  for amd64 and arm64, a Flatpak bundle, checksums, a GitHub release with the
  notes from the metainfo, and the data the website needs (`latest.json`).
- No secret in CI: only `GITHUB_TOKEN`. Signing keys live in the cluster (see
  `website.md`, “Package repositories”).
- Every action pinned by commit SHA (`uses: owner/action@<sha> # vX.Y.Z`),
  every container image by digest, every downloaded tool by version + SHA-256.

## Workflows

### 1. `check.yml`: pull requests and pushes to `main`

```yaml
on:
  pull_request:
  push: { branches: [main] }
permissions: { contents: read }
concurrency: { group: check-${{ github.ref }}, cancel-in-progress: true }
jobs:
  check:
    runs-on: ubuntu-24.04          # GTK 4.14 and libadwaita 1.5: our minimums
    steps:
      - uses: actions/checkout@<sha>
      - run: sudo apt-get install -y --no-install-recommends
             libgtk-4-dev libadwaita-1-dev libsoup-3.0-dev gettext
             desktop-file-utils appstream
      - uses: dtolnay/rust-toolchain@<sha>   # stable + rustfmt, clippy
      - uses: Swatinem/rust-cache@<sha>
      - run: make check
```

- Building on the oldest supported GTK proves the `v4_14` / `v1_5` feature
  gates are honest: an API newer than that fails the build.
- Budget: about 4 minutes cold, under 2 with the cache.

### 2. `flatpak.yml`: pull requests touching `build-aux/flatpak/` or `Cargo.lock`

- `flatpak/flatpak-github-actions/flatpak-builder@<sha>` in the container
  `ghcr.io/flathub-infra/flatpak-github-actions:gnome-50@sha256:…`.
- Fails when `cargo-sources.json` is stale (offline build), which is the usual
  mistake after a dependency update. Regenerate it with
  `flatpak-cargo-generator.py Cargo.lock -o build-aux/flatpak/cargo-sources.json`.
- Path filter so it does not cost minutes on every pull request.

### 3. `release.yml`: tags `v*`

| Job | Runner | Output |
|---|---|---|
| `deb` (matrix amd64, arm64) | `ubuntu-24.04`, `ubuntu-24.04-arm` | `tunnelbird_X.Y.Z_{amd64,arm64}.deb` via `cargo-deb` |
| `rpm` (matrix x86_64, aarch64) | same runners, container `fedora:42@sha256:…` | `tunnelbird-X.Y.Z-1.{x86_64,aarch64}.rpm` via `cargo-generate-rpm` |
| `flatpak` | `ubuntu-24.04` | `tunnelbird-X.Y.Z.flatpak` (offline bundle, for testers) |
| `publish` | `ubuntu-24.04` | GitHub release + `SHA256SUMS` + `latest.json` + build provenance |

- The `.rpm` is built inside Fedora so it links against Fedora's libraries; the
  `.deb` on the oldest supported Ubuntu (24.04) works on newer Ubuntu and on
  Debian 13.
- Both packages install exactly what `make install PREFIX=/usr` installs; the
  package metadata (`[package.metadata.deb]`, `[package.metadata.generate-rpm]`
  in `Cargo.toml`) lists the runtime dependencies: `libgtk-4-1 (>= 4.14)`,
  `libadwaita-1-0 (>= 1.5)`, `libsoup-3.0-0`, `network-manager`, `iso-codes`
  (Fedora: `gtk4`, `libadwaita`, `libsoup3`, `NetworkManager`, `iso-codes`).
  Recommended, so installed by default but not forced on WireGuard-only users:
  `network-manager-openvpn-gnome` (Fedora: `NetworkManager-openvpn-gnome`), the
  OpenVPN plugin and the part that lets the desktop ask for its password.
- `publish` checks that the tag equals `Cargo.toml`'s version and the newest
  `<release>` of the metainfo, then fails loudly otherwise.
- `actions/attest-build-provenance@<sha>` signs each file through GitHub's
  OIDC (no stored key): anyone can verify with `gh attestation verify`.
- `latest.json` is attached to the release; the website picks it up (see
  `website.md`). Shape:

```json
{
  "version": "0.2.0",
  "date": "2026-11-20",
  "url": "https://tunnelbird.ferhatsn.fr/download",
  "notes": { "en": "…", "fr": "…" },
  "minimum": "0.1.0"
}
```

Budget per release: about 25 minutes over all jobs (free once public).

### 4. Dependabot

- `cargo` weekly, grouped minor/patch, plus `github-actions` monthly.
- After a `cargo` update, the Flatpak job reminds to regenerate
  `cargo-sources.json` (or a small job regenerates it on the Dependabot branch).

## Branch protection (when public)

- `main` protected: pull request required, `check` required, squash merge.
- Never enable “include administrators” without the owner's say-so.

## First release, step by step

1. Merge `feat/tunnelbird-v0.1` into `main` through a pull request (green
   `check`).
2. Add the packaging metadata to `Cargo.toml` and test locally:
   `cargo deb`, then `sudo apt install ./target/debian/tunnelbird_*.deb` in a
   clean Ubuntu 24.04 VM; same for the `.rpm` in Fedora.
3. Set the release date in the metainfo, tag `v0.1.0`, push the tag.
4. Check the release page: 4 packages, the bundle, `SHA256SUMS`, `latest.json`,
   attestations.
5. Then the website publishes the repositories (`website.md`) and Flathub gets
   its pull request (`build-aux/flathub/README.md`).

## Testing it before spending minutes

- `act` can run `check.yml` locally in a container to catch YAML mistakes.
- The package jobs are plain commands (`cargo deb`, `cargo generate-rpm`),
  tested by hand first, so the first CI run is not the first try.
