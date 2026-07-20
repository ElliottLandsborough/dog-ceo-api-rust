SHELL := /bin/bash

CARGO ?= $(HOME)/.cargo/bin/cargo
RUSTUP ?= $(HOME)/.cargo/bin/rustup
TARGET_CPU ?= haswell
HOST_CC ?= /usr/bin/cc
HOST_CXX ?= /usr/bin/c++

RUNTIME_IMAGE_NAME ?= dog-ceo-api-rust:runtime
RUNTIME_IMAGE_TAR ?= dog_ceo_api_rust_runtime.tar
IMAGES_IMAGE_NAME ?= dog-ceo-api-rust:images
IMAGES_IMAGE_TAR ?= dog_ceo_api_rust_images.tar
REMOTE_HOST ?= troleumdeploy
REMOTE_PLATFORM ?= linux/amd64
REMOTE_BASE_DIR ?= /home/deploy/dog-ceo-api-rust
APP_BASENAME ?= dog_ceo_api_rust
IMAGES_CONTAINER_NAME ?= dog_ceo_api_images
HOST_PORTS ?= 10081 10082 10083 10084
CONTAINER_PORT ?= 3000
IMAGES_HOST_PORT ?= 10080
IMAGES_CONTAINER_PORT ?= 8080
TEMPIMAGES_DIR ?= tempimages
DOG_IMAGES_REPO ?= https://github.com/jigsawpieces/dog-api-images.git

.PHONY: help check test build build-release build-linux target-linux run run-prod parity parity-start clean \
	prepare-images cleanup-images build-runtime-image build-images-image save-images save-runtime save-image \
	send-image run-remote run-remote-images deploy-to-production delete-local-image-tar remote-logs remote-logs-images

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
	@echo "  make build-runtime-image   - build runtime API image (linux/amd64, target=runtime)"
	@echo "  make build-images-image    - build static images image (linux/amd64, target=images)"
	@echo "  make save-image            - save both runtime and images images to tar files"
	@echo "  make send-image            - upload both image tar files to remote host"
	@echo "  make run-remote            - run API containers (one per HOST_PORTS) on remote host"
	@echo "  make run-remote-images     - run static images container on remote host"
	@echo "  make deploy-to-production  - test, build, ship, and run both runtime and images containers"

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
	docker buildx build --platform $(REMOTE_PLATFORM) --target runtime --load -t $(RUNTIME_IMAGE_NAME) .

build-images-image: prepare-images
	docker buildx build --platform $(REMOTE_PLATFORM) --target images --load -t $(IMAGES_IMAGE_NAME) .

save-runtime: build-runtime-image
	docker save $(RUNTIME_IMAGE_NAME) -o $(RUNTIME_IMAGE_TAR)

save-images: build-images-image
	docker save $(IMAGES_IMAGE_NAME) -o $(IMAGES_IMAGE_TAR)

save-image: save-runtime save-images
	$(MAKE) cleanup-images

send-image:
	ssh $(REMOTE_HOST) "mkdir -p $(REMOTE_BASE_DIR) && chmod 700 $(REMOTE_BASE_DIR)"
	scp $(RUNTIME_IMAGE_TAR) $(REMOTE_HOST):$(REMOTE_BASE_DIR)/$(RUNTIME_IMAGE_TAR)
	scp $(IMAGES_IMAGE_TAR) $(REMOTE_HOST):$(REMOTE_BASE_DIR)/$(IMAGES_IMAGE_TAR)

delete-local-tars:
	rm -f $(RUNTIME_IMAGE_TAR) $(IMAGES_IMAGE_TAR)

run-remote:
	ssh $(REMOTE_HOST) "docker load -i $(REMOTE_BASE_DIR)/$(RUNTIME_IMAGE_TAR)"
	ssh $(REMOTE_HOST) 'set -euo pipefail; i=1; for port in $(HOST_PORTS); do name=$(APP_BASENAME)_$$i; docker rm -f $$name >/dev/null 2>&1 || true; docker run -d --restart unless-stopped --platform $(REMOTE_PLATFORM) -p $$port:$(CONTAINER_PORT) --name $$name $(RUNTIME_IMAGE_NAME); i=$$((i+1)); done'
	ssh $(REMOTE_HOST) "rm -f $(REMOTE_BASE_DIR)/$(RUNTIME_IMAGE_TAR)"

run-remote-images:
	ssh $(REMOTE_HOST) "docker load -i $(REMOTE_BASE_DIR)/$(IMAGES_IMAGE_TAR)"
	ssh $(REMOTE_HOST) "docker rm -f $(IMAGES_CONTAINER_NAME) >/dev/null 2>&1 || true"
	ssh $(REMOTE_HOST) "docker run -d --restart unless-stopped --platform $(REMOTE_PLATFORM) -p $(IMAGES_HOST_PORT):$(IMAGES_CONTAINER_PORT) --name $(IMAGES_CONTAINER_NAME) $(IMAGES_IMAGE_NAME)"
	ssh $(REMOTE_HOST) "rm -f $(REMOTE_BASE_DIR)/$(IMAGES_IMAGE_TAR)"

deploy-to-production: test save-image send-image run-remote run-remote-images delete-local-tars

remote-logs:
	ssh $(REMOTE_HOST) "docker logs -f $(APP_BASENAME)_1"

remote-logs-images:
	ssh $(REMOTE_HOST) "docker logs -f $(IMAGES_CONTAINER_NAME)"