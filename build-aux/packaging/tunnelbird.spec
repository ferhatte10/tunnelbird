# RPM package (Fedora, RHEL-like, openSUSE), built from the source folder:
#
#   rpmbuild -bb --build-in-place --define "_rpmdir $PWD/target/packages" \
#       build-aux/packaging/tunnelbird.spec
#
# or `make rpm`. The CI builds it inside Fedora (docs/plans/ci-and-release.md).

# The release build keeps no debug information to split out.
%global debug_package %{nil}

Name:           tunnelbird
Version:        0.1.0
Release:        1%{?dist}
Summary:        Connect to your VPN in one click
License:        GPL-3.0-or-later
URL:            https://tunnelbird.ferhatsn.fr

BuildRequires:  cargo
BuildRequires:  rust
BuildRequires:  make
BuildRequires:  gcc
BuildRequires:  gettext
BuildRequires:  pkgconfig(gtk4) >= 4.14
BuildRequires:  pkgconfig(libadwaita-1) >= 1.5
BuildRequires:  pkgconfig(libsoup-3.0)
BuildRequires:  desktop-file-utils
BuildRequires:  libappstream-glib

Requires:       gtk4 >= 4.14
Requires:       libadwaita >= 1.5
Requires:       libsoup3
Requires:       NetworkManager >= 1.16
Recommends:     NetworkManager-openvpn-gnome
Recommends:     iso-codes

%description
Tunnelbird imports the .conf (WireGuard) or .ovpn (OpenVPN) file from your VPN
provider or your own server and connects in one click, through NetworkManager,
without administrator rights. Live traffic graph, public address and location,
automatic connection on untrusted Wi-Fi, top bar icon, sharing as a file or a
QR code.

%build
make all PREFIX=%{_prefix}

%install
make install PREFIX=%{_prefix} DESTDIR=%{buildroot}

%check
desktop-file-validate %{buildroot}%{_datadir}/applications/fr.ferhatsn.Tunnelbird.desktop
appstream-util validate-relax --nonet %{buildroot}%{_metainfodir}/fr.ferhatsn.Tunnelbird.metainfo.xml

%files
%license LICENSE
%doc README.md
%{_bindir}/tunnelbird
%{_datadir}/applications/fr.ferhatsn.Tunnelbird.desktop
%{_metainfodir}/fr.ferhatsn.Tunnelbird.metainfo.xml
%{_datadir}/icons/hicolor/scalable/apps/fr.ferhatsn.Tunnelbird.svg
%{_datadir}/icons/hicolor/symbolic/apps/fr.ferhatsn.Tunnelbird-symbolic.svg
%{_datadir}/icons/hicolor/scalable/status/fr.ferhatsn.Tunnelbird-tray-*.svg
%{_datadir}/locale/*/LC_MESSAGES/tunnelbird.mo

%changelog
* Thu Oct 29 2026 Ferhat Saidoun <ferhatsnpro@gmail.com> - 0.1.0-1
- First release
