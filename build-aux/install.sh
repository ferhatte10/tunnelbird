#!/bin/sh
# Tunnelbird installer: https://tunnelbird.ferhatsn.fr/install.sh
#
#   curl -fsSL https://tunnelbird.ferhatsn.fr/install.sh | sh
#   curl -fsSL https://tunnelbird.ferhatsn.fr/install.sh | sh -s -- --dry-run
#   curl -fsSL https://tunnelbird.ferhatsn.fr/install.sh | sh -s -- --uninstall
#
# Picks the best way for this system and says each step before doing it:
#   - Ubuntu, Debian, Mint, Pop!_OS…: Tunnelbird's signed APT repository, so
#     updates arrive with the system updates;
#   - Fedora, RHEL-like…: Tunnelbird's signed DNF repository, same;
#   - anything else, or while the repository is not published: a self-contained
#     package (GTK and libadwaita inside) in your home folder, no administrator;
#   - macOS (Apple silicon): the app from the release's disk image, in
#     ~/Applications, no administrator. The app asks for the administrator's
#     password itself, once, to install its helper.
# Administrator rights (sudo) are asked only for the steps that need them.
#
# Options:
#   --dry-run          print the steps, change nothing
#   --method NAME      force apt, dnf or bundle (the self-contained package)
#   --uninstall        remove what the bundle method (or the macOS app) installed
#   --help
#
# Source: https://github.com/ferhatte10/tunnelbird/blob/main/build-aux/install.sh
set -eu

SITE=https://tunnelbird.ferhatsn.fr
REPO=ferhatte10/tunnelbird
# Where the release files are (SHA256SUMS and the package); tests point it elsewhere.
DOWNLOAD=${TUNNELBIRD_DOWNLOAD_BASE:-https://github.com/$REPO/releases/latest/download}
APP_ID=fr.ferhatsn.Tunnelbird
KEYRING=/etc/apt/keyrings/tunnelbird.gpg
APT_SOURCE=/etc/apt/sources.list.d/tunnelbird.sources
DNF_REPO=/etc/yum.repos.d/tunnelbird.repo
# The self-contained package is built on Ubuntu 24.04: it needs a C library as recent.
GLIBC_MIN=2.39

DATA=${XDG_DATA_HOME:-$HOME/.local/share}
BIN=$HOME/.local/bin
DEST=$DATA/tunnelbird

APPS=$HOME/Applications
APP_NAME=Tunnelbird.app
MAC_SERVICE="/Library/Application Support/Tunnelbird/tunnelbird-service"

dry_run=false
uninstall=false
method=""

say() { printf '\n\033[1m%s\033[0m\n' "$*"; }
note() { printf '  %s\n' "$*"; }
fail() { printf '\n\033[31mError:\033[0m %s\n' "$*" >&2; exit 1; }

# A missing requirement stops the install, but only warns in a dry run.
need() {
  if [ "$dry_run" = true ]; then note "Warning: $*"; else fail "$*"; fi
}

# Runs a command, or only shows it with --dry-run.
run() {
  printf '  $ %s\n' "$*"
  if [ "$dry_run" = false ]; then
    "$@"
  fi
}

# Same, with administrator rights.
root() {
  if [ "$(id -u)" -eq 0 ]; then
    run "$@"
  else
    run sudo "$@"
  fi
}

# Writes standard input to a system file (with administrator rights).
write_root() {
  printf '  (writing %s)\n' "$1"
  if [ "$dry_run" = true ]; then
    sed 's/^/    | /'
  elif [ "$(id -u)" -eq 0 ]; then
    cat > "$1"
  else
    sudo tee "$1" > /dev/null
  fi
}

# Writes standard input to a file of the user's.
write_user() {
  printf '  (writing %s)\n' "$1"
  if [ "$dry_run" = true ]; then
    sed 's/^/    | /'
  else
    cat > "$1"
  fi
}

# Version of the C library ("2.39"), or nothing when it is not glibc.
glibc_version() {
  getconf GNU_LIBC_VERSION 2> /dev/null | sed -n 's/^glibc //p'
}

# Whether the repository answers at this address (a quiet check, changes nothing).
published() {
  command -v curl > /dev/null 2>&1 && curl -fsI "$1" > /dev/null 2>&1
}

while [ $# -gt 0 ]; do
  case $1 in
    --dry-run) dry_run=true ;;
    --method) shift; method=${1:-} ;;
    --uninstall) uninstall=true ;;
    --help|-h)
      echo "Usage: install.sh [--dry-run] [--method apt|dnf|bundle] [--uninstall]"
      echo "Installs Tunnelbird the best way for this system. See $SITE"
      exit 0 ;;
    *) fail "Unknown option $1 (see --help)" ;;
  esac
  shift
