PREFIX ?= $(HOME)/.local

.PHONY: build install uninstall clean

build:
	@command -v cargo >/dev/null 2>&1 || { \
		echo "Installing Rust..."; \
		curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y; \
		. "$$HOME/.cargo/env"; \
	}
	cargo build --release

install: build
	install -d $(PREFIX)/bin
	install -m 755 target/release/c4 $(PREFIX)/bin/c4
	@echo "Installed c4 to $(PREFIX)/bin/c4"
	@echo "Run 'c4' to start."

uninstall:
	rm -f $(PREFIX)/bin/c4
	rm -rf $(HOME)/.config/c4
	@echo "Uninstalled c4."

clean:
	cargo clean
