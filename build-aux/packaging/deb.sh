#!/bin/sh
# Builds the .deb package (Ubuntu, Debian, Mint, Pop!_OS…) for this machine's
# architecture, from a release build. Run from the repository root:
#
#   build-aux/packaging/deb.sh        → target/packages/tunnelbird_<version>_<arch>.deb
#
# The dependencies are read from the binary (dpkg-shlibdeps), so they match
# the system it was built on. The CI builds on the oldest supported system,
# Ubuntu 24.04 (docs/plans/ci-and-release.md). Checked: a package built on
# Ubuntu 26.04 installs and starts on a clean Ubuntu 24.04.
set -eu

cd "$(dirname "$0")/../.."
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)
arch=$(dpkg --print-architecture)
stage=target/packages/deb-root
out=target/packages/tunnelbird_${version}_${arch}.deb

make all PREFIX=/usr
rm -rf "$stage"
make install PREFIX=/usr DESTDIR="$stage"
strip --strip-unneeded "$stage/usr/bin/tunnelbird"
install -Dm644 LICENSE "$stage/usr/share/doc/tunnelbird/copyright"

# Library dependencies read from the binary itself, with the versions its
# symbols need (what Debian's own packages do).
shlibs_dir=$(mktemp -d)
mkdir -p "$shlibs_dir/debian"
printf 'Source: tunnelbird\n\nPackage: tunnelbird\nArchitecture: any\n' > "$shlibs_dir/debian/control"
binary=$(realpath "$stage/usr/bin/tunnelbird")
libraries=$(cd "$shlibs_dir" && dpkg-shlibdeps -O -e "$binary" 2>/dev/null | sed -n 's/^shlibs:Depends=//p')
rm -rf "$shlibs_dir"
[ -n "$libraries" ] || { echo "Could not compute library dependencies" >&2; exit 1; }

mkdir -p "$stage/DEBIAN"
size=$(du -sk --exclude=DEBIAN "$stage" | cut -f1)
cat > "$stage/DEBIAN/control" <<EOF
Package: tunnelbird
Version: $version
Architecture: $arch
Maintainer: Ferhat Saidoun <ferhatsnpro@gmail.com>
Installed-Size: $size
Depends: $libraries, network-manager (>= 1.16)
Recommends: network-manager-openvpn-gnome, iso-codes
Section: net
Priority: optional
Homepage: https://tunnelbird.ferhatsn.fr
Description: Connect to your VPN in one click
 Tunnelbird imports the .conf (WireGuard) or .ovpn (OpenVPN) file from your
 VPN provider or your own server and connects in one click, through
 NetworkManager, without administrator rights. Live traffic graph, public
 address and location, automatic connection on untrusted Wi-Fi, top bar icon,
 sharing as a file or a QR code.
EOF

mkdir -p target/packages
dpkg-deb --root-owner-group --build "$stage" "$out" >/dev/null
echo "$out"
