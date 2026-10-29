# OpenVPN test server

A real OpenVPN server to test Tunnelbird's OpenVPN support end to end, and the
test list to go through. For testing only: delete the server afterwards.

| File | What it does |
|---|---|
| `openvpn.sh` | Turns a fresh Ubuntu 24.04 / Debian 12+ machine into an OpenVPN server and writes three client profiles. Works on any machine reachable from your computer |
| `aws.sh` | Creates that machine on AWS (`up`), shows it (`status`), deletes it and everything it created (`down`) |

`openvpn.sh` was checked in containers (2026-10-29): with a real OpenVPN
client, the three profiles connect and reach the server, so do the same
profiles once read and exported by Tunnelbird
(`cargo run -p tunnelbird-core --example ovpn -- profile.ovpn`), and a wrong
password is refused.

## Start (AWS)

```sh
aws login                               # once, opens the AWS sign-in page
chmod +x build-aux/test-server/*.sh
build-aux/test-server/aws.sh up         # about 3 minutes
```

The server is in Frankfurt (`AWS_REGION` to change it), reachable only from
your current public address. Profiles land in `~/tunnelbird-openvpn-test/`:

| Profile | Tests |
|---|---|
| `cert-and-password.ovpn` | Certificates inline + user name and password, the most common provider profile |
| `password-only.ovpn` | User name and password only, no client certificate |
| `separate-files/files.ovpn` + `ca.crt`, `client.crt`, `client.key`, `tls-crypt.key` | Certificates as separate files |
| `credentials.txt` | The test user name and password |

The server sends all traffic through the VPN (`redirect-gateway`), so the
public address and location must change to Frankfurt, Germany.

Stop: `build-aux/test-server/aws.sh down`, then delete `~/tunnelbird-openvpn-test/`
and the test tunnels in Tunnelbird.

## Test list

Start Tunnelbird with `make run`. Report each line as OK or what went wrong.

| # | Test | Expected |
|---|---|---|
| 1 | Import `cert-and-password.ovpn` (button or drag and drop) | Preview: type OpenVPN, server `<address>:1194`, UDP, login section, security: server and client certificates included, encryption `AES-256-GCM:CHACHA20-POLY1305`, extra protection tls-crypt, traffic “decided by the server” |
| 2 | Type the user name from `credentials.txt`, leave the password empty, Import | Tunnel listed with type OpenVPN; `ls ~/.cert/nm-openvpn/` shows a `tunnelbird-…` folder (files mode 600) |
| 3 | Connect | The desktop asks for the password; after typing it: connected, address `10.99.0.x`, flag 🇩🇪, “Frankfurt…, Germany”, public IP of the server |
| 4 | Watch the graph a minute, open a website | Traffic moves; no “server not responding” warning |
| 5 | Top bar menu | Tunnel listed, traffic lines, location, switch works |
| 6 | Disconnect, connect again | No password asked again (kept by the keyring) |
| 7 | Edit the tunnel: type the password, Save, reconnect | Connects without asking |
| 8 | Edit: switch the protocol to TCP, Save, connect | Fails (the server only listens on UDP), with a readable message; switch back to UDP → connects |
| 9 | Share › Export as File… | Saves an `.ovpn` with everything inline and no password; re-importing it works. No “Show QR Code” item for OpenVPN |
| 10 | Import `password-only.ovpn` | Security shows “None: user name and password” for the client certificate; connects with the password |
| 11 | Import `separate-files/files.ovpn` alone | Clear message naming the missing file (`ca.crt`) |
| 12 | Import `files.ovpn` **with** its four files selected together | One tunnel imported, the four files are not reported as errors; connects |
| 13 | Connect a WireGuard® tunnel while the OpenVPN one is up | The OpenVPN tunnel is disconnected first (one at a time) |
| 14 | Delete the OpenVPN tunnels | Gone from the list and from `nmcli connection`; their `~/.cert/nm-openvpn/tunnelbird-…` folders are gone |
| 15 | Same as 1–3 in the Flatpak (`flatpak run fr.ferhatsn.Tunnelbird` after reinstalling it) | Same results |
| 16 | Without the plugin (only if you want to check: `sudo apt remove network-manager-openvpn-gnome network-manager-openvpn`, reinstall afterwards) | At import: dialog “OpenVPN Support Is Not Installed” with the `apt install` command and a copy button |
