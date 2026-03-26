PREFIX ?= $(HOME)/.local
DAEMON_BIN = daemon/target/release/airpods-daemon
CONFIG_DIR = $(HOME)/.config/airpods-helper
AGS_WIDGET_DIR = $(HOME)/.config/ags/widget/airpods

.PHONY: all daemon install uninstall clean

all: daemon

daemon:
	cd daemon && cargo build --release

install: daemon
	install -Dm755 $(DAEMON_BIN) $(PREFIX)/bin/airpods-daemon
	install -Dm644 daemon/airpods-daemon.service $(HOME)/.config/systemd/user/airpods-daemon.service
	install -dm755 $(CONFIG_DIR)/eq/
	cp -n eq-presets/*.toml $(CONFIG_DIR)/eq/ 2>/dev/null || true
	cp -n config.example.toml $(CONFIG_DIR)/config.toml 2>/dev/null || true
	ln -sfn $(CURDIR)/widget $(AGS_WIDGET_DIR)
	systemctl --user daemon-reload
	@echo ""
	@echo "Installed. Run: systemctl --user enable --now airpods-daemon.service"
	@echo "Add AirPodsBattery to your AGS Bar.tsx"

uninstall:
	systemctl --user disable --now airpods-daemon.service 2>/dev/null || true
	rm -f $(PREFIX)/bin/airpods-daemon
	rm -f $(HOME)/.config/systemd/user/airpods-daemon.service
	rm -f $(AGS_WIDGET_DIR)
	systemctl --user daemon-reload

clean:
	cd daemon && cargo clean
