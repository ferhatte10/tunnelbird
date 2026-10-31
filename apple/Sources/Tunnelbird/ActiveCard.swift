// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

import SwiftUI

/// The connected tunnel: who it is, and its traffic over the last minute.
struct ActiveCard: View {
    @EnvironmentObject private var model: Model
    let tunnel: Tunnel

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Active Connection").font(.headline)
            VStack(alignment: .leading, spacing: 12) {
                HStack(alignment: .firstTextBaseline) {
                    Text(tunnel.name)
                        .font(.title2.weight(.bold))
                        .lineLimit(1)
                    Spacer()
                    if model.serverResponds {
                        Text("Connected")
                            .font(.callout.weight(.semibold))
                            .foregroundStyle(connectedColor)
                    } else {
                        Label("Server not responding", systemImage: "exclamationmark.triangle.fill")
                            .font(.callout.weight(.semibold))
                            .foregroundStyle(warningColor)
                    }
                }
                Text(serverLine)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .textSelection(.enabled)
                if !model.serverResponds {
                    Text("“\(tunnel.name)” is connected, but its server does not answer. Check your internet connection or try another server.")
                        .font(.callout)
                        .fixedSize(horizontal: false, vertical: true)
                }
                if tunnel.fullTunnel {
                    LocationRow()
                }
                if !tunnel.addresses.isEmpty {
                    Text("Address in the VPN: \(tunnel.addresses.joined(separator: ", "))")
                        .font(.callout)
                        .textSelection(.enabled)
                }
                if !tunnel.fullTunnel {
                    Text("Only the routed networks go through the VPN")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                Divider()
                TrafficGraph(samples: model.samples)
                    .frame(height: 110)
                    .accessibilityLabel(Text("Traffic over the last minute"))
                legend
                if let totals = model.totals {
                    Text("Since connected: \(formatSize(Double(totals.rxBytes))) received · \(formatSize(Double(totals.txBytes))) sent")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
            .padding(16)
            .background(CardBackground())
        }
    }

    private var serverLine: String {
        [tunnel.kindLabel, tunnel.provider, tunnel.endpoint].compactMap { $0 }.joined(separator: " · ")
    }

    private var legend: some View {
        let latest = model.samples.last
        return ViewThatFits(in: .horizontal) {
            HStack(spacing: 20) { download(latest); upload(latest); Spacer(); scaleLabel }
            VStack(alignment: .leading, spacing: 4) { download(latest); upload(latest); scaleLabel }
        }
        .font(.callout)
    }

    private func download(_ latest: Sample?) -> some View {
        HStack(spacing: 6) {
            Circle().fill(TrafficGraph.rxColor).frame(width: 9, height: 9)
            Text("Download")
            Text(formatRate(latest?.rx ?? 0)).fontWeight(.semibold).monospacedDigit()
        }
    }

    private func upload(_ latest: Sample?) -> some View {
        HStack(spacing: 6) {
            Circle().fill(TrafficGraph.txColor).frame(width: 9, height: 9)
            Text("Upload")
            Text(formatRate(latest?.tx ?? 0)).fontWeight(.semibold).monospacedDigit()
        }
    }

    private var scaleLabel: some View {
        Text("scale \(formatRate(TrafficGraph.scale(of: model.samples)))")
            .font(.caption)
            .foregroundStyle(.secondary)
    }
}

/// Where the internet thinks this computer is.
private struct LocationRow: View {
    @EnvironmentObject private var model: Model

    var body: some View {
        HStack(spacing: 12) {
            Text(flag).font(.system(size: 30)).accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                Text(place).font(.body.weight(.semibold))
                if let detail {
                    Text(detail)
                        .font(.callout)
                        .foregroundStyle(.secondary)
                        .textSelection(.enabled)
                }
            }
            Spacer()
            if case .locating = model.location {
                ProgressView().controlSize(.small)
            } else {
                Button {
                    model.relocate()
                } label: {
                    Image(systemName: "arrow.clockwise")
                }
                .buttonStyle(.borderless)
                .help("Check Location Again")
                .accessibilityLabel(Text("Check Location Again"))
            }
        }
    }

    private var flag: String {
        if case .ready(let found) = model.location, let code = found.countryCode,
           let emoji = flagEmoji(code) {
            return emoji
        }
        return "🌐"
    }

    private var place: String {
        switch model.location {
        case .ready(let found): return found.place ?? String(localized: "Unknown location")
        case .failed: return String(localized: "Location unavailable")
        case .idle, .locating: return String(localized: "Locating…")
        }
    }

    private var detail: String? {
        switch model.location {
        case .ready(let found):
            let ip = String(localized: "Public IP: \(found.ip)")
            return [ip, found.operatorName].compactMap { $0 }.joined(separator: " · ")
        case .failed: return String(localized: "Check your connection, then try again.")
        case .idle, .locating: return nil
        }
    }
}

struct TrafficGraph: View {
    static let rxColor = Color(nsColor: .systemBlue)
    static let txColor = Color(nsColor: .systemOrange)
    let samples: [Sample]

    /// A round value above the highest rate, so that the curve keeps its place.
    static func scale(of samples: [Sample]) -> Double {
        let top = samples.map { max($0.rx, $0.tx) }.max() ?? 0
        var decade = 1_000.0
        while true {
            for step in [1.0, 2.0, 5.0] where decade * step >= top {
                return decade * step
            }
            decade *= 10
        }
    }

    var body: some View {
        Canvas { context, size in
            let lines = 4
            for line in 0...lines {
                let y = size.height * CGFloat(line) / CGFloat(lines)
                var path = Path()
                path.move(to: CGPoint(x: 0, y: y))
                path.addLine(to: CGPoint(x: size.width, y: y))
                context.stroke(path, with: .color(.secondary.opacity(0.2)), lineWidth: 1)
            }
            let scale = Self.scale(of: samples)
            draw(\.rx, Self.rxColor, scale, size, &context)
            draw(\.tx, Self.txColor, scale, size, &context)
        }
    }

    private func draw(_ value: KeyPath<Sample, Double>, _ color: Color, _ scale: Double,
                      _ size: CGSize, _ context: inout GraphicsContext) {
        guard samples.count > 1 else { return }
        let step = size.width / CGFloat(Model.graphLength - 1)
        // The newest sample sits on the right edge.
        let start = size.width - step * CGFloat(samples.count - 1)
        var line = Path()
        for (index, sample) in samples.enumerated() {
            let point = CGPoint(
                x: start + step * CGFloat(index),
                y: size.height * (1 - CGFloat(sample[keyPath: value] / scale)))
            if index == 0 { line.move(to: point) } else { line.addLine(to: point) }
        }
        var area = line
        area.addLine(to: CGPoint(x: size.width, y: size.height))
        area.addLine(to: CGPoint(x: start, y: size.height))
        area.closeSubpath()
        context.fill(area, with: .linearGradient(
            Gradient(colors: [color.opacity(0.25), color.opacity(0.03)]),
            startPoint: .zero, endPoint: CGPoint(x: 0, y: size.height)))
        context.stroke(line, with: .color(color), style: StrokeStyle(lineWidth: 2, lineJoin: .round))
    }
}
