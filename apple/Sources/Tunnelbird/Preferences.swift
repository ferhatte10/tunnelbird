// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

import AppKit
import CoreLocation
import CoreWLAN
import ServiceManagement
import SwiftUI

struct AccessPoint: Codable, Equatable {
    let bssid: String
    let ssid: String
}

/// The user's choices, kept in the app's defaults.
@MainActor
final class Preferences: ObservableObject {
    static let shared = Preferences()
    private let defaults = UserDefaults.standard

    @Published var theme: String { didSet { defaults.set(theme, forKey: "theme"); applyTheme() } }
    @Published var runInBackground: Bool { didSet { defaults.set(runInBackground, forKey: "run-in-background") } }
    @Published var disconnectOnQuit: Bool { didSet { defaults.set(disconnectOnQuit, forKey: "disconnect-on-quit") } }
    @Published var checkUpdates: Bool { didSet { defaults.set(checkUpdates, forKey: "check-for-updates") } }
    @Published var autoConnect: Bool { didSet { defaults.set(autoConnect, forKey: "auto-connect") } }
    @Published var autoTunnel: String { didSet { defaults.set(autoTunnel, forKey: "auto-connect-tunnel") } }
    @Published var trustWired: Bool { didSet { defaults.set(trustWired, forKey: "trust-wired") } }
    @Published var strict: Bool { didSet { defaults.set(strict, forKey: "strict-wifi-check") } }
    @Published var trusted: [String] { didSet { defaults.set(trusted, forKey: "trusted-wifi") } }
    @Published var pins: [AccessPoint] {
        didSet { defaults.set(try? JSONEncoder().encode(pins), forKey: "trusted-access-points") }
    }

    private init() {
        theme = defaults.string(forKey: "theme") ?? "system"
        runInBackground = defaults.object(forKey: "run-in-background") as? Bool ?? true
        disconnectOnQuit = defaults.object(forKey: "disconnect-on-quit") as? Bool ?? true
        checkUpdates = defaults.object(forKey: "check-for-updates") as? Bool ?? true
        autoConnect = defaults.bool(forKey: "auto-connect")
        autoTunnel = defaults.string(forKey: "auto-connect-tunnel") ?? ""
        trustWired = defaults.object(forKey: "trust-wired") as? Bool ?? true
        strict = defaults.bool(forKey: "strict-wifi-check")
        trusted = defaults.stringArray(forKey: "trusted-wifi") ?? []
        pins = (defaults.data(forKey: "trusted-access-points"))
            .flatMap { try? JSONDecoder().decode([AccessPoint].self, from: $0) } ?? []
    }

    func applyTheme() {
        switch theme {
        case "light": NSApp.appearance = NSAppearance(named: .aqua)
        case "dark": NSApp.appearance = NSAppearance(named: .darkAqua)
        default: NSApp.appearance = nil
        }
    }

    /// The rule's input, as `tunnelbird_core::trust::Policy` reads it.
    var policy: [String: Any] {
        [
            "enabled": autoConnect,
            "tunnel": autoTunnel,
            "trusted_ssids": trusted,
            "trust_wired": trustWired,
            "strict": strict,
            "pins": pins.map { ["bssid": $0.bssid, "ssid": $0.ssid] },
        ]
    }
}

/// The Wi-Fi network this computer is on. macOS hides its name from apps
/// without the location permission.
@MainActor
final class WiFiAccess: NSObject, ObservableObject, CLLocationManagerDelegate {
    static let shared = WiFiAccess()
    private let manager = CLLocationManager()
    @Published var allowed = false

    override init() {
        super.init()
        manager.delegate = self
        update()
    }

    func ask() {
        manager.requestAlwaysAuthorization()
    }

    private func update() {
        allowed = manager.authorizationStatus == .authorizedAlways
    }

    nonisolated func locationManagerDidChangeAuthorization(_ manager: CLLocationManager) {
        Task { @MainActor in self.update() }
    }

    /// Snapshots must never show the real Wi-Fi names of this computer.
    private var hidden: Bool { ProcessInfo.processInfo.environment["TUNNELBIRD_SNAPSHOT"] != nil }

    var ssid: String? { hidden ? nil : CWWiFiClient.shared().interface()?.ssid() }
    var bssid: String? { hidden ? nil : CWWiFiClient.shared().interface()?.bssid()?.uppercased() }