done

case $(uname -s) in
  Linux|Darwin) ;;
  *) fail "Tunnelbird for $(uname -s) is not available yet: see $SITE" ;;
esac

# --- macOS ---------------------------------------------------------------------

if [ "$(uname -s)" = Darwin ]; then
  if [ "$uninstall" = true ]; then
    say "Removing Tunnelbird"
    [ "$dry_run" = true ] && note "Dry run: nothing will be changed."
    if [ -e "$MAC_SERVICE" ]; then
      note "The helper runs as an administrator: removing it asks for your password."
      root "$MAC_SERVICE" --uninstall-daemon
      root rm -rf "/Library/Application Support/Tunnelbird"
    fi
    run rm -rf "$APPS/$APP_NAME" "/Applications/$APP_NAME"
    note "Your tunnels were stored with the helper: they are removed with it."
    exit 0
  fi

  arch=$(uname -m)
  [ "$arch" = arm64 ] || fail "The macOS app is built for Apple silicon (arm64); this Mac is $arch."
  major=$(sw_vers -productVersion 2> /dev/null | cut -d. -f1)
  [ "${major:-13}" -ge 13 ] 2> /dev/null || fail "Tunnelbird needs macOS 13 or later."
  for tool in curl hdiutil shasum; do
    command -v "$tool" > /dev/null 2>&1 || need "$tool is required and was not found."
  done

  say "Installing Tunnelbird"
  note "System: macOS $(sw_vers -productVersion 2> /dev/null) ($arch)"
  [ "$dry_run" = true ] && note "Dry run: nothing will be changed."
  tmp=$(mktemp -d)
  trap 'hdiutil detach -quiet "$tmp/disk" 2> /dev/null || true; rm -rf "$tmp"' EXIT

  say "1/4 Finding the latest release"
  run curl -fsSL "$DOWNLOAD/SHA256SUMS" -o "$tmp/SHA256SUMS"
  if [ "$dry_run" = true ]; then
    name="Tunnelbird-VERSION-$arch.dmg"
  else
    name=$(sed -n "s/^[0-9a-f]\{64\}  \(Tunnelbird-[0-9][^ ]*-$arch\.dmg\)\$/\1/p" "$tmp/SHA256SUMS" | head -n1)
    [ -n "$name" ] || fail "No macOS app in the latest release: see $SITE/install/"
  fi
  note "Disk image: $name"
  say "2/4 Downloading"
  run curl -fSL -o "$tmp/$name" "$DOWNLOAD/$name"
  say "3/4 Checking the download"
  note "It must match the checksum the release published."
  if [ "$dry_run" = true ]; then
    printf '  $ (cd %s && grep %s SHA256SUMS | shasum -a 256 -c -)\n' "$tmp" "$name"
  else
    (cd "$tmp" && grep " $name\$" SHA256SUMS | shasum -a 256 -c -) || fail "The download does not match its checksum."
  fi
  say "4/4 Installing in $APPS"
  note "Your home folder only: no administrator. Downloaded here, not in a browser, the app is not blocked by Gatekeeper."
  run mkdir -p "$APPS"
  run hdiutil attach -quiet -nobrowse -readonly -mountpoint "$tmp/disk" "$tmp/$name"
  run rm -rf "$APPS/$APP_NAME"
  run cp -R "$tmp/disk/$APP_NAME" "$APPS/"
  run hdiutil detach -quiet "$tmp/disk"
  say "Done"
  if [ "$dry_run" = true ]; then
    note "Nothing was changed (dry run)."
  else
    note "Open Tunnelbird from $APPS (or Launchpad). The first time, it offers to install its helper:"
    note "that asks for your administrator's password, once."
    note "To update, run the same command again."
  fi
  exit 0
fi

# --- Uninstall ---------------------------------------------------------------

