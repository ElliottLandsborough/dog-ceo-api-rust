SHELL := /bin/bash

CARGO ?= $(HOME)/.cargo/bin/cargo
RUSTUP ?= $(HOME)/.cargo/bin/rustup
TARGET_CPU ?= haswell
HOST_CC ?= /usr/bin/cc
HOST_CXX ?= /usr/bin/c++

IMAGE_NAME ?= dog-ceo-api-rust:runtime
IMAGE_TAR ?= dog_ceo_api_rust_runtime.tar
REMOTE_HOST ?= troleumdeploy
REMOTE_PLATFORM ?= linux/amd64
REMOTE_BASE_DIR ?= /home/deploy/dog-ceo-api-rust
APP_BASENAME ?= dog_ceo_api_rust
HOST_PORTS ?= 10081 10082 10083 10084
CONTAINER_PORT ?= 3000
TEMPIMAGES_DIR ?= tempimages
DOG_IMAGES_REPO ?= https://github.com/jigsawpieces/dog-api-images.git

.PHONY: help check test build build-release build-linux target-linux run run-prod parity parity-start clean \
	prepare-images cleanup-images build-runtime-image save-image send-image run-remote deploy-to-production remote-logs

help:
	@echo "Available targets:"
	@echo "  make check         - cargo check"
	@echo "  make test          - cargo test"
	@echo "  make build         - cargo build"
	@echo "  make build-release - cargo build --release"
	@echo "  make target-linux  - install rust target x86_64-unknown-linux-musl (static)"
	@echo "  make build-linux   - cargo build --release --target x86_64-unknown-linux-musl (static, linker=rust-lld, cpu=$(TARGET_CPU))"
	@echo "  make run           - cargo run"
	@echo "  make run-prod      - run ./run-prod.sh"
	@echo "  make parity        - run parity checks against an already running local server"
	@echo "  make parity-start  - start local server, run parity checks, then stop server"
	@echo "  make clean         - cargo clean"
	@echo "  make build-runtime-image   - build runtime image (linux/amd64, target=runtime)"
	@echo "  make save-image            - save runtime image to local tar file"
	@echo "  make send-image            - upload image tar to remote host"
	@echo "  make run-remote            - run one container per HOST_PORTS on remote host"
	@echo "  make deploy-to-production  - build, ship, and run remote containers"

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
	CC="$(HOST_CC)" CXX="$(HOST_CXX)" \
	CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER="$(HOST_CC)" \
	CARGO_TARGET_X86_64_APPLE_DARWIN_LINKER="$(HOST_CC)" \
	CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld \
	CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_RUSTFLAGS="-C target-cpu=$(TARGET_CPU)" \
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

prepare-images:
	rm -rf $(TEMPIMAGES_DIR)
	git clone --depth 1 --single-branch $(DOG_IMAGES_REPO) $(TEMPIMAGES_DIR)
	rm -rf $(TEMPIMAGES_DIR)/.git $(TEMPIMAGES_DIR)/.gitignore $(TEMPIMAGES_DIR)/README.md $(TEMPIMAGES_DIR)/LICENSE

cleanup-images:
	rm -rf $(TEMPIMAGES_DIR)

build-runtime-image: prepare-images
	docker buildx build --platform $(REMOTE_PLATFORM) --target runtime --load -t $(IMAGE_NAME) .

save-image: build-runtime-image
	docker save $(IMAGE_NAME) -o $(IMAGE_TAR)
	$(MAKE) cleanup-images

send-image:
	ssh $(REMOTE_HOST) "mkdir -p $(REMOTE_BASE_DIR) && chmod 700 $(REMOTE_BASE_DIR)"
	scp $(IMAGE_TAR) $(REMOTE_HOST):$(REMOTE_BASE_DIR)/$(IMAGE_TAR)

delete-local-image-tar:
	rm -f $(IMAGE_TAR)

run-remote:
	ssh $(REMOTE_HOST) "docker load -i $(REMOTE_BASE_DIR)/$(IMAGE_TAR)"
	ssh $(REMOTE_HOST) 'set -euo pipefail; i=1; for port in $(HOST_PORTS); do name=$(APP_BASENAME)_$$i; docker rm -f $$name >/dev/null 2>&1 || true; docker run -d --restart unless-stopped --platform $(REMOTE_PLATFORM) -p $$port:$(CONTAINER_PORT) --name $$name $(IMAGE_NAME); i=$$((i+1)); done'
	ssh $(REMOTE_HOST) "rm -f $(REMOTE_BASE_DIR)/$(IMAGE_TAR)"

deploy-to-production: test save-image send-image run-remote delete-local-image-tar

remote-logs:
	ssh $(REMOTE_HOST) "docker logs -f $(APP_BASENAME)_1"