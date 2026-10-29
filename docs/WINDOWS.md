# Tunnelbird on Windows: hand-off

Everything needed to continue the Windows version on a Windows PC, with or
without an AI assistant. Read with `docs/plans/later.md` (decisions),
`docs/PROTOCOL.md` (interface ↔ service messages) and `docs/COMPATIBILITY.md`.

## Decisions already taken (do not reopen)

- **No Microsoft Store, nothing paid**: an installer (`.msi`) downloaded from
  the website. Code signing through the free SignPath Foundation once the
  repository is public; until then SmartScreen shows “Run anyway”.
- **A Windows service** (Rust, runs as SYSTEM) makes the tunnels; the
  **native interface in WinUI 3 (C#)** runs as the user and talks to it over
  the named pipe `\\.\pipe\tunnelbird`. All logic stays in Rust.
- Drivers: **WireGuardNT** (`wireguard.dll`, signed by its authors) for
  WireGuard®, **ovpn-dco-win** with the community `openvpn.exe` for OpenVPN.
  Check their redistribution licences before shipping.
- Targets: Windows 10 (21H2 and later) and 11, x64 and ARM64.
- Identity: the Tunnelbird icon as is, English and French, no legal or
  trademark notice in the interface; WireGuard® only as the tunnel type.

## What exists (written on Linux, NOT YET RUN on Windows)

| Piece | Where | State |
|---|---|---|
| Shared logic and tunnel plan | `crates/core` | Tested (36 tests) |
| Service logic, storage, transport | `crates/service/src/{service,store,transport}.rs` | Tested on Linux with a fake backend |
| WireGuardNT through `wireguard.dll` (loaded from the service's folder) | `crates/service/src/windows/wireguard_nt.rs` | Compiles for `x86_64-pc-windows-msvc`, never run |
| Addresses, routes, server bypass route, DNS, counters (IP Helper API) | `windows/net.rs` | Same |
| OpenVPN with ovpn-dco-win (`openvpn\openvpn.exe` next to the service) | `windows/openvpn.rs` | Same |
| Named pipe (SYSTEM/admins all, interactive users read/write, no remote) | `windows/pipe.rs` | Same |
| Storage encrypted with DPAPI in `%ProgramData%\Tunnelbird` (SYSTEM/admins only) | `windows/dpapi.rs`, `store.rs` | Same |
| Windows service, `--install-service` / `--uninstall-service` | `windows/mod.rs` | Same |
| WinUI 3 interface | — | To write |
| `.msi` installer | — | To write |

## First run, step by step

1. Install the tools: Rust (rustup, MSVC toolchain), Visual Studio 2022
   Community (free) with “.NET desktop development” and “Windows App SDK”,
   Git.
2. Get the code and switch to `feat/tunnelbird-v0.1` (or `main`).
3. `cargo build -p tunnelbird-service --release` and `cargo test -p
   tunnelbird-core -p tunnelbird-service`.
4. **Check the WireGuardNT structures first**: download WireGuardNT from
   <https://download.wireguard.com/wireguard-nt/>, compare `wireguard.h` with
   `windows/wireguard_nt.rs` (struct fields, `ALIGNED(8)`, flag values,
   function signatures). Copy `bin\amd64\wireguard.dll` next to
   `target\release\tunnelbird-service.exe`.
5. Protocol check without the network:
   `tunnelbird-service --fake`, then talk to `\\.\pipe\tunnelbird` (a small
   PowerShell client with `System.IO.Pipes.NamedPipeClientStream`, one JSON
   line per request, see docs/PROTOCOL.md).
6. Real tunnel, from an administrator terminal:
   `tunnelbird-service --install-service` (installs and starts the service),
   then through the pipe: `hello`, `import` a WireGuard® file, `connect`.
   Check: the “Tunnelbird” adapter in network settings, `route print`,
   `Get-DnsClientServerAddress`, a website opens, the public IP changed;
   `disconnect`: adapter gone, routes and DNS back. Log:
   `%ProgramData%\Tunnelbird\service.log`.
7. OpenVPN: put the OpenVPN 2.6+ community `openvpn.exe` (and its DLLs) in an
   `openvpn` folder next to the service, install the ovpn-dco-win driver,
   connect an `.ovpn` profile (test server: `build-aux/test-server/aws.sh up`).
8. `tunnelbird-service --uninstall-service` to remove it.

Points to watch (reasoned, not verified): the layout of `WIREGUARD_PEER`
(`SOCKADDR_INET` after a `WORD`); `SetInterfaceDnsSettings` needs Windows 10
2004+; full-tunnel DNS leaks (consider NRPT rules or a firewall rule later);
OpenVPN's “Initialization Sequence Completed” wait (45 s).

## Still to build

1. **WinUI 3 app** (`windows-app/` folder, C#, Windows App SDK): connects to
   the pipe, JSON lines of docs/PROTOCOL.md, tunnel list with connect switch,
   live traffic, import (file picker), editor, preferences (trusted
   networks), notification-area icon (H.NotifyIcon or the Windows App SDK
   equivalent), Mica background, English and French resources.
2. **Installer** (`.msi` with WiX Toolset, free): installs the app, the
   service (`--install-service`), `wireguard.dll`, OpenVPN and the
   ovpn-dco-win driver; uninstall removes the service.
3. CI: build on `windows-latest` (free for public repositories).

## Prompt for an AI session on Windows

Paste this in a Claude Code session opened in the cloned repository:

> You are continuing Tunnelbird, an open-source VPN client (Rust core,
> GTK app on Linux). Read `AGENTS.md`, `docs/WINDOWS.md`,
> `docs/PROTOCOL.md`, `docs/plans/later.md` and `docs/COMPATIBILITY.md`
> first. Decisions are final: no Store, nothing paid; a Rust Windows service
> (`crates/service`, `--service`) makes the tunnels with WireGuardNT and
> ovpn-dco-win; the interface is a native WinUI 3 (C#) app talking to it over
> `\\.\pipe\tunnelbird` with the JSON protocol. Start with “First run, step
> by step”: check `crates/service/src/windows/wireguard_nt.rs` against the
> real `wireguard.h`, build, test with `--fake`, then a real tunnel from an
> administrator terminal. Then build the WinUI 3 app and the WiX installer.
> Rules: English code and docs, French when talking to me, no commit without
> my explicit request, commits on a feature branch, never mention an AI in
> commits, never put the WireGuard logo or a legal notice in the interface,
> keep `docs/COMPATIBILITY.md` honest (“Tested” only when really run).
