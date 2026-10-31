// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

// A menu bar app with a main window. It only shows things and sends clicks:
// tunnels are made by the root service (crates/service, docs/MACOS.md).

import AppKit
import SwiftUI

@main
struct TunnelbirdApp: App {
    @NSApplicationDelegateAdaptor private var delegate: AppDelegate
    @StateObject private var model = Model()

    init() {
        #if DEBUG
        takeSnapshotIfAsked()
        #endif
    }

    var body: some Scene {
        Window("Tunnelbird", id: mainWindowID) {
            MainView()
                .environmentObject(model)
                .frame(minWidth: 460, idealWidth: 520, minHeight: 480, idealHeight: 640)
        }
        .defaultSize(width: 520, height: 680)
        .windowResizability(.contentMinSize)
        .commands { AppCommands(model: model) }

        Window("About Tunnelbird", id: aboutWindowID) {
            AboutView()
        }
        .windowResizability(.contentSize)
        .defaultPosition(.center)

        Settings {
            PreferencesView().environmentObject(model)
        }

        MenuBarExtra {
            MenuBarContent()
                .environmentObject(model)
        } label: {
            Image(nsImage: trayIcon(connected: model.connected != nil))
        }
        .menuBarExtraStyle(.window)
    }
}

let mainWindowID = "main"

final class AppDelegate: NSObject, NSApplicationDelegate {
    func applicationDidFinishLaunching(_ notification: Notification) {
        #if DEBUG
        if ProcessInfo.processInfo.environment["TUNNELBIRD_APPEARANCE"] != nil { return }
        #endif
        MainActor.assumeIsolated { Preferences.shared.applyTheme() }
    }

    /// The tunnel belongs to the service and would outlive the app, with no
    /// icon left to control it. A stuck service must not block quitting.
    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        MainActor.assumeIsolated {
            guard Preferences.shared.disconnectOnQuit, let model = Model.current,
                  model.tunnels.contains(where: { $0.state != .disconnected }) else {
                return .terminateNow
            }
            Task {
                await withTaskGroup(of: Void.self) { group in
                    group.addTask { await model.disconnectAll() }
                    group.addTask { try? await Task.sleep(for: .seconds(8)) }
                    await group.next()
                    group.cancelAll()
                }
                sender.reply(toApplicationShouldTerminate: true)
            }
            return .terminateLater
        }
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        MainActor.assumeIsolated { !Preferences.shared.runInBackground }
    }
}

private func trayIcon(connected: Bool) -> NSImage {
    let name = connected ? "tray-connected" : "tray-disconnected"
    guard let url = Bundle.main.url(forResource: name, withExtension: "png"),
          let image = NSImage(contentsOf: url) else {
        return NSImage(systemSymbolName: "network", accessibilityDescription: "Tunnelbird") ?? NSImage()
    }
    // The file is drawn at twice the menu bar's height.
    image.size = NSSize(width: image.size.width / 2, height: image.size.height / 2)
    return image
}

/// One preferences window, whichever way it is opened: since macOS 14 the
/// action behind it only works from the app's own menus, or through this.
@available(macOS 14, *)
private struct PreferencesButton: View {
    @Environment(\.openSettings) private var openSettings

    var body: some View {
        PanelButton("Preferences…", systemImage: "gearshape") {
            NSApp.activate(ignoringOtherApps: true)
            openSettings()
        }
    }
}

/// A full-width button of the menu bar panel, lit up under the pointer.
private struct PanelButton: View {
    let title: LocalizedStringKey
    let systemImage: String
    let action: () -> Void
    @State private var hovering = false

    init(_ title: LocalizedStringKey, systemImage: String, action: @escaping () -> Void) {
        self.title = title
        self.systemImage = systemImage
        self.action = action
    }

    var body: some View {
        Button(action: action) {
            Label(title, systemImage: systemImage)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, 8)
                .padding(.vertical, 5)
                .contentShape(Rectangle())
                .background(hovering ? Color.primary.opacity(0.1) : .clear, in: RoundedRectangle(cornerRadius: 6))
        }
        .buttonStyle(.plain)
        .onHover { hovering = $0 }
    }
}

