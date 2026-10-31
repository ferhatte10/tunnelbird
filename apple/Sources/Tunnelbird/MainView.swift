// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

import AppKit
import SwiftUI
import UniformTypeIdentifiers

struct MainView: View {
    @EnvironmentObject private var model: Model
    @State private var importing = false
    @State private var fileToImport: ImportedFile?
    @State private var pendingImports: [ImportedFile] = []
    @State private var dropTargeted = false
    #if DEBUG
    @State private var snapshotPreferences = false
    @State private var snapshotAbout = false
    #endif

    var body: some View {
        Group {
            if !model.checked {
                Color.clear
            } else if !model.ready {
                UnavailableView()
            } else if model.tunnels.isEmpty {
                EmptyTunnelsView { importing = true }
            } else {
                ScrollView {
                    VStack(alignment: .leading, spacing: 20) {
                        if let connected = model.connected {
                            ActiveCard(tunnel: connected)
                        }
                        TunnelList()
                    }
                    .padding(20)
                }
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .safeAreaInset(edge: .top, spacing: 0) { Banners() }
        .background(Color(nsColor: .windowBackgroundColor))
        .overlay {
            if dropTargeted {
                RoundedRectangle(cornerRadius: 10)
                    .strokeBorder(Color.accentColor, lineWidth: 3)
                    .padding(6)
            }
        }
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Button {
                    importing = true
                } label: {
                    Label("Import Tunnel", systemImage: "plus")
                }
                .help("Import Tunnel")
                .disabled(!model.ready)
            }
            if let version = model.updateAvailable {
                ToolbarItem(placement: .automatic) {
                    Button("Update Available…") { model.openDownloadPage() }
                        .help(Text(verbatim: version))
                }
            }
        }
        .navigationTitle("Tunnelbird")
        .onChange(of: model.importRequested) { requested in
            if requested {
                importing = true
                model.importRequested = false
            }
        }
        #if DEBUG
        .onChange(of: model.tunnels) { tunnels in
            openSnapshotSheet(tunnels)
        }
        #endif
        .fileImporter(isPresented: $importing, allowedContentTypes: importTypes, allowsMultipleSelection: true) { result in
            if case .success(let urls) = result { load(urls) }
        }
        .onDrop(of: [.fileURL], isTargeted: $dropTargeted) { providers in
            guard model.ready, !providers.isEmpty else { return false }
            Task { @MainActor in
                var urls: [URL] = []
                for provider in providers {
                    if let url = await droppedURL(provider) { urls.append(url) }
                }
                load(urls)
            }
            return true
        }
        .sheet(item: $fileToImport, onDismiss: showNextImport) { file in
            ImportSheet(file: file).environmentObject(model)
        }
        .sheet(item: $model.editedTunnel) { tunnel in
            EditSheet(tunnel: tunnel).environmentObject(model)
        }
        .sheet(item: $model.qrTunnel) { tunnel in
            QRSheet(tunnel: tunnel).environmentObject(model)
        }
        #if DEBUG
        .sheet(isPresented: $snapshotPreferences) {
            PreferencesView().environmentObject(model)
        }
        .sheet(isPresented: $snapshotAbout) {
            AboutView()
        }
        #endif
        .sheet(item: $model.logsTunnel) { tunnel in
            LogsSheet(tunnel: tunnel).environmentObject(model)
        }
        .sheet(item: $model.passwordRequest) { request in
            PasswordSheet(request: request).environmentObject(model)
        }
        .alert(item: $model.problem) { problem in
            Alert(title: Text(problem.title), message: Text(problem.details))
        }
    }

    #if DEBUG
    /// With `TUNNELBIRD_SNAPSHOT`: opens a dialog, on fictional data.
    private func openSnapshotSheet(_ tunnels: [Tunnel]) {
        let environment = ProcessInfo.processInfo.environment
        guard environment["TUNNELBIRD_SNAPSHOT"] != nil else { return }
        switch environment["TUNNELBIRD_SNAPSHOT_SHEET"] {
        case "logs":
            if model.logsTunnel == nil, let first = tunnels.first { model.showLogs(first) }
        case "edit":
            if model.editedTunnel == nil { model.editedTunnel = tunnels.first }
        case "qr":
            if model.qrTunnel == nil { model.qrTunnel = tunnels.first }
        case "preferences":
            snapshotPreferences = true
        case "about":
            snapshotAbout = true
        case "import":
            if fileToImport == nil {
                fileToImport = ImportedFile(
                    url: URL(fileURLWithPath: "/tmp/Office.ovpn"),
                    text: "client\nremote vpn.example.com 1194\nauth-user-pass\n",
                    files: [:])
            }
        default:
            break
        }
    }
    #endif

