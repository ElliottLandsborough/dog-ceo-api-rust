SHELL := /bin/bash

CARGO ?= $(HOME)/.cargo/bin/cargo

.PHONY: help check test build build-release build-x86_64 target-x86_64 run run-prod parity parity-start clean

help:
	@echo "Available targets:"
	@echo "  make check         - cargo check"
	@echo "  make test          - cargo test"
	@echo "  make build         - cargo build"
	@echo "  make build-release - cargo build --release"
	@echo "  make target-x86_64 - install rust target x86_64-apple-darwin"
	@echo "  make build-x86_64  - cargo build --release --target x86_64-apple-darwin"
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

target-x86_64:
	$(CARGO) target add x86_64-apple-darwin

build-x86_64:
	$(CARGO) build --release --target x86_64-apple-darwin

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
