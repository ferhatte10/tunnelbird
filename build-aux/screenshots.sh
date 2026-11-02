#!/bin/sh
# Takes the store screenshots (data/screenshots/<language>/*.png) with the demo
# mode, inside an invisible GNOME Shell: nothing shows on screen, and the
# shell runs fully apart from the desktop session (own D-Bus, runtime folder,
# home and settings, no extensions), with GNOME's default look as Flathub asks.
#
# Each screenshot is GNOME's own window capture: title bar, rounded corners
# and shadow included, nothing edited afterwards.
#
# Usage: build-aux/screenshots.sh [binary]   (default: target/debug/tunnelbird)
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
binary=$(realpath "${1:-$root/target/debug/tunnelbird}")
out="$root/data/screenshots"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/home" "$work/runtime" "$out"
chmod 700 "$work/runtime"

# GNOME's own defaults, without the distribution's (Ubuntu turns the accent
# orange and changes the fonts): the schemas compiled without any override.
mkdir -p "$work/schemas"
cp /usr/share/glib-2.0/schemas/*.gschema.xml "$work/schemas/"
glib-compile-schemas "$work/schemas"

# Twice the pixels, for HiDPI store pages (Flathub: 2000x1400 at most).
mkdir -p "$work/home/.config/glib-2.0/settings"
printf "[org/gnome/desktop/interface]\nscaling-factor=uint32 2\naccent-color='blue'\n" \
  > "$work/home/.config/glib-2.0/settings/keyfile"

# GNOME's default font, from the GNOME Flatpak runtime when it is installed.
for fonts in /var/lib/flatpak/runtime/org.gnome.Platform/*/*/active/files/share/fonts/Adwaita \
  "$HOME"/.local/share/flatpak/runtime/org.gnome.Platform/*/*/active/files/share/fonts/Adwaita; do
  if [ -d "$fonts" ]; then
    mkdir -p "$work/home/.local/share/fonts"
    ln -s "$fonts" "$work/home/.local/share/fonts/Adwaita"
    break
  fi
done
[ -e "$work/home/.local/share/fonts/Adwaita" ] ||
  echo "Adwaita fonts not found (install the GNOME Flatpak runtime): the font will differ" >&2

# Runs inside the private session below.
cat > "$work/session.sh" <<'EOF'
set -eu
display=tunnelbird-screenshots
gnome-shell --unsafe-mode --headless --wayland --no-x11 --mode=user \
  --virtual-monitor 2800x2000 --wayland-display "$display" >"$WORK/shell.log" 2>&1 &
shell=$!
trap 'kill $shell 2>/dev/null || true' EXIT
sleep 6

for lang in en fr; do
  case $lang in en) locale=en_US.UTF-8 ;; fr) locale=fr_FR.UTF-8 ;; esac
  mkdir -p "$OUT/$lang"
  LC_ALL=$locale WAYLAND_DISPLAY=$display GDK_BACKEND=wayland \
    TUNNELBIRD_DEMO=$lang TUNNELBIRD_SCREENSHOTS="$OUT/$lang" \
    TUNNELBIRD_CAPTURE_COMMAND="$WORK/capture.sh" "$BINARY"
done
EOF

# Called by the app (demo mode) for each screenshot: capture.sh <file.png>.
cat > "$work/capture.sh" <<'EOF'
#!/bin/sh
result=$(gdbus call --session --dest org.gnome.Shell --object-path /org/gnome/Shell \
  --method org.gnome.Shell.Eval "
    const {Gio, Shell} = imports.gi;
    const window = global.display.list_all_windows()
      .find(w => w.get_gtk_application_id() === 'fr.ferhatsn.Tunnelbird');
    const texture = window.get_compositor_private().paint_to_content(null).get_texture();
    const stream = Gio.File.new_for_path('$1').replace(null, false, 0, null);
    Shell.Screenshot.composite_to_stream(texture, 0, 0, -1, -1, 1, null, 0, 0, 1, stream)
      .then(() => stream.close(null));
    window.get_title()")
case $result in
  "(true,"*) ;;
  *) echo "GNOME Shell refused the capture: $result" >&2; exit 1 ;;
esac
EOF
chmod +x "$work/capture.sh"

env -i PATH="$PATH" HOME="$work/home" XDG_RUNTIME_DIR="$work/runtime" \
  XDG_CONFIG_HOME="$work/home/.config" XDG_DATA_HOME="$work/home/.local/share" \
  XDG_CURRENT_DESKTOP=GNOME GSETTINGS_BACKEND=keyfile GSETTINGS_SCHEMA_DIR="$work/schemas" \
  WORK="$work" OUT="$out" BINARY="$binary" \
  dbus-run-session -- sh "$work/session.sh"

echo "Screenshots in $out"
