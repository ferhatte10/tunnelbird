// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

// Public IP address and approximate location, as the internet sees them, from
// the same service as the Linux app (src/geo.rs): HTTPS, no account.

import Foundation

struct FoundLocation: Decodable, Equatable {
    let ip: String
    let country: String?
    let countryCode: String?
    let city: String?
    let operatorName: String?

    enum CodingKeys: String, CodingKey {
        case ip, country, city
        case countryCode = "country_iso"
        case operatorName = "asn_org"
    }

    /// “City, Country”, the country in the interface's language if known.
    var place: String? {
        let name = countryCode.flatMap { Locale.current.localizedString(forRegionCode: $0) } ?? country
        let parts = [city, name].compactMap { $0 }.filter { !$0.isEmpty }
        return parts.isEmpty ? nil : parts.joined(separator: ", ")
    }
}

enum LocationState: Equatable {
    case idle, locating, failed
    case ready(FoundLocation)
}

func lookUpLocation() async -> FoundLocation? {
    #if DEBUG
    // Snapshots must never show this computer's real address.
    if ProcessInfo.processInfo.environment["TUNNELBIRD_SNAPSHOT"] != nil {
        return FoundLocation(
            ip: "203.0.113.42", country: "Netherlands", countryCode: "NL",
            city: "Amsterdam", operatorName: "Example Hosting")
    }
    #endif
    guard let url = URL(string: "https://ifconfig.co/json") else { return nil }
    var request = URLRequest(url: url, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: 10)
    request.setValue("application/json", forHTTPHeaderField: "Accept")
    // Ephemeral: nothing about this lookup is kept on disk.
    let session = URLSession(configuration: .ephemeral)
    defer { session.finishTasksAndInvalidate() }
    guard let (data, response) = try? await session.data(for: request),
          (response as? HTTPURLResponse)?.statusCode == 200 else { return nil }
    return try? JSONDecoder().decode(FoundLocation.self, from: data)
}

/// The flag of a two-letter country code, from its regional indicator letters.
func flagEmoji(_ code: String) -> String? {
    let letters = code.uppercased().unicodeScalars
    guard letters.count == 2, letters.allSatisfy({ ("A"..."Z").contains($0) }) else { return nil }
    var flag = ""
    for letter in letters {
        guard let scalar = Unicode.Scalar(0x1F1E6 + letter.value - 65) else { return nil }
        flag.unicodeScalars.append(scalar)
    }
    return flag
}
