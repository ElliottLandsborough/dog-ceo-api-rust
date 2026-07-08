SHELL := /bin/bash

CARGO ?= $(HOME)/.cargo/bin/cargo
RUSTUP ?= $(HOME)/.cargo/bin/rustup

.PHONY: help check test build build-release build-linux target-linux run run-prod parity parity-start clean

help:
	@echo "Available targets:"
	@echo "  make check         - cargo check"
	@echo "  make test          - cargo test"
	@echo "  make build         - cargo build"
	@echo "  make build-release - cargo build --release"
	@echo "  make target-linux  - install rust target x86_64-unknown-linux-musl (static)"
	@echo "  make build-linux   - cargo build --release --target x86_64-unknown-linux-musl (static)"
	@echo "  make run           - cargo run"
	@echo "  make run-prod      - run ./run-prod.sh"
	@echo "  make parity        - run parity checks against an already running local server"
	@echo "  make parity-start  - start local server, run parity checks, then stop server"
	@echo "  make clean         - cargo clean"

check:
	$(CARGO) check

test:
	$(CARGO) test

build:
	$(CARGO) build

build-release:
	$(CARGO) build --release

target-linux:
	$(RUSTUP) target add x86_64-unknown-linux-musl

build-linux:
	$(CARGO) build --release --target x86_64-unknown-linux-musl

run:
	$(CARGO) run

run-prod:
	./run-prod.sh

parity:
	./scripts/parity_check.sh

parity-start:
	./scripts/parity_check.sh --start-local

clean:
	$(CARGO) clean
