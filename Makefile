PREFIX ?= /usr
DESTDIR ?=
CARGO ?= cargo
TARGET = target/release

BINS = vibe-comp vide-desktop powerslop file-exploder task-mangler vide-settings vide-about snip-and-sketchy

.PHONY: all build install uninstall clean

all: build

build:
	$(CARGO) build --release --workspace --features vibe-comp/tty

install:
	install -d $(DESTDIR)$(PREFIX)/bin
	@for b in $(BINS); do \
		echo "Installing $$b -> $(DESTDIR)$(PREFIX)/bin/"; \
		install -m 755 $(TARGET)/$$b $(DESTDIR)$(PREFIX)/bin/; \
	done
	install -m 755 scripts/vide-session $(DESTDIR)$(PREFIX)/bin/vide-session
	install -d $(DESTDIR)$(PREFIX)/share/wayland-sessions
	install -m 644 vide-de.desktop $(DESTDIR)$(PREFIX)/share/wayland-sessions/vide-de.desktop
	install -d $(DESTDIR)$(PREFIX)/share/vide-de/wallpapers
	install -m 644 assets/wallpapers/*.png $(DESTDIR)$(PREFIX)/share/vide-de/wallpapers/
	install -d $(DESTDIR)$(PREFIX)/share/icons/hicolor/512x512/apps
	install -m 644 assets/logo-512.png $(DESTDIR)$(PREFIX)/share/icons/hicolor/512x512/apps/vide-de.png

uninstall:
	@for b in $(BINS); do \
		rm -f $(DESTDIR)$(PREFIX)/bin/$$b; \
	done
	rm -f $(DESTDIR)$(PREFIX)/bin/vide-session
	rm -f $(DESTDIR)$(PREFIX)/share/wayland-sessions/vide-de.desktop
	rm -rf $(DESTDIR)$(PREFIX)/share/vide-de

clean:
	$(CARGO) clean
