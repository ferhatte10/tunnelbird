# Build and install Tunnelbird from source.
#   make                 release build
#   sudo make install    install under $(PREFIX) (default /usr/local)
#   sudo make uninstall  remove what `make install` installed
#   make dev-install     desktop entry + icons in ~/.local, pointing to the debug
#                        build: gives `cargo run` its dock icon (no sudo)
#   make dev-uninstall   remove them
#   make deb             .deb package in target/packages (Debian, Ubuntu…)
#   make rpm             .rpm package in target/packages (Fedora…; needs rpmbuild)
#   make screenshots     store screenshots in data/screenshots (demo mode,
#                        invisible GNOME Shell; see build-aux/screenshots.sh)

PREFIX    ?= /usr/local
DESTDIR   ?=
APP_ID    := fr.ferhatsn.Tunnelbird
LOCALEDIR := $(PREFIX)/share/locale
LINGUAS   := $(shell grep -v '^\#' po/LINGUAS)
VERSION   := $(shell sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)
# rustup installs cargo in ~/.cargo/bin, which is not always in PATH.
CARGO     ?= $(shell command -v cargo 2>/dev/null || echo $(HOME)/.cargo/bin/cargo)

.PHONY: all run check install install-bin install-data uninstall dev-install dev-uninstall deb rpm screenshots clean

all:
	LOCALEDIR=$(LOCALEDIR) $(CARGO) build --release --locked

run:
	$(CARGO) run

check:
	$(CARGO) fmt --all --check
	$(CARGO) clippy --workspace --all-targets -- -D warnings
	$(CARGO) test --workspace
	desktop-file-validate data/$(APP_ID).desktop
	appstreamcli validate --no-net data/$(APP_ID).metainfo.xml
	@# The packages and the store listing must announce this version.
	grep -q '^Version: *$(VERSION)$$' build-aux/packaging/tunnelbird.spec
	grep -q 'release version="$(VERSION)"' data/$(APP_ID).metainfo.xml

install: install-bin install-data

install-bin:
	install -Dm755 target/release/tunnelbird $(DESTDIR)$(PREFIX)/bin/tunnelbird

# Everything but the binary: used alone by packagers that build Rust themselves (Snap).
install-data:
	install -Dm644 data/$(APP_ID).desktop $(DESTDIR)$(PREFIX)/share/applications/$(APP_ID).desktop
	install -Dm644 data/$(APP_ID).metainfo.xml $(DESTDIR)$(PREFIX)/share/metainfo/$(APP_ID).metainfo.xml
	install -Dm644 data/icons/hicolor/scalable/apps/$(APP_ID).svg $(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps/$(APP_ID).svg
	install -Dm644 data/icons/hicolor/symbolic/apps/$(APP_ID)-symbolic.svg $(DESTDIR)$(PREFIX)/share/icons/hicolor/symbolic/apps/$(APP_ID)-symbolic.svg
	install -Dm644 data/icons/hicolor/scalable/status/$(APP_ID)-tray-connected.svg $(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/status/$(APP_ID)-tray-connected.svg
	install -Dm644 data/icons/hicolor/scalable/status/$(APP_ID)-tray-disconnected.svg $(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/status/$(APP_ID)-tray-disconnected.svg
	for lang in $(LINGUAS); do \
		mkdir -p $(DESTDIR)$(LOCALEDIR)/$$lang/LC_MESSAGES; \
		msgfmt -o $(DESTDIR)$(LOCALEDIR)/$$lang/LC_MESSAGES/tunnelbird.mo po/$$lang.po; \
		chmod 644 $(DESTDIR)$(LOCALEDIR)/$$lang/LC_MESSAGES/tunnelbird.mo; \
	done

uninstall:
	rm -f $(DESTDIR)$(PREFIX)/bin/tunnelbird
	rm -f $(DESTDIR)$(PREFIX)/share/applications/$(APP_ID).desktop
	rm -f $(DESTDIR)$(PREFIX)/share/metainfo/$(APP_ID).metainfo.xml
	rm -f $(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps/$(APP_ID).svg
	rm -f $(DESTDIR)$(PREFIX)/share/icons/hicolor/symbolic/apps/$(APP_ID)-symbolic.svg
	rm -f $(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/status/$(APP_ID)-tray-connected.svg
	rm -f $(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/status/$(APP_ID)-tray-disconnected.svg
	for lang in $(LINGUAS); do rm -f $(DESTDIR)$(LOCALEDIR)/$$lang/LC_MESSAGES/tunnelbird.mo; done

USER_DATA := $(HOME)/.local/share

dev-install:
	$(CARGO) build
	install -Dm644 data/icons/hicolor/scalable/apps/$(APP_ID).svg $(USER_DATA)/icons/hicolor/scalable/apps/$(APP_ID).svg
	install -Dm644 data/icons/hicolor/symbolic/apps/$(APP_ID)-symbolic.svg $(USER_DATA)/icons/hicolor/symbolic/apps/$(APP_ID)-symbolic.svg
	install -Dm644 data/icons/hicolor/scalable/status/$(APP_ID)-tray-connected.svg $(USER_DATA)/icons/hicolor/scalable/status/$(APP_ID)-tray-connected.svg
	install -Dm644 data/icons/hicolor/scalable/status/$(APP_ID)-tray-disconnected.svg $(USER_DATA)/icons/hicolor/scalable/status/$(APP_ID)-tray-disconnected.svg
	mkdir -p $(USER_DATA)/applications
	sed 's|^Exec=tunnelbird|Exec=$(CURDIR)/target/debug/tunnelbird|' data/$(APP_ID).desktop > $(USER_DATA)/applications/$(APP_ID).desktop

dev-uninstall:
	rm -f $(USER_DATA)/applications/$(APP_ID).desktop
	rm -f $(USER_DATA)/icons/hicolor/scalable/apps/$(APP_ID).svg
	rm -f $(USER_DATA)/icons/hicolor/symbolic/apps/$(APP_ID)-symbolic.svg
	rm -f $(USER_DATA)/icons/hicolor/scalable/status/$(APP_ID)-tray-connected.svg
	rm -f $(USER_DATA)/icons/hicolor/scalable/status/$(APP_ID)-tray-disconnected.svg

deb:
	build-aux/packaging/deb.sh

rpm:
	rpmbuild -bb --build-in-place --define "_rpmdir $(CURDIR)/target/packages" build-aux/packaging/tunnelbird.spec

screenshots:
	$(CARGO) build
	build-aux/screenshots.sh

clean:
	$(CARGO) clean
