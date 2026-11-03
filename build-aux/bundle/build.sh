#!/bin/sh
# Builds the self-contained package: target/bundle/tunnelbird-VERSION-linux-ARCH.tar.gz.
#
# GTK 4 and libadwaita travel inside it, so it runs on distributions that have
# neither or have them too old (anything with a C library as recent as the
# machine that builds it: the release is built on Ubuntu 24.04, glibc 2.39).
# NetworkManager still has to manage the network: it is not part of the package.
#
# Tools (downloaded once, verified against the digests below, never run before):
#   linuxdeploy       collects the libraries the program needs
#   linuxdeploy-plugin-gtk   adds GTK's modules, pixbuf loaders and schemas
#
#   build-aux/bundle/build.sh [--tools DIR]
set -eu

LINUXDEPLOY_VERSION=1-alpha-20251107-1
LINUXDEPLOY_SHA256_x86_64=c20cd71e3a4e3b80c3483cef793cda3f4e990aca14014d23c544ca3ce1270b4d
LINUXDEPLOY_SHA256_aarch64=620095110d693282b8ebeb244a95b5e911cf8f65f76c88b4b47d16ae6346fcff
PLUGIN_COMMIT=3b67a1d1c1b0c8268f57f2bce40fe2d33d409cea
PLUGIN_SHA256=b0f4cbc684a0103a9651f0955b635eaea0096b3a66c0f5a2c2aa337960375171

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
out=$root/target/bundle
tools=$out/tools
arch=$(uname -m)

fail() { printf 'Error: %s\n' "$*" >&2; exit 1; }
say() { printf '==> %s\n' "$*"; }

while [ $# -gt 0 ]; do
  case $1 in
    --tools) shift; tools=${1:?--tools needs a folder} ;;
    *) fail "unknown option $1" ;;
  esac
  shift
done

case $arch in
  x86_64) linuxdeploy_sha=$LINUXDEPLOY_SHA256_x86_64 ;;
  aarch64) linuxdeploy_sha=$LINUXDEPLOY_SHA256_aarch64 ;;
  *) fail "no package for $arch" ;;
esac

# fetch FILE URL SHA256: downloads when missing, then checks the digest.
fetch() {
  if [ ! -f "$1" ]; then
    mkdir -p "$(dirname "$1")"
    curl -fsSL -o "$1.part" "$2"
    mv "$1.part" "$1"
  fi
  printf '%s  %s\n' "$3" "$1" | sha256sum -c --quiet - || fail "$1 does not match its pinned digest"
  chmod +x "$1"
}

say "Tools"
mkdir -p "$tools"
fetch "$tools/linuxdeploy-$arch.AppImage" \
  "https://github.com/linuxdeploy/linuxdeploy/releases/download/$LINUXDEPLOY_VERSION/linuxdeploy-$arch.AppImage" \
  "$linuxdeploy_sha"
fetch "$tools/linuxdeploy-plugin-gtk.sh" \
  "https://raw.githubusercontent.com/linuxdeploy/linuxdeploy-plugin-gtk/$PLUGIN_COMMIT/linuxdeploy-plugin-gtk.sh" \
  "$PLUGIN_SHA256"

version=$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -n1)
app_id=fr.ferhatsn.Tunnelbird
appdir=$out/tunnelbird
rm -rf "$appdir"

say "Building Tunnelbird $version"
make -C "$root" > /dev/null
make -C "$root" install PREFIX=/usr DESTDIR="$appdir" > /dev/null

say "Collecting GTK and the libraries"
(
  cd "$out"
  PATH="$tools:$PATH" DEPLOY_GTK_VERSION=4 NO_STRIP=1 APPIMAGE_EXTRACT_AND_RUN=1 \
    "$tools/linuxdeploy-$arch.AppImage" --appdir "$appdir" \
    --executable "$appdir/usr/bin/tunnelbird" \
    --desktop-file "$appdir/usr/share/applications/$app_id.desktop" \
    --icon-file "$appdir/usr/share/icons/hicolor/scalable/apps/$app_id.svg" \
    --plugin gtk > "$out/linuxdeploy.log" 2>&1
) || { tail -20 "$out/linuxdeploy.log" >&2; fail "linuxdeploy failed (log: $out/linuxdeploy.log)"; }