    /// Profiles open one after the other; other files chosen with them are their
    /// certificates and keys, and those next to a profile are looked up too.
    private func load(_ urls: [URL]) {
        let isProfile = { (url: URL) in ["conf", "ovpn"].contains(url.pathExtension.lowercased()) }
        let chosen = urls.filter { !isProfile($0) }
        var profiles: [ImportedFile] = []
        for url in urls.filter(isProfile) {
            do {
                let text = try readText(url)
                profiles.append(ImportedFile(url: url, text: text, files: companions(of: url, text: text, chosen: chosen)))
            } catch {
                report(url, error.localizedDescription)
            }
        }
        if profiles.isEmpty, let first = urls.first {
            report(first, String(localized: "Choose a .conf or .ovpn file, with its certificates if it has any."))
        }
        pendingImports = profiles
        showNextImport()
    }

    private func showNextImport() {
        guard fileToImport == nil, !pendingImports.isEmpty else { return }
        fileToImport = pendingImports.removeFirst()
    }

    private func report(_ url: URL, _ details: String) {
        let file = url.lastPathComponent
        model.problem = ProblemReport(title: String(localized: "Cannot import “\(file)”"), details: details)
    }

    private var importTypes: [UTType] {
        ["conf", "ovpn", "crt", "pem", "key"].compactMap { UTType(filenameExtension: $0) }
    }
}

private func droppedURL(_ provider: NSItemProvider) async -> URL? {
    await withCheckedContinuation { continuation in
        _ = provider.loadObject(ofClass: URL.self) { url, _ in
            continuation.resume(returning: url)
        }
    }
}

private func readText(_ url: URL) throws -> String {
    let scoped = url.startAccessingSecurityScopedResource()
    defer { if scoped { url.stopAccessingSecurityScopedResource() } }
    return try String(contentsOf: url, encoding: .utf8)
}

/// The certificates and keys an OpenVPN profile names, by file name: among the
/// files chosen with it, then in its folder. The service checks that none lacks.
private func companions(of url: URL, text: String, chosen: [URL]) -> [String: String] {
    let directives: Set<String> = ["ca", "cert", "key", "tls-auth", "tls-crypt", "tls-crypt-v2"]
    var files: [String: String] = [:]
    for line in text.split(whereSeparator: \.isNewline) {
        let parts = line.split(whereSeparator: \.isWhitespace).map(String.init)
        guard parts.count >= 2, directives.contains(parts[0]) else { continue }
        let name = URL(fileURLWithPath: parts[1].trimmingCharacters(in: CharacterSet(charactersIn: "\"'"))).lastPathComponent
        let source = chosen.first { $0.lastPathComponent == name }
            ?? url.deletingLastPathComponent().appendingPathComponent(name)
        files[name] = try? readText(source)
    }
    return files
}

/// What the app has to say once, above everything else.
private struct Banners: View {
    @EnvironmentObject private var model: Model

    var body: some View {
        VStack(spacing: 0) {
            if model.serviceOutdated {
                banner("arrow.triangle.2.circlepath", tint: warningColor) {
                    Text("The tunnel service needs updating, as the app was.")
                } action: {
                    Button(model.installing ? "Installing…" : "Update the Service…") {
                        model.runServiceAction("--install-daemon")
                    }
                    .disabled(model.installing)
                }
            }
            if let version = model.updatedTo {
                banner("sparkles", tint: Color.accentColor) {
                    Text("Tunnelbird was updated to \(version)")
                } action: {
                    Button("What's New") { Links.open(Links.changelog) }
                    Button("Close") { model.updatedTo = nil }
                }
            }
        }
    }

    private func banner<Message: View, Actions: View>(
        _ icon: String, tint: Color,
        @ViewBuilder message: () -> Message,
        @ViewBuilder action: () -> Actions
    ) -> some View {
        VStack(spacing: 0) {
            HStack(spacing: 10) {
                Image(systemName: icon).foregroundStyle(tint)
                message().fixedSize(horizontal: false, vertical: true)
                Spacer()
                action()
            }
            .font(.callout)
            .padding(.horizontal, 16)
            .padding(.vertical, 9)
            Divider()
        }
        .background(tint.opacity(0.1))
    }
}

/// Without the service the app can do nothing. On a first use nothing is wrong
/// yet: the screen welcomes and offers to install, it does not raise an alarm.
private struct UnavailableView: View {
    @EnvironmentObject private var model: Model

