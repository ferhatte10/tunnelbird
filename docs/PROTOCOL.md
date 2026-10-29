# Interface ↔ tunnel service protocol (Windows, macOS)

On Windows and macOS, creating a tunnel needs administrator rights, so
Tunnelbird is split in two (see `docs/plans/later.md`):

- the **interface** (WinUI 3 in C# on Windows, SwiftUI on macOS) runs as the
  user and only shows things and sends clicks;
- the **tunnel service** (Rust, on top of `tunnelbird-core`) runs with the
  rights needed (Windows service as SYSTEM, macOS launchd daemon as root),
  stores the tunnels and drives the drivers.

Linux has neither: NetworkManager already is that service.

The reference is `crates/core/src/protocol.rs` (types and tests). This page
shows the same thing as JSON, for the C# and Swift sides.

## Channel

- Local only: named pipe `\\.\pipe\tunnelbird` on Windows, Unix socket
  `/var/run/tunnelbird.sock` on macOS. Never a network port.
- Access: the logged-in interactive users and administrators only (pipe
  security descriptor; socket owned by `root:admin`, mode 0660).
- One JSON object per line, UTF-8, `\n` at the end.
- The interface sends requests with an `id` of its choice; the service answers
  each one with a response carrying the same `id`. The service also pushes
  events, without `id`, at any time.
- Version: the first request is `hello`; any other request before a matching
  version is refused with `version-mismatch`. Current version: **1**.

## Requests (interface → service)

```json
{"id":1,"request":"hello","version":1}
{"id":2,"request":"list-tunnels"}
{"id":3,"request":"import","name":"Office","text":"client\nremote vpn.example.com 1194\n…","login":{"username":"alex","password":""}}
{"id":4,"request":"update","tunnel":"a1","name":"Office","text":"…","login":{"username":"alex","password":"secret"}}
{"id":5,"request":"delete","tunnel":"a1"}
{"id":6,"request":"connect","tunnel":"a1"}
{"id":7,"request":"disconnect","tunnel":"a1"}
{"id":8,"request":"export","tunnel":"a1"}
```

- `text` is the `.conf` or `.ovpn` file as the user edited it, certificates
  inline. The interface checks it first with the same parser (the core), the
  service checks it again.
- `login` only for OpenVPN profiles that ask for a user name and password.
  An empty password is asked at each connection (`password-needed`).
- `connect` disconnects any other tunnel first: one at a time, as on Linux.

## Responses (service → interface)

```json
{"id":1,"response":"hello","version":1}
{"id":2,"response":"tunnels","tunnels":[{"id":"a1","name":"Office","kind":"openvpn","endpoint":"vpn.example.com:1194","addresses":[],"full_tunnel":true,"state":"disconnected"}]}
{"id":3,"response":"tunnel","tunnel":{"id":"a1","name":"Office","kind":"openvpn","addresses":[],"full_tunnel":true,"state":"disconnected"}}
{"id":5,"response":"ok"}
{"id":8,"response":"text","text":"client\n…"}
{"id":6,"response":"error","code":"connection-failed","message":"The VPN driver is not installed"}
```

- `kind`: `wireguard` or `openvpn`. `provider` and `endpoint` are left out
  when unknown.
- Error codes: `version-mismatch`, `invalid-profile`, `unknown-tunnel`,
  `connection-failed`, `internal`. The interface shows its own translated
  sentence for the `code`, with `message` (English, from the service) as the
  details, like the “Details” button of the Linux app.

## Events (service → interface)

```json
{"event":"tunnels-changed","tunnels":[…]}
{"event":"state","tunnel":"a1","state":"connecting"}
{"event":"state","tunnel":"a1","state":"disconnected","error":"The server does not answer"}
{"event":"traffic","tunnel":"a1","counters":{"rx_bytes":1048576,"tx_bytes":65536,"rx_packets":900,"tx_packets":400}}
{"event":"password-needed","tunnel":"a1"}
```

- States: `disconnected`, `connecting`, `connected`, `disconnecting`.
- `traffic` comes once a second for the connected tunnel; counters are
  cumulative, the interface computes rates and the server health exactly like
  the Linux app (`tunnelbird_core::health`).

## What the service guarantees

- Keys and passwords never leave it except through `export`, which the
  interface only calls for the editor and for sharing.
- Tunnels are stored readable by the service only: on Windows encrypted with
  DPAPI under `%ProgramData%\Tunnelbird`, on macOS in
  `/Library/Application Support/Tunnelbird` (root, mode 0600).
- It never connects anything by itself: automatic connection on untrusted
  networks is decided by the interface (`tunnelbird_core::trust`), which
  sends `connect`.
