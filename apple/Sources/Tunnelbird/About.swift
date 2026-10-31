// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

import AppKit
import SwiftUI

let aboutWindowID = "about"

enum Links {
    static let website = URL(string: "https://tunnelbird.ferhatsn.fr")
    static let documentation = URL(string: "https://tunnelbird.ferhatsn.fr/docs")
    static let compatibility = URL(string: "https://tunnelbird.ferhatsn.fr/compatibility")
    static let privacy = URL(string: "https://tunnelbird.ferhatsn.fr/privacy")
    static let changelog = URL(string: "https://tunnelbird.ferhatsn.fr/changelog")
    static let source = URL(string: "https://github.com/ferhatte10/tunnelbird")
    static let issues = URL(string: "https://github.com/ferhatte10/tunnelbird/issues")
    static let authorSite = URL(string: "https://ferhatsn.fr")
    static let authorGitHub = URL(string: "https://github.com/ferhatte10")
    static let contact = URL(string: "mailto:ferhatsnpro@gmail.com")

    static func open(_ url: URL?) {
        if let url { NSWorkspace.shared.open(url) }
    }
}

/// The file names under `Licenses` in the app bundle, with what they cover.
private let licenses: [(name: String, file: String)] = [
    ("Tunnelbird", "Tunnelbird"),
    ("BoringTun", "BoringTun"),
    ("OpenVPN", "OpenVPN"),
    ("OpenSSL", "OpenSSL"),
    ("LZO", "LZO"),
    ("LZ4", "LZ4"),
    ("pkcs11-helper", "pkcs11-helper"),
]

struct AboutView: View {
    @State private var showingLicenses = false

    private var version: String {
        Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? ""
    }

    var body: some View {
        VStack(spacing: 14) {
            Image(nsImage: NSApp.applicationIconImage)
                .resizable()
                .frame(width: 96, height: 96)
                .accessibilityHidden(true)
            VStack(spacing: 2) {
                Text(verbatim: "Tunnelbird").font(.title.weight(.bold))
                Text("Version \(version)").font(.callout).foregroundStyle(.secondary)
                    .textSelection(.enabled)
            }
            Text("Connect to your VPN in one click.")
            VStack(spacing: 4) {
                Text("Created by \(Text(verbatim: "Ferhat Saidoun").fontWeight(.semibold))")
                HStack(spacing: 14) {
                    Link(destination: Links.authorSite ?? URL(fileURLWithPath: "/")) {
                        Text(verbatim: "ferhatsn.fr")
                    }
                    Link("Author on GitHub", destination: Links.authorGitHub ?? URL(fileURLWithPath: "/"))
                }
                .font(.callout)
            }
            Divider()
            VStack(spacing: 4) {
                Text("Free software, under the GNU General Public License, version 3 or later.")
                    .multilineTextAlignment(.center)
                    .fixedSize(horizontal: false, vertical: true)
                Text(verbatim: "© 2026 Ferhat Saidoun")
            }
            .font(.caption)
            .foregroundStyle(.secondary)
            HStack {
                Button("Website") { Links.open(Links.website) }
                Button("Source Code") { Links.open(Links.source) }
                Button("Licenses…") { showingLicenses = true }
            }
        }
        .padding(24)
        .frame(width: 400)
        .sheet(isPresented: $showingLicenses) { LicensesSheet() }
    }
}

struct LicensesSheet: View {
    @Environment(\.dismiss) private var dismiss
    @State private var selected = licenses[0].file

    private var text: String {
        Bundle.main.url(forResource: selected, withExtension: "txt", subdirectory: "Licenses")
            .flatMap { try? String(contentsOf: $0, encoding: .utf8) } ?? ""
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Picker("Licenses", selection: $selected) {
                ForEach(licenses, id: \.file) { Text(verbatim: $0.name).tag($0.file) }
            }
            .labelsHidden()
            ScrollView {
                Text(text)
                    .font(.system(.caption, design: .monospaced))
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(10)
            }
            .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(Color.primary.opacity(0.15)))
            HStack {
                Spacer()
                Button("Close") { dismiss() }
                    .keyboardShortcut(.defaultAction)
            }
        }
        .padding(16)
        .frame(width: 560, height: 480)
    }
}

/// The menus: a Mac app is judged by them too.
struct AppCommands: Commands {
    @ObservedObject var model: Model
    @Environment(\.openWindow) private var openWindow

    var body: some Commands {
        CommandGroup(replacing: .appInfo) {
            Button("About Tunnelbird") {
                openWindow(id: aboutWindowID)
                NSApp.activate(ignoringOtherApps: true)
            }
        }
        CommandGroup(replacing: .newItem) {
            Button("Import Tunnel…") {
                openWindow(id: mainWindowID)
                NSApp.activate(ignoringOtherApps: true)
                model.importRequested = true
            }
            .keyboardShortcut("o")
            .disabled(!model.ready)
        }
        CommandGroup(replacing: .help) {
            Button("Tunnelbird Help") { Links.open(Links.documentation) }
            Button("What's New") { Links.open(Links.changelog) }
            Divider()
            Button("Compatibility") { Links.open(Links.compatibility) }
            Button("Privacy") { Links.open(Links.privacy) }
            Divider()
            Button("Report a Problem…") { Links.open(Links.issues) }
            Button("Contact the Author…") { Links.open(Links.contact) }
            Divider()
            Button("Tunnelbird Website") { Links.open(Links.website) }
        }
    }
}