if [ "$uninstall" = true ]; then
  say "Removing Tunnelbird (the self-contained package)"
  note "Installed with APT or DNF? Remove it with: sudo apt remove tunnelbird, or: sudo dnf remove tunnelbird"
  [ "$dry_run" = true ] && note "Dry run: nothing will be changed."
  run rm -rf "$DEST"
  run rm -f "$BIN/tunnelbird" "$DATA/applications/$APP_ID.desktop" \
    "$DATA/icons/hicolor/scalable/apps/$APP_ID.svg" \
    "$DATA/icons/hicolor/symbolic/apps/$APP_ID-symbolic.svg"
  note "Your tunnels stay in your system's network settings until you delete them."
  exit 0
fi

# --- What is this system? ----------------------------------------------------

if [ -r /etc/os-release ]; then
  # shellcheck disable=SC1091
  . /etc/os-release
fi
ids=" ${ID:-} ${ID_LIKE:-} "
arch=$(uname -m)
case $arch in
  x86_64|aarch64) ;;
  *) fail "There is no package for $arch (x86_64 and aarch64 only)." ;;
esac

if [ -z "$method" ]; then
  case $ids in
    *" debian "*|*" ubuntu "*) method=apt ;;
    *" fedora "*|*" rhel "*|*" centos "*) method=dnf ;;
    *) method=bundle ;;
  esac
  # The repository comes after the first release: until it answers, use the package.
  case $method in
    apt) published "$SITE/apt/dists/stable/Release" || { method=bundle; repo_missing=true; } ;;
    dnf) published "$SITE/rpm/$arch/repodata/repomd.xml" || { method=bundle; repo_missing=true; } ;;
  esac
fi

say "Installing Tunnelbird"
note "System: ${PRETTY_NAME:-unknown Linux} ($arch)"
note "Method: $method"
[ "${repo_missing:-false}" = true ] && note "The APT/DNF repository is not published yet: installing the self-contained package."
[ "$dry_run" = true ] && note "Dry run: nothing will be changed."

# --- Requirements ------------------------------------------------------------

if command -v systemctl > /dev/null 2>&1 && systemctl is-active --quiet NetworkManager 2>/dev/null; then
  note "NetworkManager: running"
else
  note "Warning: NetworkManager does not seem to manage the network here."
  note "Tunnelbird hands tunnels to NetworkManager: without it, it cannot connect."
fi

if [ "$method" = bundle ]; then
  for tool in curl tar bash; do
    command -v "$tool" > /dev/null 2>&1 || need "$tool is required and was not found."
  done
  command -v sha256sum > /dev/null 2>&1 || need "sha256sum is required and was not found."
  libc=$(glibc_version)
  if [ -z "$libc" ]; then
    need "This system does not use glibc (Alpine, for instance): the package cannot run here."
  elif [ "$(printf '%s\n%s\n' "$libc" "$GLIBC_MIN" | sort -V | head -n1)" != "$GLIBC_MIN" ]; then
    need "The package needs the C library $GLIBC_MIN or newer, this system has $libc."
  else
    note "C library: $libc"
  fi
  [ -r /usr/share/mime/mime.cache ] || note "Warning: shared-mime-info is missing (package of that name): without it the icons do not show."
  command -v fc-list > /dev/null 2>&1 || note "Warning: fontconfig is missing: the text could not be drawn."
fi

# --- Install -----------------------------------------------------------------

case $method in
  apt)
    command -v apt-get > /dev/null 2>&1 || fail "apt-get not found: this system does not use APT"
    say "1/3 Trusting Tunnelbird's signing key"
    note "Packages are signed; the key only vouches for Tunnelbird's repository."
    root install -d -m 755 /etc/apt/keyrings
    key=$(mktemp)
    trap 'rm -f "$key"' EXIT
    run curl -fsSL "$SITE/tunnelbird.gpg" -o "$key"
    root install -m 644 "$key" "$KEYRING"
    say "2/3 Adding the repository"
    write_root "$APT_SOURCE" <<EOF
Types: deb
URIs: $SITE/apt
Suites: stable
Components: main
Signed-By: $KEYRING
EOF
    say "3/3 Installing"
    root apt-get update
    root apt-get install -y tunnelbird
    ;;

  dnf)
    command -v dnf > /dev/null 2>&1 || fail "dnf not found: this system does not use DNF"
    say "1/2 Adding the repository (packages and index are signed)"
    write_root "$DNF_REPO" <<EOF
