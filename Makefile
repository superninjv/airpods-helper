PREFIX ?= $(HOME)/.local
DAEMON_BIN = target/release/airpods-daemon
CLI_BIN = target/release/airpods-cli
CONFIG_DIR = $(HOME)/.config/airpods-helper
AGS_WIDGET_DIR = $(HOME)/.config/ags/widget/airpods
DBUS_SERVICES_DIR = $(HOME)/.local/share/dbus-1/services

.PHONY: all build install uninstall clean

all: build

build:
	cargo build --workspace --release

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
	@echo "Installed. Next steps:"
	@echo "  sudo setcap 'cap_net_raw,cap_net_admin+eip' $(PREFIX)/bin/airpods-daemon"
	@echo "  systemctl --user enable --now airpods-daemon.service"

uninstall:
	systemctl --user disable --now airpods-daemon.service 2>/dev/null || true
	rm -f $(PREFIX)/bin/airpods-daemon
	rm -f $(PREFIX)/bin/airpods-cli
	rm -f $(HOME)/.config/systemd/user/airpods-daemon.service
	rm -f $(DBUS_SERVICES_DIR)/org.costa.AirPods.service
	rm -f $(AGS_WIDGET_DIR)
	systemctl --user daemon-reload

clean:
	cargo clean