    /// What the computer reaches the internet through: `wifi`, `wired`, `other`,
    /// or empty. From the system's own ranking (`scutil --nwi`), not from the
    /// network path: with the Wi-Fi down a path remains, on Ethernet-like
    /// interfaces (Thunderbolt) that lead nowhere. A tunnel is not listed.
    func currentNetworkKind() -> String {
        guard let name = primaryInterface() else { return "" }
        if name == CWWiFiClient.shared().interface()?.interfaceName { return "wifi" }
        return name.hasPrefix("en") ? "wired" : "other"
    }

    private func primaryInterface() -> String? {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/sbin/scutil")
        process.arguments = ["--nwi"]
        let pipe = Pipe()
        process.standardOutput = pipe
        process.standardError = Pipe()
        guard (try? process.run()) != nil else { return nil }
        let data = pipe.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        return String(decoding: data, as: UTF8.self)
            .split(separator: "\n")
            .compactMap { line -> String? in
                let parts = line.split(separator: " ", omittingEmptySubsequences: true)
                return parts.count > 2 && parts[1] == ":" && parts[2] == "flags" && parts[0] != "REACH"
                    ? String(parts[0]) : nil
            }
            .first
    }

    /// The networks this computer has already joined.
    func savedNetworks() -> [String] {
        if hidden { return ["Home", "Office"] }
        let name = CWWiFiClient.shared().interface()?.interfaceName ?? "en0"
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/sbin/networksetup")
        process.arguments = ["-listpreferredwirelessnetworks", name]
        let pipe = Pipe()
        process.standardOutput = pipe
        process.standardError = Pipe()
        guard (try? process.run()) != nil else { return [] }
        let data = pipe.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        return String(decoding: data, as: UTF8.self)
            .split(separator: "\n")
            .dropFirst()
            .map { $0.trimmingCharacters(in: .whitespaces) }
            .sorted { $0.localizedCaseInsensitiveCompare($1) == .orderedAscending }
    }
}

struct PreferencesView: View {
    @State private var tab = ProcessInfo.processInfo.environment["TUNNELBIRD_SNAPSHOT_TAB"] ?? "general"

    var body: some View {
        TabView(selection: $tab) {
            GeneralTab().tabItem { Label("General", systemImage: "gearshape") }.tag("general")
            AutoConnectTab().tabItem { Label("Automatic Connection", systemImage: "wifi") }.tag("auto")
        }
        .frame(width: 520, height: 560)
    }
}

private struct GeneralTab: View {
    @ObservedObject private var prefs = Preferences.shared
    @State private var startAtLogin = SMAppService.mainApp.status == .enabled
    @State private var loginFailure: String?
    @State private var language = (UserDefaults.standard.stringArray(forKey: "AppleLanguages")?.first).map { String($0.prefix(2)) } ?? ""
    @State private var languageChanged = false

    var body: some View {
        Form {
            Section("Appearance") {
                Picker("Theme", selection: $prefs.theme) {
                    Text("Follow System").tag("system")
                    Text("Light").tag("light")
                    Text("Dark").tag("dark")
                }
            }
            Section("Behavior") {
                Toggle("Run in Background", isOn: $prefs.runInBackground)
                Toggle(isOn: $prefs.disconnectOnQuit) {
                    Text("Disconnect When Quitting")
                    Text("Once the app is closed, no icon remains to control the VPN")
                }
                Toggle("Start at Login", isOn: Binding(
                    get: { startAtLogin },
                    set: { setStartAtLogin($0) }
                ))
                if let loginFailure {
                    Text(loginFailure).font(.caption).foregroundStyle(errorColor)
                }
                Toggle("Check for Updates", isOn: $prefs.checkUpdates)
            }
            Section("Language") {
                Picker("Interface Language", selection: Binding(
                    get: { language },
                    set: { setLanguage($0) }
                )) {
                    Text("System Language").tag("")
                    Text(verbatim: "English").tag("en")
                    Text(verbatim: "Français").tag("fr")
                }
                if languageChanged {
                    HStack {
                        Text("Restart Tunnelbird to apply the new language").font(.caption)
                        Spacer()
                        Button("Restart", action: restart)
                    }
                }
            }
        }
        .formStyle(.grouped)
    }

    private func setStartAtLogin(_ on: Bool) {
        do {
            if on {
                try SMAppService.mainApp.register()
            } else {
                try SMAppService.mainApp.unregister()
            }
            startAtLogin = on
            loginFailure = nil
        } catch {
            loginFailure = String(localized: "Cannot change start at login")
        }
    }

