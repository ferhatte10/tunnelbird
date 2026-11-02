#!/bin/zsh
# Puts OpenVPN and its libraries in the app, from Homebrew, so that nobody needs
# Homebrew to use an OpenVPN profile. Usage: bundle-openvpn.sh RESOURCES_FOLDER
set -e
RESOURCES=$1
OPENVPN=$(brew --prefix openvpn)
TARGET=$RESOURCES/openvpn
mkdir -p "$TARGET/lib" "$RESOURCES/Licenses"
cp "$OPENVPN/sbin/openvpn" "$TARGET/openvpn"
cp "$OPENVPN/libexec/openvpn/dns-updown" "$TARGET/dns-updown"
chmod u+w "$TARGET/openvpn"

homebrew_deps() { otool -L "$1" | sed 1d | awk '{print $1}' | grep -E '^/opt/homebrew|^/usr/local' || true }

# Copies the libraries a file needs, then those they need in turn.
pending=("$TARGET/openvpn")
while (( $#pending )); do
  file=$pending[1]; shift pending
  for dep in $(homebrew_deps "$file"); do
    name=${dep:t}
    if [[ ! -e $TARGET/lib/$name ]]; then
      cp -L "$dep" "$TARGET/lib/$name"
      chmod u+w "$TARGET/lib/$name"
      pending+=("$TARGET/lib/$name")
    fi
    if [[ $file == */lib/* ]]; then
      install_name_tool -change "$dep" "@loader_path/$name" "$file" 2>/dev/null
    else
      install_name_tool -change "$dep" "@executable_path/lib/$name" "$file" 2>/dev/null
    fi
  done
  [[ $file == */lib/* ]] && install_name_tool -id "@loader_path/${file:t}" "$file" 2>/dev/null
done
# Changing a file breaks its signature; macOS kills what has none on Apple silicon.
for file in "$TARGET"/lib/* "$TARGET/openvpn"; do
  codesign --force --sign - "$file" 2>/dev/null
done

# What their licenses ask of whoever passes them on.
license() { # name formula file
  cat "$(brew --prefix "$2")/$3" > "$RESOURCES/Licenses/$1.txt"
}
license OpenVPN openvpn COPYING
license OpenSSL openssl@4 LICENSE.txt
license LZO lzo COPYING
license LZ4 lz4 LICENSE
license pkcs11-helper pkcs11-helper COPYING
