// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

import AppKit
import SwiftUI

func formatSize(_ bytes: Double) -> String {
    let formatter = ByteCountFormatter()
    formatter.countStyle = .decimal
    // Otherwise 0 is written as a word (“Zero KB”).
    formatter.allowsNonnumericFormatting = false
    formatter.allowedUnits = [.useKB, .useMB, .useGB, .useTB]
    return formatter.string(fromByteCount: Int64(bytes))
}

func formatRate(_ bytesPerSecond: Double) -> String {
    String(localized: "\(formatSize(bytesPerSecond))/s")
}

/// The system's orange has the same problem as its green on a light background.
let warningColor = Color(nsColor: NSColor(name: nil) { appearance in
    appearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua
        ? NSColor(srgbRed: 1.00, green: 0.70, blue: 0.25, alpha: 1)
        : NSColor(srgbRed: 0.65, green: 0.33, blue: 0.00, alpha: 1)
})

/// The system's green is too pale on a light background (about 2:1): a darker
/// one in light mode keeps the “Connected” text readable (about 5:1).
let connectedColor = Color(nsColor: NSColor(name: nil) { appearance in
    appearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua
        ? NSColor(srgbRed: 0.19, green: 0.82, blue: 0.35, alpha: 1)
        : NSColor(srgbRed: 0.10, green: 0.50, blue: 0.23, alpha: 1)
})

/// The same care for errors written on a tinted background.
let errorColor = Color(nsColor: NSColor(name: nil) { appearance in
    appearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua
        ? NSColor(srgbRed: 1.00, green: 0.50, blue: 0.47, alpha: 1)
        : NSColor(srgbRed: 0.70, green: 0.11, blue: 0.11, alpha: 1)
})
