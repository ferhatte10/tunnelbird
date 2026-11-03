#!/bin/bash
# Checks a built disk image the way a user's Mac would meet it: the app is in it, signed, and
# carries OpenVPN with its libraries (nothing is read from Homebrew's folders).
# Usage: smoke.sh apple/build/Tunnelbird-VERSION-arm64.dmg
set -euo pipefail
DMG=$1
MOUNT=$(mktemp -d)
trap 'hdiutil detach -quiet "$MOUNT" 2> /dev/null || true; rmdir "$MOUNT" 2> /dev/null || true' EXIT
hdiutil attach -quiet -nobrowse -readonly -mountpoint "$MOUNT" "$DMG"
APP=$MOUNT/Tunnelbird.app
RES=$APP/Contents/Resources

test -d "$APP" || { echo "no Tunnelbird.app in the disk image" >&2; exit 1; }
test -L "$MOUNT/Applications" || { echo "no Applications link in the disk image" >&2; exit 1; }
codesign --verify --deep --strict "$APP"
echo "signature: ok"

for file in "$APP/Contents/MacOS/Tunnelbird" "$RES/tunnelbird-service" "$RES/openvpn/openvpn" "$RES"/openvpn/lib/*; do
  outside=$(otool -L "$file" | sed 1d | awk '{print $1}' | grep -E '^/opt/homebrew|^/usr/local' || true)
  if [ -n "$outside" ]; then
    echo "$file still needs: $outside" >&2
    exit 1
  fi
done
echo "libraries: all inside the app"

"$RES/openvpn/openvpn" --version | head -n1
for license in Tunnelbird BoringTun; do
  test -s "$RES/Licenses/$license.txt" || { echo "missing license $license" >&2; exit 1; }
done
echo "licenses: ok"
