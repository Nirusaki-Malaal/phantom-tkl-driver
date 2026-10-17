PREFIX ?= $(HOME)/.local

.PHONY: build run check install
build:
	cargo build --release --locked
run:
	cargo run --locked
check:
	cargo fmt --all -- --check
	cargo test --workspace --locked
	cargo clippy --workspace --locked --all-targets -- -D warnings
	python3 -m unittest discover -s tests -p 'test_*.py'
install:
	./install.sh --prefix "$(PREFIX)"
