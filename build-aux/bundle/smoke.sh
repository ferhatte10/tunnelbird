#!/bin/sh
# Starts a built self-contained package in demo mode (no NetworkManager, no
# network) in a virtual screen, and checks that it stays up with no error.
#
#   build-aux/bundle/smoke.sh target/bundle/tunnelbird-X.Y.Z-linux-ARCH.tar.gz
set -eu
tarball=${1:?usage: smoke.sh PACKAGE.tar.gz}
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
tar -xzf "$tarball" -C "$work"

status=0
GSK_RENDERER=cairo TUNNELBIRD_DEMO=en \
  xvfb-run -a timeout 10 "$work/tunnelbird/AppRun" > "$work/log" 2>&1 || status=$?
# 124: still running when the timeout stopped it, which is what we want.
[ "$status" -eq 124 ] || { cat "$work/log"; echo "smoke: exit $status, expected it to keep running" >&2; exit 1; }
if grep -iE 'critical|error|cannot open|not found' "$work/log"; then
  echo "smoke: errors in the log" >&2
  exit 1
fi
echo "smoke: ok"
