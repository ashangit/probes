RUST_EDITION ?= 2021

.PHONY: all
all: build

.PHONY: install-deps
install-deps:
	rustup component add rustfmt
	rustup component add clippy

.PHONY: deps
deps:
	cargo update

.PHONY: fmt
fmt: install-deps
	cargo fmt --all

.PHONY: lint
lint: install-deps
	cargo clippy --fix

.PHONY: build
build: test
	 cargo build --release

.PHONY: test
test: install-deps
	cargo fmt --all -- --check
	# TODO: cargo clippy -- -D warnings
	cargo clippy
	cargo test

.PHONY: run
run: test
	cargo run

.PHONY: clean
clean:
	cargo clean