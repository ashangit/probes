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
	cargo clippy --fix --allow-dirty --allow-staged

.PHONY: build
build:
	 RUSTFLAGS="--cfg tokio_unstable" cargo build --release

.PHONY: docker-build
docker-build:
	docker build . --network=host -t probes:latest -f Dockerfile --rm --pull

.PHONY: test
test: install-deps
	cargo fmt --all -- --check
	cargo clippy -- -D warnings
	cargo test

.PHONY: doc
doc: test
	cargo doc --open --document-private-items

.PHONY: run
run: test
	RUST_LOG=debug RUSTFLAGS="--cfg tokio_unstable" cargo run --package probes --bin mempoke -- --services-tag memcached

.PHONY: clean
clean:
	cargo clean