    private func setLanguage(_ code: String) {
        language = code
        if code.isEmpty {
            UserDefaults.standard.removeObject(forKey: "AppleLanguages")
        } else {
            UserDefaults.standard.set([code], forKey: "AppleLanguages")
        }
        languageChanged = true
    }

    private func restart() {
        let configuration = NSWorkspace.OpenConfiguration()
        configuration.createsNewApplicationInstance = true
        NSWorkspace.shared.openApplication(at: Bundle.main.bundleURL, configuration: configuration) { _, _ in
            DispatchQueue.main.async { NSApp.terminate(nil) }
        }
    }
}

private struct AutoConnectTab: View {
    @EnvironmentObject private var model: Model
    @ObservedObject private var prefs = Preferences.shared
    @ObservedObject private var wifi = WiFiAccess.shared
    @State private var saved: [String] = []

    var body: some View {
        Form {
            Section {
                Text("Tunnelbird connects a tunnel by itself when you join a network you do not trust, such as café or hotel Wi-Fi. It never disconnects anything by itself.")
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
                Toggle("Connect on Untrusted Networks", isOn: $prefs.autoConnect)
                Picker("Tunnel to Connect", selection: $prefs.autoTunnel) {
                    if model.tunnels.isEmpty {
                        Text("Import a tunnel first").tag(prefs.autoTunnel)
                    } else {
                        if !model.tunnels.contains(where: { $0.id == prefs.autoTunnel }) {
                            Text("Choose…").tag(prefs.autoTunnel)
                        }
                        ForEach(model.tunnels) { Text($0.name).tag($0.id) }
                    }
                }
                Toggle(isOn: $prefs.trustWired) {
                    Text("Trust Wired Networks")
                    Text("A cable usually means home or office")
                }
                Toggle(isOn: $prefs.strict) {
                    Text("Strict Verification")
                    Text("Also check the hardware of the Wi-Fi router, so a hotspot with the same name is not trusted. Best for networks without repeaters.")
                }
            }
            if !wifi.allowed {
                Section {
                    Label("macOS only tells apps the name of your Wi-Fi with the location permission. Without it, every Wi-Fi counts as untrusted.", systemImage: "location")
                        .font(.callout)
                        .fixedSize(horizontal: false, vertical: true)
                    Button("Allow Access to the Wi-Fi Name…") { wifi.ask() }
                }
            }
            Section("Trusted Wi-Fi Networks") {
                if let ssid = wifi.ssid, !prefs.trusted.contains(ssid) {
                    Button {
                        trust(ssid, bssid: wifi.bssid)
                    } label: {
                        Label("Trust “\(ssid)”", systemImage: "plus")
                    }
                    .help("The Wi-Fi network you are on now")
                }
                ForEach(prefs.trusted, id: \.self) { ssid in
                    HStack {
                        Image(systemName: "wifi").foregroundStyle(.secondary)
                        VStack(alignment: .leading) {
                            Text(ssid)
                            if prefs.strict {
                                let count = prefs.pins.filter { $0.ssid == ssid }.count
                                Text(count == 0
                                     ? String(localized: "Its access point will be remembered the next time you connect")
                                     : String(localized: "Checked against \(count) access point(s)"))
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                            }
                        }
                        Spacer()
                        Button {
                            prefs.trusted.removeAll { $0 == ssid }
                            prefs.pins.removeAll { $0.ssid == ssid }
                        } label: {
                            Image(systemName: "trash")
                        }
                        .buttonStyle(.borderless)
                        .help("Stop Trusting")
                        .accessibilityLabel(Text("Stop Trusting"))
                    }
                }
                let candidates = saved.filter { !prefs.trusted.contains($0) }
                if !candidates.isEmpty {
                    Menu {
                        ForEach(candidates, id: \.self) { ssid in
                            Button(ssid) { trust(ssid, bssid: nil) }
                        }
                    } label: {
                        Label("Add a Saved Wi-Fi Network", systemImage: "plus")
                    }
                }
            }
        }
        .formStyle(.grouped)
        .task { saved = wifi.savedNetworks() }
    }

    private func trust(_ ssid: String, bssid: String?) {
        prefs.trusted.append(ssid)
        if let bssid { prefs.pins.append(AccessPoint(bssid: bssid, ssid: ssid)) }
    }
}
