PREFIX ?= $(HOME)/.local
SYSTEM_PREFIX ?= /usr/local
DAEMON_BIN = target/release/airpods-daemon
CLI_BIN = target/release/airpods-cli
CONFIG_DIR = $(HOME)/.config/airpods-helper
AGS_WIDGET_DIR = $(HOME)/.config/ags/widget/airpods
DBUS_SERVICES_DIR = $(HOME)/.local/share/dbus-1/services

.PHONY: all build install install-system uninstall uninstall-system doctor clean

all: build

build:
	cargo build --workspace --release

# Per-user install into ~/.local (does not require sudo, except for the final setcap step)
install: build
	install -Dm755 $(DAEMON_BIN) $(PREFIX)/bin/airpods-daemon
	install -Dm755 $(CLI_BIN) $(PREFIX)/bin/airpods-cli
	install -Dm644 daemon/airpods-daemon.service $(HOME)/.config/systemd/user/airpods-daemon.service
	install -Dm644 daemon/org.costa.AirPods.service $(DBUS_SERVICES_DIR)/org.costa.AirPods.service
	install -dm755 $(CONFIG_DIR)/eq/
	cp -n eq-presets/*.toml $(CONFIG_DIR)/eq/ 2>/dev/null || true
	cp -n config.example.toml $(CONFIG_DIR)/config.toml 2>/dev/null || true
	ln -sfn $(CURDIR)/widget $(AGS_WIDGET_DIR)
	systemctl --user daemon-reload
	@echo ""
	@echo "Installed into $(PREFIX). Next:"
	@echo "  sudo setcap 'cap_net_raw,cap_net_admin+eip' $(PREFIX)/bin/airpods-daemon"
	@echo "  systemctl --user enable --now airpods-daemon.service"
	@echo "  airpods-cli doctor"

# System-wide install into /usr/local — sudo runs the binary cp + setcap in one shot
install-system: build
	sudo install -Dm755 $(DAEMON_BIN) $(SYSTEM_PREFIX)/bin/airpods-daemon
	sudo install -Dm755 $(CLI_BIN) $(SYSTEM_PREFIX)/bin/airpods-cli
	sudo setcap 'cap_net_raw,cap_net_admin+eip' $(SYSTEM_PREFIX)/bin/airpods-daemon
	sudo install -Dm644 daemon/airpods-daemon.service /usr/lib/systemd/user/airpods-daemon.service
	sudo sed -i 's|%h/.local/bin/airpods-daemon|$(SYSTEM_PREFIX)/bin/airpods-daemon|' \
		/usr/lib/systemd/user/airpods-daemon.service
	sudo install -Dm644 daemon/org.costa.AirPods.service /usr/share/dbus-1/services/org.costa.AirPods.service
	sudo sed -i 's|%h/.local/bin/airpods-daemon|$(SYSTEM_PREFIX)/bin/airpods-daemon|' \
		/usr/share/dbus-1/services/org.costa.AirPods.service
	sudo install -dm755 /usr/share/airpods-helper/eq-presets
	sudo install -m644 eq-presets/*.toml /usr/share/airpods-helper/eq-presets/
	install -dm755 $(CONFIG_DIR)/eq/
	cp -n eq-presets/*.toml $(CONFIG_DIR)/eq/ 2>/dev/null || true
	cp -n config.example.toml $(CONFIG_DIR)/config.toml 2>/dev/null || true
	systemctl --user daemon-reload
	@echo ""
	@echo "Installed system-wide into $(SYSTEM_PREFIX). Caps are already set."
	@echo "  systemctl --user enable --now airpods-daemon.service"
	@echo "  airpods-cli doctor"

doctor:
	@$(PREFIX)/bin/airpods-cli doctor 2>/dev/null || \
		$(SYSTEM_PREFIX)/bin/airpods-cli doctor 2>/dev/null || \
		airpods-cli doctor

uninstall:
	systemctl --user disable --now airpods-daemon.service 2>/dev/null || true
	rm -f $(PREFIX)/bin/airpods-daemon
	rm -f $(PREFIX)/bin/airpods-cli
	rm -f $(HOME)/.config/systemd/user/airpods-daemon.service
	rm -f $(DBUS_SERVICES_DIR)/org.costa.AirPods.service
	rm -f $(AGS_WIDGET_DIR)
	systemctl --user daemon-reload

uninstall-system:
	systemctl --user disable --now airpods-daemon.service 2>/dev/null || true
	sudo rm -f $(SYSTEM_PREFIX)/bin/airpods-daemon
	sudo rm -f $(SYSTEM_PREFIX)/bin/airpods-cli
	sudo rm -f /usr/lib/systemd/user/airpods-daemon.service
	sudo rm -f /usr/share/dbus-1/services/org.costa.AirPods.service
	sudo rm -rf /usr/share/airpods-helper
	systemctl --user daemon-reload

clean:
	cargo clean