    var body: some View {
        VStack(spacing: 12) {
            if (model.serviceInstalled && !model.installing) || model.serviceInApp == nil {
                Image(systemName: "exclamationmark.triangle")
                    .font(.system(size: 40))
                    .foregroundStyle(.secondary)
                Text("Tunnel Service Unavailable")
                    .font(.title2.weight(.semibold))
                Text("Tunnelbird needs its service to manage VPN tunnels. Make sure it is running.")
                    .multilineTextAlignment(.center)
                    .foregroundStyle(.secondary)
                if let failure = model.failure {
                    Text(failure)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .textSelection(.enabled)
                }
            } else {
                Image(nsImage: NSApp.applicationIconImage)
                    .resizable()
                    .frame(width: 96, height: 96)
                    .accessibilityHidden(true)
                Text("Welcome to Tunnelbird")
                    .font(.title2.weight(.semibold))
                Text("One last step: Tunnelbird installs a small service that creates your VPN tunnels.")
                    .multilineTextAlignment(.center)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            if model.serviceInApp != nil {
                Button {
                    model.runServiceAction("--install-daemon")
                } label: {
                    if model.installing {
                        HStack(spacing: 6) {
                            ProgressView().controlSize(.small)
                            Text("Installing…")
                        }
                    } else if model.serviceInstalled {
                        Text("Reinstall the Service…")
                    } else {
                        Text("Install the Service…")
                    }
                }
                .controlSize(.large)
                .keyboardShortcut(.defaultAction)
                .disabled(model.installing)
                .padding(.top, 4)
                Text("macOS asks for the administrator's password, once.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            } else {
                HStack(spacing: 6) {
                    ProgressView().controlSize(.small)
                    Text("Trying to connect…").foregroundStyle(.secondary)
                }
                .padding(.top, 4)
            }
        }
        .padding(32)
    }
}

private struct EmptyTunnelsView: View {
    let onImport: () -> Void

    var body: some View {
        VStack(spacing: 12) {
            Image(nsImage: NSApp.applicationIconImage)
                .resizable()
                .frame(width: 96, height: 96)
            Text("No Tunnels")
                .font(.title2.weight(.semibold))
            Text("Import the file provided by your VPN server, or drop it here.")
                .multilineTextAlignment(.center)
                .foregroundStyle(.secondary)
            Button("Import File…", action: onImport)
                .controlSize(.large)
                .keyboardShortcut(.defaultAction)
                .padding(.top, 4)
        }
        .padding(32)
    }
}

private struct TunnelList: View {
    @EnvironmentObject private var model: Model

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Tunnels").font(.headline)
            VStack(spacing: 0) {
                ForEach(Array(model.tunnels.enumerated()), id: \.element.id) { index, tunnel in
                    if index > 0 { Divider().padding(.leading, 44) }
                    TunnelRow(tunnel: tunnel)
                }
            }
            .background(CardBackground())
        }
    }
}

/// The rounded panel behind a group of rows.
struct CardBackground: View {
    var body: some View {
        // Not a window color: in dark mode those are too close to the window.
        RoundedRectangle(cornerRadius: 10)
            .fill(Color.primary.opacity(0.05))
            .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(Color.primary.opacity(0.08)))
    }
}

private struct TunnelRow: View {
    @EnvironmentObject private var model: Model
    @State private var confirmingDelete = false
    let tunnel: Tunnel

    private var inFlight: Bool {
        model.busy.contains(tunnel.id) || tunnel.state == .connecting || tunnel.state == .disconnecting
    }

    var body: some View {
        HStack(spacing: 12) {
            Image(systemName: tunnel.state == .connected ? "lock.shield.fill" : "lock.shield")
                .font(.title3)
                .foregroundStyle(tunnel.state == .connected ? connectedColor : Color.secondary)
                .frame(width: 24)
            VStack(alignment: .leading, spacing: 2) {
                Text(tunnel.name).lineLimit(1)
                Text(subtitle)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
            Spacer()
            if inFlight {
                ProgressView().controlSize(.small)
                Text(busyLabel).font(.caption).foregroundStyle(.secondary)
            } else {
                Toggle(isOn: Binding(
                    get: { tunnel.state == .connected },
                    set: { _ in model.toggle(tunnel) }
                )) {
                    Text(tunnel.state == .connected ? "Disconnect" : "Connect")
                }
                .toggleStyle(.switch)
                .labelsHidden()
            }
            Menu {
                Button("View Logs") { model.showLogs(tunnel) }
                Divider()
                Button("Edit…") { model.editedTunnel = tunnel }
                Button("Export as File…") { exportTunnel(tunnel, model: model) }
                if tunnel.kind != "openvpn" {
                    Button("Show QR Code") { model.qrTunnel = tunnel }
                }
                Divider()
                Button("Delete…", role: .destructive) { confirmingDelete = true }
                    .disabled(inFlight)
            } label: {
                Image(systemName: "ellipsis")
            }
            .menuStyle(.borderlessButton)
            .menuIndicator(.hidden)
            .fixedSize()
            .help("More")
            .accessibilityLabel(Text("More"))
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 10)
        .confirmationDialog(
            "Delete “\(tunnel.name)”?",
            isPresented: $confirmingDelete,
            titleVisibility: .visible
        ) {
            Button("Delete", role: .destructive) { model.delete(tunnel) }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("The tunnel will be removed from this computer. You can import it again from its file.")
        }
    }

    private var busyLabel: LocalizedStringKey {
        tunnel.state == .connected || tunnel.state == .disconnecting ? "Disconnecting…" : "Connecting…"
    }

    private var subtitle: String {
        let state: String
        switch tunnel.state {
        case .connected: state = String(localized: "Connected")
        case .connecting: state = String(localized: "Connecting…")
        case .disconnecting: state = String(localized: "Disconnecting…")
        case .disconnected: state = tunnel.endpoint ?? String(localized: "Disconnected")
        }
        return [tunnel.provider, state].compactMap { $0 }.joined(separator: " · ")
    }
}
