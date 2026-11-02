#!/bin/sh
# Throwaway OpenVPN server to test Tunnelbird's OpenVPN support for real.
#
# Run as root on a fresh Ubuntu 24.04 (or Debian 12+) machine reachable from
# the internet, for example a small cloud instance:
#
#   sudo sh openvpn.sh
#
# It installs OpenVPN and Easy-RSA, creates a private certificate authority,
# starts the server (UDP 1194, all traffic through the VPN, NAT to the
# internet) and writes three client profiles to /root/tunnelbird-test/:
#
#   cert-and-password.ovpn   client certificate + user name and password,
#                            everything inline (most provider profiles)
#   password-only.ovpn       user name and password, no client certificate
#   separate-files/          the same profile with ca.crt, client.crt… as
#                            separate files (select them all when importing)
#   credentials.txt          the test user name and password
#
# Everything is for testing only: delete the machine afterwards.
set -eu

[ "$(id -u)" -eq 0 ] || { echo "Run as root (sudo sh $0)" >&2; exit 1; }

SERVER=/etc/openvpn/server
PKI=/etc/openvpn/easy-rsa
OUT=/root/tunnelbird-test
NET=10.99.0.0
USER_NAME=tunnelbird

echo "==> Installing OpenVPN and Easy-RSA"
export DEBIAN_FRONTEND=noninteractive
# SSH passes on the caller's locale (fr_FR…), which the server does not have.
export LC_ALL=C.UTF-8
apt-get update -q
apt-get install -y -q openvpn easy-rsa iptables iproute2 curl

echo "==> Creating the certificate authority and certificates"
if [ ! -d "$PKI/pki" ]; then
  make-cadir "$PKI"
  cd "$PKI"
  ./easyrsa --batch init-pki
  ./easyrsa --batch --req-cn="Tunnelbird test CA" build-ca nopass
  ./easyrsa --batch build-server-full server nopass
  ./easyrsa --batch build-client-full client nopass
fi
cd "$PKI"
install -m 600 pki/ca.crt pki/issued/server.crt pki/private/server.key "$SERVER/"
[ -f "$SERVER/tls-crypt.key" ] || openvpn --genkey secret "$SERVER/tls-crypt.key"

echo "==> Creating the test user"
if [ ! -f "$SERVER/test-user" ]; then
  password=$(head -c 18 /dev/urandom | base64 | tr -d '/+=' | cut -c1-20)
  printf '%s\n%s\n' "$USER_NAME" "$(printf '%s' "$password" | sha256sum | cut -d' ' -f1)" \
    > "$SERVER/test-user"
  # Read by the check script, which runs as the server's unprivileged user.
  chown root:nogroup "$SERVER/test-user"
  chmod 640 "$SERVER/test-user"
  mkdir -p "$OUT"
  printf 'user name: %s\npassword: %s\n' "$USER_NAME" "$password" > "$OUT/credentials.txt"
  chmod 600 "$OUT/credentials.txt"
fi

# Checks the user name and password OpenVPN writes to a temporary file
# (line 1: user name, line 2: password). Runs as the server's unprivileged
# user (nobody): the test-user file holds only a hash of the password.
cat > "$SERVER/check-user.sh" <<'EOF'
#!/bin/sh
expected_user=$(sed -n 1p /etc/openvpn/server/test-user)
expected_hash=$(sed -n 2p /etc/openvpn/server/test-user)
user=$(sed -n 1p "$1")
hash=$(sed -n 2p "$1" | tr -d '\n' | sha256sum | cut -d' ' -f1)
[ "$user" = "$expected_user" ] && [ "$hash" = "$expected_hash" ]
EOF
chmod 755 "$SERVER/check-user.sh"

# Lets VPN clients reach the internet through this machine.
cat > "$SERVER/nat.sh" <<EOF
#!/bin/sh
sysctl -q -w net.ipv4.ip_forward=1
out=\$(ip -4 route show default | awk '{print \$5; exit}')
iptables -t nat -C POSTROUTING -s $NET/24 -o "\$out" -j MASQUERADE 2>/dev/null ||
  iptables -t nat -A POSTROUTING -s $NET/24 -o "\$out" -j MASQUERADE
EOF
chmod 755 "$SERVER/nat.sh"

echo "==> Writing the server configuration"
cat > "$SERVER/server.conf" <<EOF
port 1194
proto udp
dev tun
ca ca.crt
cert server.crt
key server.key
dh none
tls-crypt tls-crypt.key
topology subnet
server $NET 255.255.255.0
push "redirect-gateway def1 bypass-dhcp"
push "dhcp-option DNS 1.1.1.1"
push "dhcp-option DNS 9.9.9.9"
keepalive 10 60
data-ciphers AES-256-GCM:CHACHA20-POLY1305
auth SHA256
# Profiles with and without a client certificate are both accepted; every
# client must give the test user name and password.
verify-client-cert optional
script-security 2
auth-user-pass-verify $SERVER/check-user.sh via-file
up $SERVER/nat.sh
user nobody
group nogroup
persist-key
persist-tun
verb 3
EOF

systemctl enable --now openvpn-server@server
systemctl restart openvpn-server@server
sleep 3
if ! systemctl is-active --quiet openvpn-server@server; then
  echo "The OpenVPN server did not start:" >&2
  journalctl -u openvpn-server@server --no-pager -n 20 >&2 || true
  exit 1
fi

echo "==> Writing the client profiles"
# PUBLIC_ADDRESS overrides the address written in the profiles (tests).
public_ip=${PUBLIC_ADDRESS:-$(curl -fsS https://checkip.amazonaws.com || hostname -I | awk '{print $1}')}
mkdir -p "$OUT/separate-files"
common="client
dev tun
proto udp
remote $public_ip 1194
resolv-retry infinite
nobind
persist-key
persist-tun
remote-cert-tls server
data-ciphers AES-256-GCM:CHACHA20-POLY1305
auth SHA256
auth-user-pass
verb 3"
block() { printf '<%s>\n%s\n</%s>\n' "$1" "$(cat "$2")" "$1"; }
{
  echo "$common"
  block ca pki/ca.crt
  block cert pki/issued/client.crt
  block key pki/private/client.key
  block tls-crypt "$SERVER/tls-crypt.key"
} > "$OUT/cert-and-password.ovpn"
{
  echo "$common"
  block ca pki/ca.crt
  block tls-crypt "$SERVER/tls-crypt.key"
} > "$OUT/password-only.ovpn"
cp pki/ca.crt "$OUT/separate-files/ca.crt"
cp pki/issued/client.crt "$OUT/separate-files/client.crt"
cp pki/private/client.key "$OUT/separate-files/client.key"
cp "$SERVER/tls-crypt.key" "$OUT/separate-files/tls-crypt.key"
{
  echo "$common"
  echo "ca ca.crt"
  echo "cert client.crt"
  echo "key client.key"
  echo "tls-crypt tls-crypt.key"
} > "$OUT/separate-files/files.ovpn"
chmod -R go-rwx "$OUT"

echo
echo "Profiles in $OUT, server address $public_ip (UDP 1194)."
