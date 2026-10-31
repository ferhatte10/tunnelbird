// swift-tools-version:5.9
// SPDX-License-Identifier: GPL-3.0-or-later
// Built with the free command line tools: `apple/build.sh` makes the app bundle.
import PackageDescription

let package = Package(
    name: "Tunnelbird",
    platforms: [.macOS(.v13)],
    targets: [.executableTarget(name: "Tunnelbird", path: "Sources/Tunnelbird")]
)
