BIN_NAME := vsn1-cli
BUILD_DIR := target/release
BINDIR ?= /usr/local/bin
RUNTIME_ROOT ?= /usr/share/vsn1-cli/runtimes
DESTDIR ?=
INSTALL ?= install
CP ?= cp
RM ?= rm

.PHONY: build install install-cli install-runtimes uninstall

build:
	cargo build --release

install: build install-cli install-runtimes

install-cli:
	$(INSTALL) -d "$(DESTDIR)$(BINDIR)"
	$(INSTALL) -m 0755 "$(BUILD_DIR)/$(BIN_NAME)" "$(DESTDIR)$(BINDIR)/$(BIN_NAME)"

install-runtimes:
	$(INSTALL) -d "$(DESTDIR)$(RUNTIME_ROOT)"
	for runtime in assets/runtimes/*; do \
		name=$$(basename "$$runtime"); \
		$(RM) -rf "$(DESTDIR)$(RUNTIME_ROOT)/$$name"; \
		$(INSTALL) -d "$(DESTDIR)$(RUNTIME_ROOT)/$$name"; \
		$(CP) -R "$$runtime"/. "$(DESTDIR)$(RUNTIME_ROOT)/$$name"/; \
	done

uninstall:
	$(RM) -f "$(DESTDIR)$(BINDIR)/$(BIN_NAME)"
	for runtime in assets/runtimes/*; do \
		name=$$(basename "$$runtime"); \
		$(RM) -rf "$(DESTDIR)$(RUNTIME_ROOT)/$$name"; \
	done
