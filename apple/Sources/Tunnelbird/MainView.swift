// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

import AppKit
import SwiftUI
import UniformTypeIdentifiers

struct MainView: View {
    @EnvironmentObject private var model: Model
    @State private var importing = false
    @State private var fileToImport: ImportedFile?
    @State private var dropTargeted = false
    #if DEBUG
    @State private var snapshotPreferences = false
    @State private var snapshotAbout = false
    #endif

    var body: some View {
        Group {
            if !model.ready {
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
        .fileImporter(isPresented: $importing, allowedContentTypes: importTypes) { result in
            if case .success(let url) = result { load(url) }
        }
        .onDrop(of: [.fileURL], isTargeted: $dropTargeted) { providers in
            guard model.ready, let provider = providers.first else { return false }
            _ = provider.loadObject(ofClass: URL.self) { url, _ in
                if let url { Task { @MainActor in load(url) } }
            }
            return true
        }
        .sheet(item: $fileToImport) { file in
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
                    text: "client\nremote vpn.example.com 1194\nauth-user-pass\n")
            }
        default:
            break
        }
    }
    #endif

    private func load(_ url: URL) {
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        do {
            let text = try String(contentsOf: url, encoding: .utf8)
            fileToImport = ImportedFile(url: url, text: text)
        } catch {
            let file = url.lastPathComponent
            model.problem = ProblemReport(
                title: String(localized: "Cannot import “\(file)”"),
                details: error.localizedDescription)
        }
    }

    private var importTypes: [UTType] {
        [UTType(filenameExtension: "conf"), UTType(filenameExtension: "ovpn")].compactMap { $0 }
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