[tunnelbird]
name=Tunnelbird
baseurl=$SITE/rpm/\$basearch
enabled=1
gpgcheck=1
repo_gpgcheck=1
gpgkey=$SITE/tunnelbird.gpg
EOF
    say "2/2 Installing"
    root dnf install -y tunnelbird
    ;;

  bundle)
    tmp=$(mktemp -d)
    trap 'rm -rf "$tmp"' EXIT
    say "1/4 Finding the latest release"
    run curl -fsSL "$DOWNLOAD/SHA256SUMS" -o "$tmp/SHA256SUMS"
    if [ "$dry_run" = true ]; then
      name="tunnelbird-VERSION-linux-$arch.tar.gz"
    else
      name=$(sed -n "s/^[0-9a-f]\{64\}  \(tunnelbird-[0-9][^ ]*-linux-$arch\.tar\.gz\)\$/\1/p" "$tmp/SHA256SUMS" | head -n1)
      [ -n "$name" ] || fail "No package for $arch in the latest release: see $SITE/install/"
    fi
    note "Package: $name"
    say "2/4 Downloading"
    run curl -fSL -o "$tmp/$name" "$DOWNLOAD/$name"
    say "3/4 Checking the download"
    note "It must match the checksum the release published."
    if [ "$dry_run" = true ]; then
      printf '  $ (cd %s && grep %s SHA256SUMS | sha256sum -c -)\n' "$tmp" "$name"
    else
      (cd "$tmp" && grep " $name\$" SHA256SUMS | sha256sum -c -) || fail "The download does not match its checksum."
    fi
    say "4/4 Installing in $DEST"
    note "Your home folder only: no administrator, nothing outside it."
    run mkdir -p "$DATA/applications" "$DATA/icons/hicolor/scalable/apps" "$DATA/icons/hicolor/symbolic/apps" "$BIN"
    run tar -xzf "$tmp/$name" -C "$tmp"
    run rm -rf "$DEST"
    run mv "$tmp/tunnelbird" "$DEST"
    # A launcher, not a link: AppRun finds its files from the path it is started by.
    write_user "$BIN/tunnelbird" <<EOF
#!/bin/sh
exec "$DEST/AppRun" "\$@"
EOF
    run chmod +x "$BIN/tunnelbird"
    run cp "$DEST/usr/share/icons/hicolor/scalable/apps/$APP_ID.svg" "$DATA/icons/hicolor/scalable/apps/"
    run cp "$DEST/usr/share/icons/hicolor/symbolic/apps/$APP_ID-symbolic.svg" "$DATA/icons/hicolor/symbolic/apps/"
    if [ "$dry_run" = true ]; then
      write_user "$DATA/applications/$APP_ID.desktop" <<EOF
(the menu entry, launching $DEST/AppRun)
EOF
    else
      sed "s|^Exec=.*|Exec=$DEST/AppRun %F|" "$DEST/usr/share/applications/$APP_ID.desktop" \
        | write_user "$DATA/applications/$APP_ID.desktop"
    fi
    command -v update-desktop-database > /dev/null 2>&1 && run update-desktop-database "$DATA/applications"
    # What this system must provide: ldd names the libraries the package does not carry.
    if [ "$dry_run" = false ]; then
      missing=$(ldd "$DEST/usr/bin/tunnelbird" 2> /dev/null | sed -n 's/^[[:space:]]*\([^ ]*\) => not found$/\1/p' | sort -u | tr '\n' ' ')
      if [ -n "$missing" ]; then
        note "Warning: this system lacks libraries that every desktop has: $missing"
        note "Install them with your package manager, then start Tunnelbird."
      fi
    fi
    ;;

  *) fail "Unknown method $method (apt, dnf or bundle)" ;;
esac

say "Done"
if [ "$dry_run" = true ]; then
  note "Nothing was changed (dry run)."
else
  note "Open Tunnelbird from your applications."
  case $method in
    apt|dnf) note "Updates come with your system updates." ;;
    bundle)
      note "To update, run the same command again."
      case ":$PATH:" in
        *":$BIN:"*) ;;
        *) note "To start it from a terminal, add $BIN to your PATH." ;;
      esac ;;
  esac
fi
