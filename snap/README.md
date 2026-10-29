# Snap

The Ubuntu App Center lists snaps, not Flatpaks: this package brings
Tunnelbird to it.

## Build

Easiest: connect the GitHub repository on <https://snapcraft.io/build>; every
push builds amd64 and arm64 snaps on Canonical's servers.

Locally (needs LXD):

```sh
sudo snap install snapcraft --classic
sudo snap install lxd && sudo lxd init --auto
snapcraft
sudo snap install --dangerous tunnelbird_*.snap
sudo snap connect tunnelbird:network-manager
sudo snap connect tunnelbird:network-observe
```

## Interfaces

| Plug | Why | Auto-connected? |
|---|---|---|
| `network` | Public IP and location lookup | Yes |
| `network-observe` | Tunnel traffic counters | No: request on the forum |
| `network-manager` | List, import, connect and edit tunnels | No: request on the forum |
| `unity7` | Top bar icon | Yes |
| GNOME extension (`desktop`, `wayland`, `x11`, `gsettings`, `opengl`) | Window and theme | Yes |

`network-manager` gives control over the network, so the Snap Store does not
connect it automatically. Before publishing, ask for auto-connection of
`network-manager` and `network-observe` on <https://forum.snapcraft.io>
(category “store-requests”), explaining that Tunnelbird is a VPN client built on
NetworkManager. Until it is granted, users run the two `snap connect` commands.

## Snap specifics in the code

- The top bar icon is registered under the connection's unique bus name and
  ships a pixmap fallback: snap icons are not in the host icon theme.
- Start at login writes `fr.ferhatsn.Tunnelbird.desktop` in the snap's config
  folder with `Exec=tunnelbird --background`; snapd launches it (`autostart`).
- Translations are found through `TUNNELBIRD_LOCALEDIR`.

## Publish

1. Create an Ubuntu One account and register the name: `snapcraft register tunnelbird`.
2. Upload a build and release it to `edge`, test, then `stable`.
