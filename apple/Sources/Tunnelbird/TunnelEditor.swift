// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

import AppKit
import CoreImage
import CoreImage.CIFilterBuiltins
import SwiftUI

/// A tunnel's file, edited setting by setting. The text stays the reference:
/// each field reads and writes its line, so that settings without a field, and
/// the other servers of a file, are kept as they are. The service checks the
/// result with the real parser, and says what is wrong.
struct ProfileText {
    var text: String

    private var lines: [Substring] { text.split(separator: "\n", omittingEmptySubsequences: false) }

    private func isSection(_ line: Substring) -> Bool {
        line.trimmingCharacters(in: .whitespaces).hasPrefix("[")
    }

    private func range(of section: String, in lines: [Substring]) -> Range<Int>? {
        let header = "[\(section.lowercased())]"
        guard let start = lines.firstIndex(where: { $0.trimmingCharacters(in: .whitespaces).lowercased() == header }) else {
            return nil
        }
        let end = lines[(start + 1)...].firstIndex(where: isSection) ?? lines.count
        return (start + 1)..<end
    }

    private func keyOf(_ line: Substring) -> String? {
        let parts = line.split(separator: "=", maxSplits: 1)
        return parts.count == 2 ? parts[0].trimmingCharacters(in: .whitespaces).lowercased() : nil
    }

    /// In `[section]`, the first occurrence: the first server's, as on Linux.
    func value(_ section: String, _ key: String) -> String {
        let all = lines
        guard let range = range(of: section, in: all) else { return "" }
        for line in all[range] where keyOf(line) == key.lowercased() {
            let parts = line.split(separator: "=", maxSplits: 1)
            return parts[1].trimmingCharacters(in: .whitespaces)
        }
        return ""
    }

    /// Empty removes the line; a new one goes right after the section's header.
    mutating func set(_ section: String, _ key: String, _ value: String) {
        var all = lines
        guard let range = range(of: section, in: all) else { return }
        let index = all[range].firstIndex { keyOf($0) == key.lowercased() }
        let line = Substring("\(key) = \(value)")
        switch (index, value.isEmpty) {
        case (let index?, true): all.remove(at: index)
        case (let index?, false): all[index] = line
        case (nil, false): all.insert(line, at: range.lowerBound)
        case (nil, true): break
        }
        text = all.joined(separator: "\n")
    }

    var serverCount: Int {
        lines.filter { $0.trimmingCharacters(in: .whitespaces).lowercased() == "[peer]" }.count
    }

    /// An OpenVPN directive's arguments, on its first line.
    func directive(_ name: String) -> [String] {
        for line in lines {
            let parts = line.split(whereSeparator: \.isWhitespace).map(String.init)
            if parts.first == name { return Array(parts.dropFirst()) }
        }
        return []
    }

    mutating func setDirective(_ name: String, _ arguments: [String]) {
        var all = lines
        let line = Substring(([name] + arguments).joined(separator: " "))
        if let index = all.firstIndex(where: { $0.split(whereSeparator: \.isWhitespace).first == Substring(name) }) {
            all[index] = line
        } else {
            all.insert(line, at: min(1, all.count))
        }
        text = all.joined(separator: "\n")
    }
}

struct EditSheet: View {
    @EnvironmentObject private var model: Model
    @Environment(\.dismiss) private var dismiss
    let tunnel: Tunnel
    @State private var name: String
    @State private var profile = ProfileText(text: "")
    @State private var showingFile = false
    @State private var username = ""
    @State private var password = ""
    @State private var loading = true
    @State private var saving = false
    @State private var failure: String?

    init(tunnel: Tunnel) {
        self.tunnel = tunnel
        _name = State(initialValue: tunnel.name)
    }

    private func field(_ section: String, _ key: String) -> Binding<String> {
        Binding(
            get: { profile.value(section, key) },
            set: { profile.set(section, key, $0.trimmingCharacters(in: .whitespacesAndNewlines)) })
    }

    /// `host:port`, which the file writes as `remote host port`.
    private var remote: Binding<String> {
        Binding(
            get: { profile.directive("remote").prefix(2).joined(separator: ":") },
            set: { typed in
                let parts = typed.split(separator: ":", maxSplits: 1).map(String.init)
                let rest = profile.directive("remote").dropFirst(2)
                profile.setDirective("remote", parts + rest)
            })
    }

    private var transport: Binding<String> {
        Binding(
            get: { profile.directive("proto").first?.hasPrefix("tcp") == true ? "tcp" : "udp" },
            set: { profile.setDirective("proto", [$0]) })
    }

