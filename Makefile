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
	 RUSTFLAGS="--cfg tokio_unstable" cargo build --release

.PHONY: test
test: install-deps
	cargo fmt --all -- --check
	cargo clippy -- -D warnings
	cargo test

.PHONY: run
run: test
	RUST_LOG=debug RUSTFLAGS="--cfg tokio_unstable" cargo run --package probes --bin mempoke -- --services-tag memcached

.PHONY: clean
clean:
	cargo clean