# The modules GTK loads when it needs them (SVG and image loaders, input methods) sit in
# sub-folders and have no search path of their own: without it they cannot find the
# libraries next to the program. LD_LIBRARY_PATH would fix that, but every program the
# app starts (journalctl, xdg-open…) would inherit it.
command -v patchelf > /dev/null || fail "install patchelf first"
find "$appdir/usr/lib" -mindepth 2 -type f -name '*.so*' | while read -r module; do
  patchelf --set-rpath "\$ORIGIN/$(realpath --relative-to="$(dirname "$module")" "$appdir/usr/lib")" "$module"
done

# The plugin's own start-up file forces an old GTK theme (which switches
# libadwaita's style off), X11 and its own settings: its content is replaced by ours
# (under the same name: AppRun sources that file).
say "Start-up file and settings"
cache=$(find "$appdir/usr/lib/gdk-pixbuf-2.0" -name loaders.cache | head -n1)
[ -n "$cache" ] || fail "no pixbuf loaders collected"
# GTK reads the loaders' absolute paths from this file, and the package moves: the
# paths are filled in when the program starts (see the start-up file below).
modules=${cache%/*}
sed 's#^"\(libpixbufloader[^"]*\)"#"@APPDIR@/'"${modules#"$appdir"/}"'/loaders/\1"#' "$cache" > "$cache.in"
rm "$cache"
cat > "$appdir/apprun-hooks/linuxdeploy-plugin-gtk.sh" << EOF
#! /usr/bin/env bash
export APPDIR="\${APPDIR:-"\$(dirname "\$(realpath "\$0")")"}"
export XDG_DATA_DIRS="\$APPDIR/usr/share:\${XDG_DATA_DIRS:-/usr/local/share:/usr/share}"
export GTK_EXE_PREFIX="\$APPDIR/usr"
export GTK_PATH="\$APPDIR/usr/lib/gtk-4.0"
pixbuf_cache="\${XDG_CACHE_HOME:-\$HOME/.cache}/tunnelbird"
[ -w "\$HOME" ] || pixbuf_cache="\${TMPDIR:-/tmp}/tunnelbird-\$(id -u)"
pixbuf_cache="\$pixbuf_cache/pixbuf-\$(printf %s "\$APPDIR" | cksum | cut -d' ' -f1).cache"
if [ ! -s "\$pixbuf_cache" ]; then
  mkdir -p "\${pixbuf_cache%/*}"
  sed "s#@APPDIR@#\$APPDIR#g" "\$APPDIR/${cache#"$appdir"/}.in" > "\$pixbuf_cache.\$\$" \\
    && mv "\$pixbuf_cache.\$\$" "\$pixbuf_cache"
fi
export GDK_PIXBUF_MODULE_FILE="\$pixbuf_cache"
EOF
chmod +x "$appdir/apprun-hooks/linuxdeploy-plugin-gtk.sh"

# The schemas of this machine's desktop (dock, shell, terminal…) are not
# Tunnelbird's: only GTK's and the system settings it reads stay.
schemas=$appdir/usr/share/glib-2.0/schemas
rm -f "$schemas"/*.override
glib-compile-schemas "$schemas"

# The icons the window draws (list, edit, delete…) come from the Adwaita theme;
# a KDE or minimal system may not have it. The user's own theme still wins.
# Only the symbolic ones: they are what the interface uses.
[ -d /usr/share/icons/Adwaita/symbolic ] || fail "install adwaita-icon-theme first"
mkdir -p "$appdir/usr/share/icons/Adwaita"
cp /usr/share/icons/Adwaita/index.theme "$appdir/usr/share/icons/Adwaita/"
cp -a /usr/share/icons/Adwaita/symbolic "$appdir/usr/share/icons/Adwaita/"
# SVG icons need the pixbuf loader of librsvg, which the plugin must have collected.
ls "$appdir"/usr/lib/gdk-pixbuf-2.0/*/loaders/libpixbufloader*svg* > /dev/null 2>&1 \
  || fail "no SVG loader collected: install librsvg2-common first"

say "Packing"
tarball=$out/tunnelbird-$version-linux-$arch.tar.gz
tar --owner=0 --group=0 --numeric-owner --sort=name -czf "$tarball" -C "$out" tunnelbird
say "Done: $tarball ($(du -h "$tarball" | cut -f1))"
