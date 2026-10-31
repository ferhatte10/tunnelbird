// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

import AppKit
import CoreImage
import CoreImage.CIFilterBuiltins
import SwiftUI

/// Edits a tunnel's file as text: the service checks it again with the real
/// parser, and says what is wrong.
struct EditSheet: View {
    @EnvironmentObject private var model: Model
    @Environment(\.dismiss) private var dismiss
    let tunnel: Tunnel
    @State private var name: String
    @State private var text = ""
    @State private var username = ""
    @State private var password = ""
    @State private var loading = true
    @State private var saving = false
    @State private var failure: String?

    init(tunnel: Tunnel) {
        self.tunnel = tunnel
        _name = State(initialValue: tunnel.name)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Edit Tunnel").font(.title2.weight(.semibold))
            TextField("Name", text: $name)
            if loading {
                ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                TextEditor(text: $text)
                    .font(.system(.callout, design: .monospaced))
                    .autocorrectionDisabled()
                    .frame(minHeight: 260)
                    .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(Color.primary.opacity(0.15)))
                if tunnel.kind == "openvpn" {
                    HStack {
                        TextField("User Name", text: $username)
                        SecureField("Password", text: $password)
                    }
                    Text("Leave these empty to keep the current login.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
            if let failure {
                Label(failure, systemImage: "exclamationmark.triangle.fill")
                    .foregroundStyle(errorColor)
                    .font(.callout)
                    .textSelection(.enabled)
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
        .frame(width: 560, height: 520)
        .task {
            do {
                text = try await model.exported(tunnel)
            } catch {
                failure = (error as? ServiceError)?.details ?? error.localizedDescription
            }
            loading = false
        }
    }

    private func save() {
        saving = true
        failure = nil
        Task {
            do {
                try await model.update(
                    tunnel,
                    name: name.trimmingCharacters(in: .whitespaces),
                    text: text,
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