    var body: some View {
        VStack(spacing: 12) {
            Text("Edit Tunnel")
                .font(.title2.weight(.semibold))
                .frame(maxWidth: .infinity, alignment: .leading)
            Picker("Edit Tunnel", selection: $showingFile) {
                Text("Settings").tag(false)
                Text("File").tag(true)
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            if loading {
                ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if showingFile {
                TextEditor(text: $profile.text)
                    .font(.system(.callout, design: .monospaced))
                    .autocorrectionDisabled()
                    .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(Color.primary.opacity(0.15)))
            } else if tunnel.kind == "openvpn" {
                openVPNForm
            } else {
                wireGuardForm
            }
            if let failure {
                Label(failure, systemImage: "exclamationmark.triangle.fill")
                    .foregroundStyle(errorColor)
                    .font(.callout)
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .fixedSize(horizontal: false, vertical: true)
            }
            HStack {
                if tunnel.state == .connected {
                    Text("Reconnect to apply the changes.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button(action: save) {
                    if saving {
                        HStack(spacing: 6) {
                            ProgressView().controlSize(.small)
                            Text("Saving…")
                        }
                    } else {
                        Text("Save")
                    }
                }
                .keyboardShortcut(.defaultAction)
                .disabled(saving || loading || name.trimmingCharacters(in: .whitespaces).isEmpty)
            }
        }
        .padding(20)
        .frame(width: 560, height: 620)
        .task {
            do {
                profile.text = try await model.exported(tunnel)
            } catch {
                failure = (error as? ServiceError)?.details ?? error.localizedDescription
            }
            loading = false
        }
    }

    private var wireGuardForm: some View {
        Form {
            Section {
                TextField("Name", text: $name)
            }
            Section("Server") {
                TextField("Server Address", text: field("Peer", "Endpoint"))
                TextField("Server Public Key", text: field("Peer", "PublicKey"))
                TextField("Preshared Key (optional)", text: field("Peer", "PresharedKey"))
                TextField("Routed Networks", text: field("Peer", "AllowedIPs"))
                TextField("Keepalive in Seconds (optional)", text: field("Peer", "PersistentKeepalive"))
                if profile.serverCount > 1 {
                    Text("This tunnel has \(profile.serverCount) servers. Only the first one is shown; the others are kept.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
            Section {
                TextField("Addresses", text: field("Interface", "Address"))
                TextField("DNS Servers", text: field("Interface", "DNS"))
                TextField("Private Key", text: field("Interface", "PrivateKey"))
                TextField("MTU (optional)", text: field("Interface", "MTU"))
            } header: {
                Text("This Computer")
            } footer: {
                Text("Addresses and keys of this computer inside the VPN")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
    }

    private var openVPNForm: some View {
        Form {
            Section {
                TextField("Name", text: $name)
            }
            Section("Server") {
                TextField("Server Address", text: remote)
                Picker("Protocol", selection: transport) {
                    Text(verbatim: "UDP").tag("udp")
                    Text(verbatim: "TCP").tag("tcp")
                }
            }
            Section {
                TextField("User Name", text: $username)
                SecureField("Password", text: $password)
            } header: {
                Text("Login")
            } footer: {
                Text("Leave these empty to keep the current login.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
    }

    private func save() {
        saving = true
        failure = nil
        Task {
            do {
                try await model.update(
                    tunnel,
                    name: name.trimmingCharacters(in: .whitespaces),
                    text: profile.text,
                    login: username.isEmpty ? nil : (username, password))
                dismiss()
            } catch {
                failure = (error as? ServiceError)?.details ?? error.localizedDescription
            }
            saving = false
        }
    }
}

/// The QR code the mobile VPN apps scan. It holds the private key: it is only
/// ever on screen, never written anywhere.
struct QRSheet: View {
    @EnvironmentObject private var model: Model
    @Environment(\.dismiss) private var dismiss
    let tunnel: Tunnel
    @State private var image: NSImage?
    @State private var failure: String?

    var body: some View {
        VStack(spacing: 14) {
            Text("Scan with Your Phone").font(.title2.weight(.semibold))
            Group {
                if let image {
                    Image(nsImage: image)
                        .interpolation(.none)
                        .resizable()
                        .padding(12)
                        .background(Color.white)
                        .clipShape(RoundedRectangle(cornerRadius: 10))
                        .accessibilityLabel(Text("QR code of “\(tunnel.name)”"))
                } else if let failure {
                    Text(failure).multilineTextAlignment(.center)
                } else {
                    ProgressView()
                }
            }
            .frame(width: 300, height: 300)
            Text("On your phone, open your VPN app, add a tunnel and choose to scan a QR code.")
                .multilineTextAlignment(.center)
                .fixedSize(horizontal: false, vertical: true)
            Label("This code contains your private key: anyone who scans it can use this tunnel. Show it only to your own devices.",
                  systemImage: "exclamationmark.triangle.fill")
                .font(.callout)
                .foregroundStyle(warningColor)
                .fixedSize(horizontal: false, vertical: true)
            Button("Close") { dismiss() }
                .keyboardShortcut(.defaultAction)
        }
        .padding(20)
        .frame(width: 400)
        .task {
            do {
                let text = try await model.exported(tunnel)
                image = qrCode(text)
                if image == nil {
                    failure = String(localized: "This tunnel is too large for a QR code. Export it as a file instead.")
                }
            } catch {
                failure = (error as? ServiceError)?.details ?? error.localizedDescription
            }
        }
    }
}

private func qrCode(_ text: String) -> NSImage? {
    let filter = CIFilter.qrCodeGenerator()
    filter.message = Data(text.utf8)
    filter.correctionLevel = "M"
    guard let output = filter.outputImage,
          let cgImage = CIContext().createCGImage(output, from: output.extent) else { return nil }
    return NSImage(cgImage: cgImage, size: NSSize(width: output.extent.width, height: output.extent.height))
}

/// Writes the tunnel to a file only its owner can read: it holds the keys.
@MainActor
func exportTunnel(_ tunnel: Tunnel, model: Model) {
    Task {
        do {
            let text = try await model.exported(tunnel)
            let panel = NSSavePanel()
            panel.title = String(localized: "Export Tunnel")
            panel.message = String(localized: "Shared tunnels include the private key: keep them to yourself.")
            panel.nameFieldStringValue = "\(tunnel.name).\(tunnel.kind == "openvpn" ? "ovpn" : "conf")"
            guard panel.runModal() == .OK, let url = panel.url else { return }
            try writePrivately(text, to: url)
        } catch {
            model.problem = ProblemReport(
                title: String(localized: "Export Tunnel"),
                details: (error as? ServiceError)?.details ?? error.localizedDescription)
        }
    }
}
