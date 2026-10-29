# Flatpak

## Local build

```sh
flatpak install flathub org.gnome.Sdk//50 org.freedesktop.Sdk.Extension.rust-stable//25.08
flatpak-builder --user --install --force-clean build-flatpak build-aux/flatpak/fr.ferhatsn.Tunnelbird.json
flatpak run fr.ferhatsn.Tunnelbird
```

The build is **offline**, as Flathub requires: every Rust crate comes from
`cargo-sources.json`, with its checksum. Regenerate that file whenever
`Cargo.lock` changes:

```sh
python3 -m venv /tmp/fcg && /tmp/fcg/bin/pip install aiohttp tomlkit
curl -LO https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/master/cargo/flatpak-cargo-generator.py
/tmp/fcg/bin/python flatpak-cargo-generator.py Cargo.lock -o build-aux/flatpak/cargo-sources.json
```

For the Flathub submission, replace the `dir` source with the project's git
repository (`url` + `tag` + `commit`).

## Permissions

| Permission | Why |
|---|---|
| `--socket=wayland`, `--socket=fallback-x11`, `--share=ipc`, `--device=dri` | Show the window (GPU rendering) |
| `--share=network` | Public IP and location lookup; the host network namespace also exposes tunnel counters in `/sys/class/net` |
| `--system-talk-name=org.freedesktop.NetworkManager` | List, import, connect and edit tunnels |
| `--filesystem=~/.cert/nm-openvpn:create` | OpenVPN certificates and keys, written where NetworkManager's OpenVPN may read them (the folder `nm-connection-editor` uses; on Fedora, SELinux denies other places in the home folder) |
| `--talk-name=org.kde.StatusNotifierWatcher` | Top bar icon (registered under the app's unique bus name, so no `--own-name`) |

No other file system access: imports and exports go through the file chooser portal,
notifications and start at login through their portals.
