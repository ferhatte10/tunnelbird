// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

// What the interface shows, kept up to date from the service. All the tunnel
// logic stays in the service: this only shows things and sends clicks.

import AppKit
import Foundation
import Network
import SwiftUI
import UserNotifications

struct Sample {
    let rx: Double
    let tx: Double
}

struct ProblemReport: Identifiable {
    let id = UUID()
    let title: String
    let details: String
}

struct LatestVersion: Decodable {
    let version: String
}

struct PasswordRequest: Identifiable {
    let id: String
    let name: String
}

@MainActor
final class Model: ObservableObject {
    /// Trafic over the last minute, one sample a second.
    static let graphLength = 60

    @Published var ready = false
    /// Until the first try is over, there is nothing to say about the service.
    @Published var checked = false
    @Published var failure: String?
    @Published var tunnels: [Tunnel] = []
    @Published var busy: Set<String> = []
    @Published var samples: [Sample] = []
    @Published var totals: Counters?
    @Published var problem: ProblemReport?
    @Published var passwordRequest: PasswordRequest?
    @Published var serverResponds = true
    @Published var location = LocationState.idle
    /// The logs on screen, and their tunnel; the service keeps them all.
    @Published var logsTunnel: Tunnel?
    @Published var logLines: [LogLine] = []
    @Published var updateAvailable: String?
    @Published var editedTunnel: Tunnel?
    @Published var qrTunnel: Tunnel?
    @Published var importRequested = false
    @Published var installing = false

    private var connection: ServiceConnection?
    private var last: (date: Date, counters: Counters)?
    private var locationTask: Task<Void, Never>?
    private var locatedTunnel: String?
    private let pathMonitor = NWPathMonitor()
    private var lastNetwork: String?
    private var settling: Task<Void, Never>?
    private var warnedNoResponse = false
    /// Snapshots stay away from the network, the notifications and the Wi-Fi.
    private let quiet = ProcessInfo.processInfo.environment["TUNNELBIRD_SNAPSHOT"] != nil
    private let socketPath = ProcessInfo.processInfo.environment["TUNNELBIRD_SOCKET"]
        ?? "/var/run/tunnelbird.sock"

    var connected: Tunnel? { tunnels.first { $0.state == .connected } }

    /// For the app delegate, which SwiftUI cannot hand the model to.
    private(set) static weak var current: Model?

