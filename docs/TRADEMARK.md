# Use of the WireGuard® trademark

“WireGuard” and the “WireGuard” logo are registered trademarks of Jason A. Donenfeld.
Tunnelbird is an independent project, not affiliated with or endorsed by the WireGuard project.

## What we asked and what was answered (2026-10-29)

Requests were sent to `wireguard-trademark-usage [at] zx2c4.com`, as the
[WireGuard trademark policy](https://www.wireguard.com/trademark-policy/) describes.

| Request | Answer |
|---|---|
| Use the WireGuard logo in the app icon | **Refused**: the logo is off-limits |
| Use “Wire” or “Guard” in the app name | **Refused**: the project was renamed from “WireDeck” to **Tunnelbird** |
| Say in plain text that the app imports WireGuard® configuration files, with the acknowledgment and a link | **Accepted** |

## Rules for contributors

- Never use the WireGuard logo, or anything resembling it, in icons, screenshots or artwork.
- Never put “Wire” or “Guard” in names (app, package, binary, domain).
- Tunnelbird has its own identity: name, icon and texts are ours. The protocol
  name appears **only as the tunnel type**, written `WireGuard®`, from the single
  constant `config::TUNNEL_TYPE`, as allowed above.
- Like provider names (Mullvad, Proton VPN…), the type is shown as a plain
  description of the imported file, without an acknowledgment section: a
  maintainer decision (2026-10-29), relying on fair use to name what a file is.
  If the trademark owner asks for the acknowledgment, add it back to the About
  dialog (`add_legal_section`) and the README.
- OpenVPN is a registered trademark of OpenVPN Inc. It is shown the same way:
  only as the type of an imported `.ovpn` file (`ovpn::TUNNEL_TYPE`), never
  as part of Tunnelbird's name or artwork, never with its logo.
- Technical identifiers required to talk to the system (NetworkManager's
  `"wireguard"` connection type, code comments) are not branding and stay.
