// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

// The messages of docs/PROTOCOL.md; the reference is crates/core/src/protocol.rs.

import Foundation

let protocolVersion = 2

enum TunnelState: String, Decodable {
    case disconnected, connecting, connected, disconnecting
}

struct Tunnel: Decodable, Identifiable, Equatable {
    let id: String
    let name: String
    let kind: String
    var endpoint: String?
    var addresses: [String]
    let fullTunnel: Bool
    var provider: String?
    var state: TunnelState

    enum CodingKeys: String, CodingKey {
        case id, name, kind, endpoint, addresses, provider, state
        case fullTunnel = "full_tunnel"
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(String.self, forKey: .id)
        name = try c.decode(String.self, forKey: .name)
        kind = try c.decode(String.self, forKey: .kind)
        endpoint = try c.decodeIfPresent(String.self, forKey: .endpoint)
        addresses = try c.decodeIfPresent([String].self, forKey: .addresses) ?? []
        fullTunnel = try c.decode(Bool.self, forKey: .fullTunnel)
        provider = try c.decodeIfPresent(String.self, forKey: .provider)
        state = try c.decode(TunnelState.self, forKey: .state)
    }

    /// The only place the protocols' names appear: as the type of a file.
    var kindLabel: String { kind == "openvpn" ? "OpenVPN" : "WireGuard®" }
}

struct Counters: Decodable, Equatable {
    let rxBytes: UInt64
    let txBytes: UInt64

    enum CodingKeys: String, CodingKey {
        case rxBytes = "rx_bytes"
        case txBytes = "tx_bytes"
    }
}

struct LogLine: Decodable, Equatable, Identifiable {
    let id = UUID()
    let time: TimeInterval
    let source: String
    let severity: String
    let message: String

    enum CodingKeys: String, CodingKey {
        case time, source, severity, message
    }
}

/// What the rule of trusted networks says (`tunnelbird_core::trust`).
struct TrustDecision: Decodable {
    let decision: String
    let bssid: String?
    let ssid: String?
}

/// Anything the service writes. `tunnel` is an object in answers and an
/// identifier in events, hence the two fields.
struct ServiceMessage: Decodable {
    let id: UInt64?
    let response: String?
    let event: String?
    let tunnels: [Tunnel]?
    let tunnel: Tunnel?
    let tunnelID: String?
    let text: String?
    let code: String?
    let message: String?
    let state: TunnelState?
    let error: String?
    let counters: Counters?
    let health: String?
    let lines: [LogLine]?
    let line: LogLine?
    let decision: TrustDecision?
    let serviceVersion: String?

    enum CodingKeys: String, CodingKey {
        case id, response, event, tunnels, tunnel, text, code, message, state, error, counters, health, lines, line, decision
        case serviceVersion = "service_version"
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decodeIfPresent(UInt64.self, forKey: .id)
        response = try c.decodeIfPresent(String.self, forKey: .response)
        event = try c.decodeIfPresent(String.self, forKey: .event)
        tunnels = try c.decodeIfPresent([Tunnel].self, forKey: .tunnels)
        tunnel = try? c.decodeIfPresent(Tunnel.self, forKey: .tunnel)
        tunnelID = try? c.decodeIfPresent(String.self, forKey: .tunnel)
        text = try c.decodeIfPresent(String.self, forKey: .text)
        code = try c.decodeIfPresent(String.self, forKey: .code)
        message = try c.decodeIfPresent(String.self, forKey: .message)
        state = try c.decodeIfPresent(TunnelState.self, forKey: .state)
        error = try c.decodeIfPresent(String.self, forKey: .error)
        counters = try c.decodeIfPresent(Counters.self, forKey: .counters)
        health = try c.decodeIfPresent(String.self, forKey: .health)
        lines = try c.decodeIfPresent([LogLine].self, forKey: .lines)
        line = try c.decodeIfPresent(LogLine.self, forKey: .line)
        decision = try c.decodeIfPresent(TrustDecision.self, forKey: .decision)
        serviceVersion = try c.decodeIfPresent(String.self, forKey: .serviceVersion)
    }
}

/// A failed request: `code` picks the translated sentence, `details` is the
/// service's English message.
struct ServiceError: Error {
    let code: String
    let details: String
}
