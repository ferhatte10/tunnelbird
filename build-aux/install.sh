#!/bin/sh
# Tunnelbird installer: https://tunnelbird.ferhatsn.fr/install.sh
#
#   curl -fsSL https://tunnelbird.ferhatsn.fr/install.sh | sh
#   curl -fsSL https://tunnelbird.ferhatsn.fr/install.sh | sh -s -- --dry-run
#
# Picks the best way for this system and says each step before doing it:
#   - Ubuntu, Debian, Mint, Pop!_OS…: Tunnelbird's signed APT repository, so
#     updates arrive with the system updates;
#   - Fedora, RHEL-like…: Tunnelbird's signed DNF repository, same;
#   - anything else: the Flatpak from Flathub.
# Administrator rights (sudo) are asked only for the steps that need them.
#
# Options:
#   --dry-run          print the steps, change nothing
#   --method NAME      force apt, dnf or flatpak
#   --help
#
# Source: https://github.com/ferhatte10/tunnelbird/blob/main/build-aux/install.sh
set -eu

SITE=https://tunnelbird.ferhatsn.fr
APP_ID=fr.ferhatsn.Tunnelbird
KEYRING=/etc/apt/keyrings/tunnelbird.gpg
APT_SOURCE=/etc/apt/sources.list.d/tunnelbird.sources
DNF_REPO=/etc/yum.repos.d/tunnelbird.repo

dry_run=false
method=""

say() { printf '\n\033[1m%s\033[0m\n' "$*"; }
note() { printf '  %s\n' "$*"; }
fail() { printf '\n\033[31mError:\033[0m %s\n' "$*" >&2; exit 1; }

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

while [ $# -gt 0 ]; do
  case $1 in
    --dry-run) dry_run=true ;;
    --method) shift; method=${1:-} ;;
    --help|-h)
      echo "Usage: install.sh [--dry-run] [--method apt|dnf|flatpak]"
      echo "Installs Tunnelbird the best way for this system. See $SITE"
      exit 0 ;;
    *) fail "Unknown option $1 (see --help)" ;;
  esac
  shift
done

[ "$(uname -s)" = Linux ] || fail "Tunnelbird for $(uname -s) is not available yet: see $SITE"

# --- What is this system? ----------------------------------------------------

if [ -r /etc/os-release ]; then
  # shellcheck disable=SC1091
  . /etc/os-release
fi
ids=" ${ID:-} ${ID_LIKE:-} "
arch=$(uname -m)

if [ -z "$method" ]; then
  case $ids in
    *" debian "*|*" ubuntu "*) method=apt ;;
    *" fedora "*|*" rhel "*|*" centos "*) method=dnf ;;
    *) method=flatpak ;;
  esac
  # Packages exist for these two processors only; others use the Flatpak.
  case $arch in
    x86_64|aarch64) ;;
    *) method=flatpak ;;
  esac
fi

say "Installing Tunnelbird"
note "System: ${PRETTY_NAME:-unknown Linux} ($arch)"
note "Method: $method"
[ "$dry_run" = true ] && note "Dry run: nothing will be changed."

# --- Requirements ------------------------------------------------------------

if command -v systemctl > /dev/null 2>&1 && systemctl is-active --quiet NetworkManager 2>/dev/null; then
  note "NetworkManager: running"
else
  note "Warning: NetworkManager does not seem to manage the network here."
  note "Tunnelbird hands tunnels to NetworkManager: without it, it cannot connect."
fi

# --- Install -----------------------------------------------------------------

case $method in
  apt)
    command -v apt-get > /dev/null 2>&1 || fail "apt-get not found: try --method flatpak"
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
    command -v dnf > /dev/null 2>&1 || fail "dnf not found: try --method flatpak"
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

  flatpak)
    if ! command -v flatpak > /dev/null 2>&1; then
      case $ids in
        *" arch "*) get_flatpak="sudo pacman -S flatpak" ;;
        *" suse "*|*" opensuse "*) get_flatpak="sudo zypper install flatpak" ;;
        *" debian "*|*" ubuntu "*) get_flatpak="sudo apt install flatpak" ;;
        *" fedora "*|*" rhel "*) get_flatpak="sudo dnf install flatpak" ;;
        *) get_flatpak="" ;;
      esac
      message="Flatpak is not installed. Install it first${get_flatpak:+ ($get_flatpak)}, see https://flathub.org/setup, then run this again."
      [ "$dry_run" = true ] || fail "$message"
      note "$message"
    fi
    say "1/2 Adding Flathub (for this user)"
    run flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
    say "2/2 Installing"
    run flatpak install --user -y flathub "$APP_ID"
    ;;

  *) fail "Unknown method $method (apt, dnf or flatpak)" ;;
esac

say "Done"
if [ "$dry_run" = true ]; then
  note "Nothing was changed (dry run)."
else
  note "Open Tunnelbird from your applications."
  case $method in
    apt|dnf) note "Updates come with your system updates." ;;
    flatpak) note "Updates come through Flathub (Software, or: flatpak update)." ;;
  esac
fi