    init() {
        Self.current = self
        Task { await connectLoop() }
        guard !quiet else { return }
        UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound]) { _, _ in }
        // Only says that something changed: see `currentNetworkKind`.
        pathMonitor.pathUpdateHandler = { _ in
            Task { @MainActor [weak self] in self?.networkChanged() }
        }
        pathMonitor.start(queue: .main)
        Task { await checkForUpdate() }
    }

    private func notify(id: String, title: String, body: String) {
        guard !quiet else { return }
        let content = UNMutableNotificationContent()
        content.title = title
        content.body = body
        UNUserNotificationCenter.current().add(
            UNNotificationRequest(identifier: id, content: content, trigger: nil))
    }

    /// A network that goes down or comes up changes several times in a row:
    /// the rule waits until it has settled, and only the last state counts.
    private func networkChanged() {
        settling?.cancel()
        settling = Task {
            try? await Task.sleep(for: .seconds(3))
            if !Task.isCancelled { await applyTrustRule(WiFiAccess.shared.currentNetworkKind()) }
        }
    }

    /// Applies the rule of trusted networks when the computer joins a network:
    /// a user who disconnects by hand afterwards is respected.
    private func applyTrustRule(_ kind: String) async {
        guard !kind.isEmpty else {
            lastNetwork = nil
            return
        }
        let wifi = WiFiAccess.shared
        let ssid = kind == "wifi" ? wifi.ssid : nil
        let identity = "\(kind):\(ssid ?? "")"
        // With a tunnel up the system only sees the tunnel: not a network joined.
        guard identity != lastNetwork, tunnels.allSatisfy({ $0.state == .disconnected }),
              ready, let connection else { return }
        lastNetwork = identity
        let prefs = Preferences.shared
        guard prefs.autoConnect else { return }
        var network: [String: Any] = ["uuid": identity, "kind": kind]
        if let ssid { network["ssid"] = ssid }
        if kind == "wifi", let bssid = wifi.bssid { network["bssid"] = bssid }
        guard let answer = try? await connection.send(
            ["request": "trust-decision", "policy": prefs.policy, "network": network]),
            let decision = answer.decision, !Task.isCancelled else { return }
        switch decision.decision {
        case "pin":
            if let bssid = decision.bssid, let ssid = decision.ssid {
                prefs.pins.append(AccessPoint(bssid: bssid, ssid: ssid))
            }
        case "connect":
            guard let tunnel = tunnels.first(where: { $0.id == prefs.autoTunnel }),
                  tunnels.allSatisfy({ $0.state == .disconnected }) else { return }
            let place = ssid.map { String(localized: "Wi-Fi “\($0)”") } ?? String(localized: "This network")
            notify(
                id: "auto-connect",
                title: String(localized: "Connect “\(tunnel.name)”"),
                body: String(localized: "\(place) is not one of your trusted networks."))
            connect(tunnel)
        default:
            break
        }
    }

    private func checkForUpdate() async {
        let prefs = Preferences.shared
        let defaults = UserDefaults.standard
        guard prefs.checkUpdates, !quiet else { return }
        updateAvailable = defaults.string(forKey: "update-available")
        let last = defaults.double(forKey: "last-update-check")
        let now = Date().timeIntervalSince1970
        guard now < last || now - last >= 24 * 60 * 60,
              let url = URL(string: "https://tunnelbird.ferhatsn.fr/latest.json") else { return }
        let session = URLSession(configuration: .ephemeral)
        defer { session.finishTasksAndInvalidate() }
        guard let (data, response) = try? await session.data(from: url),
              (response as? HTTPURLResponse)?.statusCode == 200,
              let latest = try? JSONDecoder().decode(LatestVersion.self, from: data) else { return }
        defaults.set(now, forKey: "last-update-check")
        let current = Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "0"
        let candidate = latest.version.trimmingCharacters(in: CharacterSet(charactersIn: "v"))
        if candidate.compare(current, options: .numeric) == .orderedDescending {
            updateAvailable = candidate
            defaults.set(candidate, forKey: "update-available")
        } else {
            updateAvailable = nil
            defaults.removeObject(forKey: "update-available")
        }
    }

    func openDownloadPage() {
        if let url = URL(string: "https://tunnelbird.ferhatsn.fr/download") {
            NSWorkspace.shared.open(url)
        }
    }

    /// The tunnel's file, keys included, for the editor and sharing.
    func exported(_ tunnel: Tunnel) async throws -> String {
        guard let connection else {
            throw ServiceError(code: "unavailable", details: String(localized: "The service is not running"))
        }
        return try await connection.send(["request": "export", "tunnel": tunnel.id]).text ?? ""
    }

    func update(_ tunnel: Tunnel, name: String, text: String, login: (String, String)?) async throws {
        guard let connection else {
            throw ServiceError(code: "unavailable", details: String(localized: "The service is not running"))
        }
        var body: [String: Any] = ["request": "update", "tunnel": tunnel.id, "name": name, "text": text]
        if let login {
            body["login"] = ["username": login.0, "password": login.1]
        }
        _ = try await connection.send(body)
    }

    /// Keeps a connection to the service, which may start later than the app.
    private func connectLoop() async {
        while true {
            if connection == nil {
                await open()
            }
            try? await Task.sleep(for: .seconds(2))
        }
    }

    private func open() async {
        defer { checked = true }
        do {
            let opened = try ServiceConnection(path: socketPath)
            connection = opened
            opened.start(
                onEvent: { message in Task { @MainActor [weak self] in self?.handle(message) } },
                onClose: { Task { @MainActor [weak self] in self?.lost(opened) } })
            _ = try await opened.send(["request": "hello", "version": protocolVersion])
            let answer = try await opened.send(["request": "list-tunnels"])
            tunnels = answer.tunnels ?? []
            updateLocation()
            ready = true
            failure = nil
        } catch let error as ServiceError {
            connection = nil
            ready = false
            failure = error.details
        } catch {
            connection = nil
            ready = false
            failure = error.localizedDescription
        }
    }

    private func lost(_ closed: ServiceConnection) {
        guard connection === closed else { return }
        connection = nil
        ready = false
        failure = String(localized: "The connection to the service was lost")
        tunnels = []
        resetTraffic()
        updateLocation()
    }

    private func handle(_ message: ServiceMessage) {
        switch message.event {
        case "tunnels-changed":
            tunnels = message.tunnels ?? []
            if connected == nil { resetTraffic() }
            updateLocation()
        case "state":
            guard let id = message.tunnelID, let state = message.state,
                  let index = tunnels.firstIndex(where: { $0.id == id }) else { return }
            let wasConnected = tunnels[index].state == .connected || tunnels[index].state == .disconnecting
            let name = tunnels[index].name
            tunnels[index].state = state
            if let error = message.error {
                if wasConnected {
                    // Stopped on its own: the traffic no longer goes through the VPN.
                    notify(
                        id: "dropped",
                        title: String(localized: "VPN disconnected"),
                        body: String(localized: "“\(name)” stopped on its own. Your traffic no longer goes through the VPN."))
                } else {
                    problem = ProblemReport(
                        title: String(localized: "Could not connect “\(name)”"),
                        details: error)
                }
            }
        case "traffic":
            if let counters = message.counters { addSample(counters) }
            serverResponds = message.health != "no-response"
            if !serverResponds, !warnedNoResponse, let tunnel = connected {
                warnedNoResponse = true
                notify(
                    id: "no-response",
                    title: String(localized: "VPN server not responding"),
                    body: String(localized: "“\(tunnel.name)” is connected, but its server does not answer. Check your internet connection or try another server."))
            } else if serverResponds {
                warnedNoResponse = false
            }
        case "log":
            if let line = message.line, message.tunnelID == logsTunnel?.id {
                logLines.append(line)
            }
        case "password-needed":
            guard let id = message.tunnelID,
                  let tunnel = tunnels.first(where: { $0.id == id }) else { return }
            passwordRequest = PasswordRequest(id: id, name: tunnel.name)
        default:
            break
        }
    }

    private func addSample(_ counters: Counters) {
        let now = Date()
        if let last, counters.rxBytes >= last.counters.rxBytes, counters.txBytes >= last.counters.txBytes {
            let seconds = max(now.timeIntervalSince(last.date), 0.1)
            samples.append(Sample(
                rx: Double(counters.rxBytes - last.counters.rxBytes) / seconds,
                tx: Double(counters.txBytes - last.counters.txBytes) / seconds))
            if samples.count > Self.graphLength {
                samples.removeFirst(samples.count - Self.graphLength)
            }
        }
        last = (now, counters)
        totals = counters
    }

    private func resetTraffic() {
        samples = []
        totals = nil
        last = nil
        serverResponds = true
        warnedNoResponse = false
    }

    /// Looks the public address up once per connected full tunnel: what the
    /// internet sees then is the VPN's exit, and the request goes through it.
    private func updateLocation() {
        guard let tunnel = connected, tunnel.fullTunnel else {
            locationTask?.cancel()
            locatedTunnel = nil
            location = .idle
            return
        }
        if locatedTunnel != tunnel.id { locate(tunnel.id) }
    }

    func relocate() {
        if let tunnel = connected, tunnel.fullTunnel { locate(tunnel.id) }
    }

    private func locate(_ id: String) {
        locatedTunnel = id
        location = .locating
        locationTask?.cancel()
        locationTask = Task {
            // Leaves the tunnel a moment to carry traffic, then one more try.
            try? await Task.sleep(for: .seconds(2))
            var result = await lookUpLocation()
            if result == nil, !Task.isCancelled {
                try? await Task.sleep(for: .seconds(5))
                result = await lookUpLocation()
            }
            if Task.isCancelled || locatedTunnel != id { return }
            location = result.map(LocationState.ready) ?? .failed
        }
    }

    private func perform(on tunnel: Tunnel, title: String, _ work: @escaping (ServiceConnection) async throws -> Void) {
        guard let connection else { return }
        busy.insert(tunnel.id)
        Task {
            do {
                try await work(connection)
            } catch let error as ServiceError {
                // A failed connection also arrives as a `state` event.
                if error.code != "connection-failed" {
                    problem = ProblemReport(title: title, details: error.details)
                }
            } catch {
                problem = ProblemReport(title: title, details: error.localizedDescription)
            }
            busy.remove(tunnel.id)
        }
    }

    /// Only ever connects: what the automatic rule uses, unlike a click.
    private func connect(_ tunnel: Tunnel) {
        perform(on: tunnel, title: String(localized: "Cannot connect “\(tunnel.name)”")) {
            _ = try await $0.send(["request": "connect", "tunnel": tunnel.id])
        }
    }

    func toggle(_ tunnel: Tunnel) {
        if tunnel.state == .disconnected {
            connect(tunnel)
        } else {
            perform(on: tunnel, title: String(localized: "Cannot disconnect “\(tunnel.name)”")) {
                _ = try await $0.send(["request": "disconnect", "tunnel": tunnel.id])
            }
        }
    }

    func showLogs(_ tunnel: Tunnel) {
        logLines = []
        logsTunnel = tunnel
        guard let connection else { return }
        Task {
            let answer = try? await connection.send(["request": "logs", "tunnel": tunnel.id])
            if logsTunnel?.id == tunnel.id {
                // Lines that arrived while waiting are in the answer too.
                logLines = answer?.lines ?? logLines
            }
        }
    }

    /// True once the service was installed, running or not.
    var serviceInstalled: Bool {
        FileManager.default.fileExists(
            atPath: "/Library/LaunchDaemons/fr.ferhatsn.Tunnelbird.Service.plist")
    }

    /// The service carried by the app, if this build has one.
    var serviceInApp: URL? {
        Bundle.main.url(forResource: "tunnelbird-service", withExtension: nil)
    }

    /// Installs or removes the service: macOS asks the administrator's password.
    func runServiceAction(_ argument: String) {
        guard let service = serviceInApp, !installing else { return }
        installing = true
        let prompt = String(localized: "Tunnelbird needs your password once, to install the service that creates the tunnels.")
        let command = "\"'\(service.path)' \(argument)\""
        let source = "do shell script \(command) with prompt \"\(prompt)\" with administrator privileges"
        Task.detached {
            var failure: NSDictionary?
            NSAppleScript(source: source)?.executeAndReturnError(&failure)
            let message = failure?[NSAppleScript.errorMessage] as? String
            let cancelled = failure?[NSAppleScript.errorNumber] as? Int == -128
            await MainActor.run { [weak self] in
                if let message, !cancelled {
                    self?.problem = ProblemReport(
                        title: String(localized: "Cannot install the service"),
                        details: message)
                }
                guard failure == nil, argument == "--install-daemon" else {
                    self?.installing = false
                    return
                }
                Task { await self?.waitForService() }
            }
        }
    }

    /// launchd takes a moment to start what was just installed: the screen keeps
    /// saying that it is installing rather than showing a service in trouble.
    private func waitForService() async {
        // A connection to the service that was just replaced is of no use.
        connection?.disconnect()
        for _ in 0..<30 where !ready {
            if connection == nil { await open() }
            if !ready { try? await Task.sleep(for: .milliseconds(500)) }
        }
        installing = false
    }

    /// Asks for every tunnel up to go down, and waits for the answers.
    func disconnectAll() async {
        guard let connection else { return }
        for tunnel in tunnels where tunnel.state != .disconnected {
            _ = try? await connection.send(["request": "disconnect", "tunnel": tunnel.id])
        }
    }

    func delete(_ tunnel: Tunnel) {
        perform(on: tunnel, title: String(localized: "Cannot delete “\(tunnel.name)”")) {
            _ = try await $0.send(["request": "delete", "tunnel": tunnel.id])
        }
    }

    /// Throws so that the import dialog can stay open and show why.
    func importTunnel(name: String, text: String, login: (String, String)?, files: [String: String]) async throws {
        guard let connection else {
            throw ServiceError(code: "unavailable", details: String(localized: "The service is not running"))
        }
        var body: [String: Any] = ["request": "import", "name": name, "text": text]
        if !files.isEmpty { body["files"] = files }
        if let login {
            body["login"] = ["username": login.0, "password": login.1]
        }
        _ = try await connection.send(body)
    }

    /// The password an OpenVPN server asks for: stored with the tunnel by an
    /// `update`, then the tunnel connects again.
    func answerPassword(_ request: PasswordRequest, username: String, password: String) {
        guard let tunnel = tunnels.first(where: { $0.id == request.id }) else { return }
        perform(on: tunnel, title: String(localized: "Cannot connect “\(tunnel.name)”")) {
            let exported = try await $0.send(["request": "export", "tunnel": tunnel.id])
            _ = try await $0.send([
                "request": "update", "tunnel": tunnel.id, "name": tunnel.name,
                "text": exported.text ?? "",
                "login": ["username": username, "password": password],
            ])
            _ = try await $0.send(["request": "connect", "tunnel": tunnel.id])
        }
    }
}
