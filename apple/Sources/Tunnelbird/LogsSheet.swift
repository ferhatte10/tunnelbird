// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

import AppKit
import SwiftUI

/// The logs of a tunnel, live: what the service saw and what the VPN program
/// wrote, kept by the service for its session.
struct LogsSheet: View {
    @EnvironmentObject private var model: Model
    @Environment(\.dismiss) private var dismiss
    let tunnel: Tunnel
    @State private var problemsOnly = false
    @State private var notice: String?

    private var shown: [LogLine] {
        problemsOnly ? model.logLines.filter { $0.severity != "info" } : model.logLines
    }

    var body: some View {
        VStack(spacing: 0) {
            header
            Divider()
            if shown.isEmpty {
                empty
            } else {
                list
            }
            if let notice {
                Divider()
                Text(notice)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal, 16)
                    .padding(.vertical, 8)
            }
        }
        .frame(width: 620, height: 520)
    }

    private var header: some View {
        HStack(spacing: 8) {
            Toggle(isOn: $problemsOnly) {
                Label("Problems Only", systemImage: "exclamationmark.triangle")
            }
            .toggleStyle(.button)
            VStack(spacing: 0) {
                Text("Logs").font(.headline)
                Text(tunnel.name).font(.caption).foregroundStyle(.secondary).lineLimit(1)
            }
            .frame(maxWidth: .infinity)
            Button(action: copy) {
                Image(systemName: "doc.on.doc")
            }
            .help("Copy Logs")
            .accessibilityLabel(Text("Copy Logs"))
            .disabled(shown.isEmpty)
            Button(action: save) {
                Image(systemName: "square.and.arrow.down")
            }
            .help("Save Logs")
            .accessibilityLabel(Text("Save Logs"))
            .disabled(shown.isEmpty)
            Button("Close") { dismiss() }
                .keyboardShortcut(.cancelAction)
        }
        .padding(12)
    }

    private var empty: some View {
        VStack(spacing: 6) {
            Text(problemsOnly ? "No Problems" : "No Events Yet")
                .font(.title3.weight(.semibold))
            if problemsOnly {
                Text("Nothing went wrong on this tunnel.").foregroundStyle(.secondary)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private var list: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(spacing: 0) {
                    ForEach(shown) { line in
                        LogRow(line: line).id(line.id)
                    }
                }
                .textSelection(.enabled)
            }
            // New lines stay in view.
            .onChange(of: shown.count) { _ in
                if let last = shown.last { proxy.scrollTo(last.id, anchor: .bottom) }
            }
            .onAppear {
                if let last = shown.last { proxy.scrollTo(last.id, anchor: .bottom) }
            }
        }
    }

    private var text: String {
        shown.map(plainLine).joined(separator: "\n") + "\n"
    }

    private func copy() {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
    }

    private func save() {
        let panel = NSSavePanel()
        panel.title = String(localized: "Save Logs")
        panel.nameFieldStringValue = "\(tunnel.name).log"
        guard panel.runModal() == .OK, let url = panel.url else { return }
        do {
            try writePrivately(text, to: url)
            notice = String(localized: "Saved to \(url.path)")
        } catch {
            notice = String(localized: "This location cannot be written to")
        }
    }
}

/// Logs name servers and addresses: only the owner may read the file (0600),
/// from before anything is written in it.
func writePrivately(_ text: String, to url: URL) throws {
    let fileManager = FileManager.default
    if !fileManager.fileExists(atPath: url.path) {
        guard fileManager.createFile(atPath: url.path, contents: nil, attributes: [.posixPermissions: 0o600]) else {
            throw CocoaError(.fileWriteNoPermission)
        }
    }
    try fileManager.setAttributes([.posixPermissions: 0o600], ofItemAtPath: url.path)
    let handle = try FileHandle(forWritingTo: url)
    defer { try? handle.close() }
    try handle.truncate(atOffset: 0)
    try handle.write(contentsOf: Data(text.utf8))
}

private let logDate: DateFormatter = {
    let formatter = DateFormatter()
    formatter.dateStyle = .short
    formatter.timeStyle = .medium
    return formatter
}()

private func sourceLabel(_ line: LogLine) -> String {
    switch line.source {
    case "openvpn": return "OpenVPN"
    case "wireguard": return "WireGuard®"
    default: return "Tunnelbird"
    }
}

private func severityLabel(_ line: LogLine) -> String? {
    switch line.severity {
    case "warning": return String(localized: "Warning")
    case "error": return String(localized: "Error")
    default: return nil
    }
}

/// The service writes English; its own fixed sentences have a translation.
private func messageText(_ line: LogLine) -> String {
    guard line.source == "service" else { return line.message }
    for prefix in ["Cannot connect", "Stopped on its own"] where line.message.hasPrefix(prefix + ": ") {
        let rest = line.message.dropFirst(prefix.count + 2)
        return "\(Bundle.main.localizedString(forKey: prefix, value: prefix, table: nil)) : \(rest)"
    }
    return Bundle.main.localizedString(forKey: line.message, value: line.message, table: nil)
}

private func plainLine(_ line: LogLine) -> String {
    let date = logDate.string(from: Date(timeIntervalSince1970: line.time))
    let severity = severityLabel(line).map { " [\($0)]" } ?? ""
    return "\(date) \(sourceLabel(line))\(severity) \(messageText(line))"
}

private struct LogRow: View {
    @Environment(\.colorScheme) private var colorScheme
    let line: LogLine

    private var tint: Color? {
        switch line.severity {
        case "warning": return Color.orange.opacity(colorScheme == .dark ? 0.2 : 0.14)
        case "error": return Color.red.opacity(colorScheme == .dark ? 0.24 : 0.12)
        default: return nil
        }
    }

    private var severityColor: Color {
        line.severity == "error" ? errorColor : warningColor
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            HStack(spacing: 10) {
                Text(logDate.string(from: Date(timeIntervalSince1970: line.time)))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .monospacedDigit()
                Text(sourceLabel(line))
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(Color.accentColor)
                Spacer()
                if let label = severityLabel(line) {
                    Text(label)
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(severityColor)
                }
            }
            Text(messageText(line))
                .font(.system(.callout, design: .monospaced))
                .frame(maxWidth: .infinity, alignment: .leading)
                .fixedSize(horizontal: false, vertical: true)
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 8)
        .background(tint)
    }
}
