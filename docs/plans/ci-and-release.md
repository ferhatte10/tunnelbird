# Plan: continuous integration and the first release

Status: **written** (2026-10-29) in `.github/`: `check.yml`, `service.yml`,
`bundle.yml`, `install-sh.yml`, `website.yml`, `release.yml`, `dependabot.yml`.
Not run on GitHub yet: the Actions minutes are used up while the repository is
private (hosted runners are free once it is public). Until then pull requests
are merged by hand and the site image is published from the maintainer's computer.

Checked locally (2026-10-29): `actionlint` 1.7.12 (with ShellCheck on every
`run` block) clean; `make check`, the ShellCheck step and `make deb` with the
workflow's exact package list in a clean Ubuntu 24.04 container; `make rpm` with the
workflow's commands (`dnf builddep`) in a clean Fedora 42 container;
the four `install.sh` dry runs; the release's version check (right and wrong
tag) and its notes / `latest.json` step; the website image's smoke tests.
Not checked: the Windows and macOS jobs, the arm64
runners, the attestation and `gh release` steps (GitHub only).

## Goals

- Every pull request proves the code is formatted, lint-free, tested, and that
  the desktop file and the store listing are valid (`make check`).
- Every tag `vX.Y.Z` produces, with no manual step: `.deb` and `.rpm` packages
  for amd64 and arm64, a self-contained package for each (GTK inside), checksums, a GitHub release with the
  notes from the metainfo, and the data the website needs (`latest.json`).
- No secret in CI: only `GITHUB_TOKEN`. Signing keys live on the server that serves the repositories (see
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

### 2. `release.yml`: tags `v*`

| Job | Runner | Output |
|---|---|---|
| `deb` (matrix amd64, arm64) | `ubuntu-24.04`, `ubuntu-24.04-arm` | `tunnelbird_X.Y.Z_{amd64,arm64}.deb` via `make deb` (`build-aux/packaging/deb.sh`) |
| `rpm` (matrix x86_64, aarch64) | same runners, container `fedora:42@sha256:…` | `tunnelbird-X.Y.Z-1.fc42.{x86_64,aarch64}.rpm` via `make rpm` (`tunnelbird.spec`) |
| `bundle` (matrix amd64, arm64) | `ubuntu-24.04`, `ubuntu-24.04-arm` | `tunnelbird-X.Y.Z-linux-{x86_64,aarch64}.tar.gz` via `build-aux/bundle/build.sh`, started in a virtual screen by `smoke.sh` before it is published |
| `publish` | `ubuntu-24.04` | GitHub release + `SHA256SUMS` + `latest.json` + build provenance |

- The `.rpm` is built inside Fedora so it links against Fedora's libraries; the
  `.deb` on the oldest supported Ubuntu (24.04) works on newer Ubuntu and on
  Debian 13.
- Both packages install exactly what `make install PREFIX=/usr` installs
  (written and checked: `build-aux/packaging/README.md`). Runtime
  dependencies: `libgtk-4-1 (>= 4.14)`,
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
  "url": "https://tunnelbird.ferhatsn.fr/install/"
}
```

  The app reads only `version` (`src/update.rs`).

Budget per release: about 25 minutes over all jobs (free once public).

### 3. Dependabot

- `cargo` and the website's `bun` weekly, grouped minor/patch;
  `github-actions` and the website's base images (`docker`) monthly.

### 4. `service.yml`: the core and the service on Windows and macOS

- `windows-2025` and `macos-15`: clippy `-D warnings`, tests and a release
  build of `tunnelbird-core` (without gettext) and `tunnelbird-service`.
- Path filter (`crates/`, `Cargo.lock`): those runners cost 2× and 10× the
  minutes while private. The macOS code has run on a Mac; the Windows code was
  written without Windows and may fail at first (`docs/WINDOWS.md`,
  `docs/MACOS.md`).

### 5. `install-sh.yml` and `website.yml`

- `install.sh --dry-run` in Ubuntu 24.04, Debian 13, Fedora 42 and Arch
  containers (pinned by digest): each must choose apt, apt, dnf, and “other” (no package yet).
- `website.yml`: see `website.md`, “Deployment”.

### 6. The macOS `.dmg` (not written yet)

Today `apple/make-dmg.sh` runs on the maintainer's Mac. To build it in CI: a
`macos-15` job (Apple silicon) on the tag, with `brew install openvpn`, running
`apple/make-dmg.sh`; the `.dmg` goes to the release with its checksum and
attestation, and `latest.json` gets the macOS download so the app's update
notice works. The app is not signed with a paid account (`docs/MACOS.md`):
users go through “Open Anyway”. Intel is a second job, once an Intel Mac can
test it.

## Branch protection (when public)

- `main` protected: pull request required, `check` required, squash merge.
- Never enable “include administrators” without the maintainer's say-so.

## First release, step by step

1. Merge `feat/tunnelbird-v0.1` into `main` through a pull request (green
   `check`).
2. Packaging: done (`make deb`, `make rpm`), checked in clean Ubuntu 24.04
   and Fedora 42 containers; install on a real desktop VM once before the tag.
3. Set the release date in the metainfo, tag `v0.1.0`, push the tag.
4. Check the release page: 4 packages, `SHA256SUMS`, `latest.json`,
   attestations.
5. Then the website publishes the repositories (`website.md`).
6. There is no store: neither Flathub nor the Snap Store (decision of 2026-11-03).
   The install script is the one way in.

## Testing it before spending minutes

- `act` can run `check.yml` locally in a container to catch YAML mistakes.
- The package jobs are plain commands (`make deb`, `make rpm`),
  tested by hand first, so the first CI run is not the first try.
