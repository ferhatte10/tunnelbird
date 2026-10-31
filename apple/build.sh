#!/bin/zsh
# Builds apple/build/Tunnelbird.app. Needs only the command line tools.
# Usage: build.sh [debug|release]
set -e
cd "${0:A:h}"
CONFIG=${1:-debug}
# The command line tools' SDK 27 needs the SwiftUI macros, which only come with
# Xcode (“plugin for module 'SwiftUIMacros' not found”): use SDK 26 when it is there.
SDK26=/Library/Developer/CommandLineTools/SDKs/MacOSX26.sdk
if [[ -z $SDKROOT && -d $SDK26 ]]; then
  export SDKROOT=$SDK26
fi
# The app compares this version with the service's, to offer to update it.
APP_VERSION=$(/usr/libexec/PlistBuddy -c 'Print CFBundleShortVersionString' Resources/Info.plist)
SERVICE_VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' ../crates/service/Cargo.toml | head -1)
if [[ $APP_VERSION != $SERVICE_VERSION ]]; then
  print "Versions differ: app $APP_VERSION, service $SERVICE_VERSION" >&2
  exit 1
fi
swift build -c "$CONFIG"
APP=build/Tunnelbird.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp ".build/$CONFIG/Tunnelbird" "$APP/Contents/MacOS/Tunnelbird"
cp Resources/Info.plist "$APP/Contents/Info.plist"
cp Resources/AppIcon.icns Resources/tray-*.png "$APP/Contents/Resources/"
cp -R Resources/en.lproj Resources/fr.lproj "$APP/Contents/Resources/"
mkdir -p "$APP/Contents/Resources/Licenses"
cp ../LICENSE "$APP/Contents/Resources/Licenses/Tunnelbird.txt"
cp ../vendor/boringtun/LICENSE "$APP/Contents/Resources/Licenses/BoringTun.txt"
# The service, which the app installs with the administrator's password.
(cd .. && cargo build --release -p tunnelbird-service)
cp ../target/release/tunnelbird-service "$APP/Contents/Resources/tunnelbird-service"
./bundle-openvpn.sh "$APP/Contents/Resources"
# Ad hoc signature: no account needed, and Apple silicon refuses unsigned code.
codesign --force --sign - "$APP"
print "Built $APP"
