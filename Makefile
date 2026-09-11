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
install:
	./install.sh --prefix "$(PREFIX)"
