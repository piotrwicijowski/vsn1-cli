BIN_NAME := vsn1-cli
DAEMON_BIN_NAME := vsn1-daemon
BUILD_DIR := target/release
OS_NAME := $(shell uname -s)
BINDIR ?= /usr/local/bin
RUNTIME_ROOT ?= /usr/share/vsn1-cli/runtimes
SERVICE_ASSET_DIR := assets/services
SYSTEMD_USER_UNITDIR ?= /usr/lib/systemd/user
LAUNCHD_AGENT_DIR ?= /Library/LaunchAgents
DESTDIR ?=
INSTALL ?= install
CP ?= cp
RM ?= rm

.PHONY: build install install-common install-linux install-macos install-cli install-daemon install-runtimes install-systemd-user-service install-launchd-agent uninstall uninstall-common uninstall-linux uninstall-macos uninstall-systemd-user-service uninstall-launchd-agent

build:
	cargo build --release

install:
ifeq ($(OS_NAME),Linux)
	$(MAKE) install-linux
else ifeq ($(OS_NAME),Darwin)
	$(MAKE) install-macos
else
	@printf 'unsupported OS for `make install`: %s\n' "$(OS_NAME)"; exit 1
endif

install-common: build install-cli install-daemon install-runtimes

install-linux: install-common install-systemd-user-service

install-macos: install-common install-launchd-agent

install-cli:
	$(INSTALL) -d "$(DESTDIR)$(BINDIR)"
	$(INSTALL) -m 0755 "$(BUILD_DIR)/$(BIN_NAME)" "$(DESTDIR)$(BINDIR)/$(BIN_NAME)"

install-daemon:
	$(INSTALL) -d "$(DESTDIR)$(BINDIR)"
	$(INSTALL) -m 0755 "$(BUILD_DIR)/$(DAEMON_BIN_NAME)" "$(DESTDIR)$(BINDIR)/$(DAEMON_BIN_NAME)"

install-runtimes:
	$(INSTALL) -d "$(DESTDIR)$(RUNTIME_ROOT)"
	for runtime in assets/runtimes/*; do \
		name=$$(basename "$$runtime"); \
		$(RM) -rf "$(DESTDIR)$(RUNTIME_ROOT)/$$name"; \
		$(INSTALL) -d "$(DESTDIR)$(RUNTIME_ROOT)/$$name"; \
		$(CP) -R "$$runtime"/. "$(DESTDIR)$(RUNTIME_ROOT)/$$name"/; \
	done

install-systemd-user-service:
	$(INSTALL) -d "$(DESTDIR)$(SYSTEMD_USER_UNITDIR)"
	$(INSTALL) -m 0644 "$(SERVICE_ASSET_DIR)/vsn1-daemon.service" "$(DESTDIR)$(SYSTEMD_USER_UNITDIR)/vsn1-daemon.service"

install-launchd-agent:
	$(INSTALL) -d "$(DESTDIR)$(LAUNCHD_AGENT_DIR)"
	$(INSTALL) -m 0644 "$(SERVICE_ASSET_DIR)/com.vsn1.vsn1-daemon.plist" "$(DESTDIR)$(LAUNCHD_AGENT_DIR)/com.vsn1.vsn1-daemon.plist"

uninstall-systemd-user-service:
	$(RM) -f "$(DESTDIR)$(SYSTEMD_USER_UNITDIR)/vsn1-daemon.service"

uninstall-launchd-agent:
	$(RM) -f "$(DESTDIR)$(LAUNCHD_AGENT_DIR)/com.vsn1.vsn1-daemon.plist"

uninstall:

ifeq ($(OS_NAME),Linux)
	$(MAKE) uninstall-linux
else ifeq ($(OS_NAME),Darwin)
	$(MAKE) uninstall-macos
else
	@printf 'unsupported OS for `make uninstall`: %s\n' "$(OS_NAME)"; exit 1
endif

uninstall-common:
	$(RM) -f "$(DESTDIR)$(BINDIR)/$(BIN_NAME)"
	$(RM) -f "$(DESTDIR)$(BINDIR)/$(DAEMON_BIN_NAME)"
	for runtime in assets/runtimes/*; do \
		name=$$(basename "$$runtime"); \
		$(RM) -rf "$(DESTDIR)$(RUNTIME_ROOT)/$$name"; \
	done

uninstall-linux: uninstall-common uninstall-systemd-user-service

uninstall-macos: uninstall-common uninstall-launchd-agent
