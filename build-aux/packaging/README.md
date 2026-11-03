# Native packages

| File | Makes | Command |
|---|---|---|
| `deb.sh` | `.deb` for Ubuntu, Debian, Mint, Pop!_OS… | `make deb` |
| `tunnelbird.spec` | `.rpm` for Fedora and RHEL-like systems | `make rpm` (needs `rpmbuild`) |
| `../install.sh` | One-command installer served by the website: adds the signed APT or DNF repository, or installs the self-contained package (`../bundle/`) in the home folder | `sh build-aux/install.sh --dry-run` |

Packages land in `target/packages/`. Both install exactly what `make install
PREFIX=/usr` installs, and recommend the OpenVPN plugin and `iso-codes`.

- `.deb` dependencies are read from the binary by `dpkg-shlibdeps`, so they
  match the system it was built on; NetworkManager is added. The CI builds on
  Ubuntu 24.04, the oldest supported.
- `.rpm` builds from source inside Fedora (`%check` validates the desktop file
  and the store listing).
- `make check` fails if the `.spec` or the metainfo do not announce the
  version in `Cargo.toml`: bump all three together.

## Checked (2026-10-29)

- `.deb` built on Ubuntu 26.04: installs on a clean Ubuntu 24.04 container
  with its dependencies (OpenVPN plugin and `iso-codes` pulled as recommended),
  and starts (`tunnelbird --help`).
- `.rpm` built from source in a clean Fedora 42 container, installed and
  started there.
- `install.sh --dry-run` on clean Ubuntu 24.04, Debian 13, Fedora 42 and Arch
  containers: right method for each, every step shown. All shell scripts pass
  ShellCheck.

The APT/DNF repositories themselves (signed, served by the website) are not
online yet: see `docs/plans/website.md`.
