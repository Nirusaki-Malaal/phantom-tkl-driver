PREFIX ?= $(HOME)/.local

.PHONY: build run check install
build:
	cargo build --release --locked
run:
	cargo run --locked
check:
	cargo fmt --check
	cargo test --locked
	cargo clippy --locked --all-targets -- -D warnings
install: build
	install -Dm755 target/release/phantom-studio "$(DESTDIR)$(PREFIX)/bin/phantom-studio"
	install -Dm644 packaging/io.github.phantom.Studio.desktop "$(DESTDIR)$(PREFIX)/share/applications/io.github.phantom.Studio.desktop"
	install -Dm644 packaging/io.github.phantom.Studio.svg "$(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps/io.github.phantom.Studio.svg"
