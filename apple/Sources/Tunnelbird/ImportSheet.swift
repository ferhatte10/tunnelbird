// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

import SwiftUI

struct ImportedFile: Identifiable {
    let id = UUID()
    let url: URL
    let text: String
    /// Certificates and keys found with the file, by name.
    let files: [String: String]
}

/// What the file says, for the preview. The service checks the file again with
/// the real parser; keys are never read here.
struct FilePreview {
    let kind: String
    var server: String?
    var addresses: [String] = []
    var asksLogin = false

    init(text: String) {
        let lines = text.split(whereSeparator: \.isNewline).map { $0.trimmingCharacters(in: .whitespaces) }
        if lines.contains(where: { $0.lowercased() == "[interface]" }) {
            kind = "WireGuard®"
            for line in lines {
                let parts = line.split(separator: "=", maxSplits: 1).map { $0.trimmingCharacters(in: .whitespaces) }
                guard parts.count == 2 else { continue }
                switch parts[0].lowercased() {
                case "endpoint": server = server ?? parts[1]
                case "address": addresses += parts[1].split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) }
                default: break
                }
            }
        } else {
            kind = "OpenVPN"
            for line in lines {
                let parts = line.split(whereSeparator: \.isWhitespace).map(String.init)
                if parts.first == "remote", parts.count >= 2 {
                    server = server ?? parts.dropFirst().prefix(2).joined(separator: ":")
                }
                if parts.first == "auth-user-pass" { asksLogin = true }
            }
        }
    }
}

struct ImportSheet: View {
    @EnvironmentObject private var model: Model
    @Environment(\.dismiss) private var dismiss
    let file: ImportedFile
    private let preview: FilePreview
    @State private var name: String
    @State private var username = ""
    @State private var password = ""
    @State private var importing = false
    @State private var failure: String?

    init(file: ImportedFile) {
        self.file = file
        preview = FilePreview(text: file.text)
        _name = State(initialValue: file.url.deletingPathExtension().lastPathComponent)
    }

    private var canImport: Bool {
        !name.trimmingCharacters(in: .whitespaces).isEmpty
            && !importing
            && (!preview.asksLogin || !username.isEmpty)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Import Tunnel").font(.title2.weight(.semibold))
            Form {
                TextField("Name", text: $name)
                LabeledContent("Type", value: preview.kind)
                if let server = preview.server {
                    LabeledContent("Server Address", value: server)
                }
                if !preview.addresses.isEmpty {
                    LabeledContent("Addresses", value: preview.addresses.joined(separator: ", "))
                }
                if preview.asksLogin {
                    TextField("User Name", text: $username)
                    SecureField("Password", text: $password)
                    Text("An empty password is asked at each connection.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
            if let failure {
                Label(failure, systemImage: "exclamationmark.triangle.fill")
                    .foregroundStyle(Color(nsColor: .systemRed))
                    .font(.callout)
                    .textSelection(.enabled)
                    .fixedSize(horizontal: false, vertical: true)
            }
            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button(action: run) {
                    if importing {
                        HStack(spacing: 6) {
                            ProgressView().controlSize(.small)
                            Text("Importing…")
                        }
                    } else {
                        Text("Import")
                    }
                }
                .keyboardShortcut(.defaultAction)
                .disabled(!canImport)
            }
        }
        .padding(20)
        .frame(width: 440)
    }

    private func run() {
        importing = true
        failure = nil
        Task {
            do {
                let login = preview.asksLogin ? (username, password) : nil
                try await model.importTunnel(
                    name: name.trimmingCharacters(in: .whitespaces), text: file.text, login: login, files: file.files)
                dismiss()
            } catch let error as ServiceError {
                failure = error.details
            } catch {
                failure = error.localizedDescription
            }
            importing = false
        }
    }
}

struct PasswordSheet: View {
    @EnvironmentObject private var model: Model
    @Environment(\.dismiss) private var dismiss
    let request: PasswordRequest
    @State private var username = ""
    @State private var password = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Connect “\(request.name)”").font(.title2.weight(.semibold))
            Form {
                TextField("User Name", text: $username)
                SecureField("Password", text: $password)
            }
            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button("Connect") {
                    model.answerPassword(request, username: username, password: password)
                    dismiss()
                }
                .keyboardShortcut(.defaultAction)
                .disabled(username.isEmpty || password.isEmpty)
            }
        }
        .padding(20)
        .frame(width: 400)
    }
}