/// A panel and not a menu: a menu is rebuilt with each second's traffic, and
/// macOS then loses the line under the pointer.
struct MenuBarContent: View {
    @EnvironmentObject private var model: Model
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            status
            if let connected = model.connected {
                activity(connected)
            }
            if !model.tunnels.isEmpty {
                Divider()
                ForEach(model.tunnels) { tunnel in
                    tunnelRow(tunnel)
                }
            }
            Divider()
            VStack(spacing: 0) {
                if model.updateAvailable != nil {
                    PanelButton("Update Available…", systemImage: "arrow.down.circle") {
                        model.openDownloadPage()
                    }
                }
                PanelButton("Open Tunnelbird", systemImage: "macwindow") {
                    openWindow(id: mainWindowID)
                    NSApp.activate(ignoringOtherApps: true)
                }
                if #available(macOS 14, *) {
                    PreferencesButton()
                } else {
                    PanelButton("Preferences…", systemImage: "gearshape") {
                        NSApp.activate(ignoringOtherApps: true)
                        NSApp.sendAction(Selector(("showPreferencesWindow:")), to: nil, from: nil)
                    }
                }
                PanelButton("Quit", systemImage: "power") { NSApp.terminate(nil) }
            }
        }
        .padding(12)
        .frame(width: 300)
    }

    @ViewBuilder private var status: some View {
        if let connected = model.connected {
            if model.serverResponds {
                Text("Connected to “\(connected.name)”")
                    .font(.headline)
                    .foregroundStyle(connectedColor)
            } else {
                Text("“\(connected.name)”: server not responding")
                    .font(.headline)
                    .foregroundStyle(warningColor)
            }
            if connected.fullTunnel, case .ready(let found) = model.location {
                Text([flagEmoji(found.countryCode ?? ""), found.place, found.ip]
                    .compactMap { $0 }.joined(separator: " "))
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
            }
        } else {
            Text("Not connected").font(.headline)
        }
    }

    private func activity(_ connected: Tunnel) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            TrafficGraph(samples: model.samples)
                .frame(height: 48)
                .accessibilityLabel(Text("Traffic over the last minute"))
            HStack {
                Text("↓ \(formatRate(model.samples.last?.rx ?? 0))")
                    .foregroundStyle(TrafficGraph.rxColor)
                Spacer()
                Text("↑ \(formatRate(model.samples.last?.tx ?? 0))")
                    .foregroundStyle(TrafficGraph.txColor)
            }
            .font(.callout.weight(.semibold))
            .monospacedDigit()
        }
    }

    private func tunnelRow(_ tunnel: Tunnel) -> some View {
        HStack {
            Text(tunnel.name).lineLimit(1)
            Spacer()
            if model.busy.contains(tunnel.id) || tunnel.state == .connecting || tunnel.state == .disconnecting {
                ProgressView().controlSize(.small)
                Text(tunnel.state == .disconnecting ? "Disconnecting…" : "Connecting…")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            } else {
                Toggle(isOn: Binding(
                    get: { tunnel.state == .connected },
                    set: { _ in model.toggle(tunnel) }
                )) {
                    Text(tunnel.state == .connected ? "Disconnect" : "Connect")
                }
                .toggleStyle(.switch)
                .controlSize(.small)
                .labelsHidden()
            }
        }
    }
}

#if DEBUG
/// For checking the interface with the fake service, without any screen
/// permission: `TUNNELBIRD_SNAPSHOT=out.png` saves the window, then quits.
private func takeSnapshotIfAsked() {
    let environment = ProcessInfo.processInfo.environment
    guard let path = environment["TUNNELBIRD_SNAPSHOT"] else { return }
    if let appearance = environment["TUNNELBIRD_APPEARANCE"] {
        NSApplication.shared.appearance = NSAppearance(named: appearance == "dark" ? .darkAqua : .aqua)
    }
    let delay = Double(environment["TUNNELBIRD_SNAPSHOT_DELAY"] ?? "") ?? 3
    DispatchQueue.main.asyncAfter(deadline: .now() + delay) {
        if let main = NSApp.windows.first(where: { $0.contentView != nil && $0.isVisible && $0.level == .normal }),
           let view = (main.attachedSheet ?? main).contentView,
           let bitmap = view.bitmapImageRepForCachingDisplay(in: view.bounds) {
            view.cacheDisplay(in: view.bounds, to: bitmap)
            try? bitmap.representation(using: .png, properties: [:])?.write(to: URL(fileURLWithPath: path))
        }
        // Not `terminate`: an open dialog would hold it back.
        exit(0)
    }
}
#endif
