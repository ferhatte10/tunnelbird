#!/bin/zsh
# Builds apple/build/Tunnelbird-VERSION-ARCH.dmg: the app and a link to
# Applications to drop it on.
set -e
cd "${0:A:h}"
./build.sh release
VERSION=$(/usr/libexec/PlistBuddy -c 'Print CFBundleShortVersionString' Resources/Info.plist)
DMG="build/Tunnelbird-$VERSION-$(uname -m).dmg"
STAGE=$(mktemp -d)
trap 'rm -rf "$STAGE"' EXIT
cp -R build/Tunnelbird.app "$STAGE/"
ln -s /Applications "$STAGE/Applications"
rm -f "$DMG"
hdiutil create -quiet -volname Tunnelbird -srcfolder "$STAGE" -ov -format UDZO "$DMG"
print "Built $DMG"
