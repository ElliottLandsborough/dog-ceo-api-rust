SHELL := /bin/bash

CARGO ?= $(HOME)/.cargo/bin/cargo
TARGET_CPU ?= haswell

RUNTIME_IMAGE_NAME ?= dog-ceo-api-rust:runtime
RUNTIME_IMAGE_TAR ?= dog_ceo_api_rust_runtime.tar
IMAGES_IMAGE_NAME ?= dog-ceo-static:images
IMAGES_IMAGE_TAR ?= dog_ceo_static_images.tar
REMOTE_HOST ?= golf2deploy
REMOTE_SSH_USER ?= deploy
REMOTE_CONN ?= $(REMOTE_SSH_USER)@$(REMOTE_HOST)
REMOTE_PLATFORM ?= linux/amd64
REMOTE_ENGINE ?= podman
REMOTE_BASE_DIR ?= /home/$(REMOTE_SSH_USER)/dog-ceo-api-rust
APP_BASENAME ?= dog_ceo_api_rust
IMAGES_CONTAINER_NAME ?= dog_ceo_api_images
HOST_PORTS ?= 10081
CONTAINER_PORT ?= 3000
IMAGES_HOST_PORT ?= 10080
IMAGES_CONTAINER_PORT ?= 8080
REMOTE_TMPFS ?= /tmp:rw,noexec,nosuid,nodev,size=64m
REMOTE_EXTRA_RUN_ARGS ?=
TEMPIMAGES_DIR ?= tempimages
IMAGES_REPO ?= https://github.com/jigsawpieces/dog-api-images.git

.PHONY: help check test build build-release run run-prod parity parity-start clean \
	fetch-images refresh-images require-images cleanup-images build-runtime-image build-static-image save-static-image save-runtime save-image \
	send-image run-remote run-remote-static run-remote-images deploy-to-production delete-local-tars remote-logs remote-logs-static remote-logs-images \
	deploy-to-host

help:
	@echo "Available targets:"
	@echo "  make check         - cargo check"
	@echo "  make test          - cargo test"
	@echo "  make build         - cargo build"
	@echo "  make build-release - cargo build --release"
	@echo "  make run           - cargo run"
	@echo "  make run-prod      - run ./run-prod.sh"
	@echo "  make parity        - run parity checks against an already running local server"
	@echo "  make parity-start  - start local server, run parity checks, then stop server"
	@echo "  make clean         - cargo clean"
	@echo "  make fetch-images        - one-time clone of dog-api-images into $(TEMPIMAGES_DIR)"
	@echo "  make refresh-images      - update existing $(TEMPIMAGES_DIR) with latest upstream"
	@echo "  make cleanup-images      - remove local $(TEMPIMAGES_DIR) clone"
	@echo "  make build-runtime-image   - build runtime API image (linux/amd64, target=runtime)"
	@echo "  make build-static-image    - build static files image (linux/amd64, target=images)"
	@echo "  make save-image            - save both runtime and images images to tar files"
	@echo "  make send-image            - upload both image tar files to remote host"
	@echo "  make run-remote            - run API containers bound to localhost for nginx proxy"
	@echo "  make run-remote-static     - run static files container bound to localhost:$(IMAGES_HOST_PORT)"
	@echo "  make run-remote-images     - alias for run-remote-static"
	@echo "  make deploy-to-production  - test, build, ship, and run both runtime and images containers"
	@echo "  make deploy-to-host HOST=x - ship + run existing tars on a single host (ssh alias)"
	@echo "  make remote-logs           - tail API logs from the remote host"
	@echo "  make remote-logs-static    - tail static files logs from the remote host"
	@echo "  make remote-logs-images    - alias for remote-logs-static"
	@echo ""
	@echo "Remote deploy vars:"
	@echo "  REMOTE_SSH_USER=$(REMOTE_SSH_USER)"
	@echo "  REMOTE_ENGINE=$(REMOTE_ENGINE)"
	@echo "  REMOTE_TMPFS=$(REMOTE_TMPFS)"
	@echo "  REMOTE_EXTRA_RUN_ARGS=$(REMOTE_EXTRA_RUN_ARGS)"

check:
	$(CARGO) check

test:
	$(CARGO) test

build:
	$(CARGO) build

build-release:
	$(CARGO) build --release

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

fetch-images:
	@if [[ -d "$(TEMPIMAGES_DIR)" ]]; then \
		echo "$(TEMPIMAGES_DIR) already exists; skipping clone"; \
	else \
		git clone --depth 1 --single-branch $(IMAGES_REPO) $(TEMPIMAGES_DIR); \
		rm -rf $(TEMPIMAGES_DIR)/.git $(TEMPIMAGES_DIR)/.gitignore $(TEMPIMAGES_DIR)/README.md $(TEMPIMAGES_DIR)/LICENSE; \
	fi

refresh-images:
	@if [[ ! -d "$(TEMPIMAGES_DIR)" ]]; then \
		echo "$(TEMPIMAGES_DIR) not found; run 'make fetch-images' first"; \
		exit 1; \
	fi
	rm -rf $(TEMPIMAGES_DIR)
	git clone --depth 1 --single-branch $(IMAGES_REPO) $(TEMPIMAGES_DIR)
	rm -rf $(TEMPIMAGES_DIR)/.git $(TEMPIMAGES_DIR)/.gitignore $(TEMPIMAGES_DIR)/README.md $(TEMPIMAGES_DIR)/LICENSE

require-images:
	@test -d $(TEMPIMAGES_DIR) || { echo "missing $(TEMPIMAGES_DIR); run 'make fetch-images' first"; exit 1; }

cleanup-images:
	rm -rf $(TEMPIMAGES_DIR)

build-runtime-image: require-images
	docker build --platform $(REMOTE_PLATFORM) --build-arg RUST_TARGET_CPU="$(TARGET_CPU)" --target runtime -t $(RUNTIME_IMAGE_NAME) .

build-static-image: require-images
	docker build --platform $(REMOTE_PLATFORM) --target images -t $(IMAGES_IMAGE_NAME) .

save-runtime: build-runtime-image
	docker save $(RUNTIME_IMAGE_NAME) -o $(RUNTIME_IMAGE_TAR)

save-static-image: build-static-image
	docker save $(IMAGES_IMAGE_NAME) -o $(IMAGES_IMAGE_TAR)

save-image: save-runtime save-static-image

send-image:
	ssh $(REMOTE_CONN) "mkdir -p $(REMOTE_BASE_DIR) && chmod 700 $(REMOTE_BASE_DIR)"
	scp $(RUNTIME_IMAGE_TAR) $(REMOTE_CONN):$(REMOTE_BASE_DIR)/$(RUNTIME_IMAGE_TAR)
	scp $(IMAGES_IMAGE_TAR) $(REMOTE_CONN):$(REMOTE_BASE_DIR)/$(IMAGES_IMAGE_TAR)

delete-local-tars:
	rm -f $(RUNTIME_IMAGE_TAR) $(IMAGES_IMAGE_TAR)

run-remote:
	ssh $(REMOTE_CONN) "$(REMOTE_ENGINE) load -i $(REMOTE_BASE_DIR)/$(RUNTIME_IMAGE_TAR)"
	ssh $(REMOTE_CONN) 'set -euo pipefail; \
	i=1; for port in $(HOST_PORTS); do \
		name=$(APP_BASENAME)_$$i; \
		$(REMOTE_ENGINE) rm -f "$$name" >/dev/null 2>&1 || true; \
		i=$$((i+1)); \
	done; \
	i=1; for port in $(HOST_PORTS); do \
		name=$(APP_BASENAME)_$$i; \
		$(REMOTE_ENGINE) run -d --restart unless-stopped --read-only --tmpfs $(REMOTE_TMPFS) --platform $(REMOTE_PLATFORM) -p $$port:$(CONTAINER_PORT) $(REMOTE_EXTRA_RUN_ARGS) --name $$name $(RUNTIME_IMAGE_NAME); \
		i=$$((i+1)); \
	done'
	ssh $(REMOTE_CONN) 'set -euo pipefail; uid=$$(id -u); export XDG_RUNTIME_DIR="/run/user/$$uid"; export DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/$$uid/bus"; mkdir -p ~/.config/systemd/user; i=1; for port in $(HOST_PORTS); do name=$(APP_BASENAME)_$$i; $(REMOTE_ENGINE) generate systemd --name "$$name" --files --new; mv -f container-"$$name".service ~/.config/systemd/user/; i=$$((i+1)); done; systemctl --user daemon-reload; i=1; for port in $(HOST_PORTS); do name=$(APP_BASENAME)_$$i; systemctl --user enable --now container-"$$name".service; i=$$((i+1)); done'
	ssh $(REMOTE_CONN) "rm -f $(REMOTE_BASE_DIR)/$(RUNTIME_IMAGE_TAR)"

run-remote-static:
	ssh $(REMOTE_CONN) "$(REMOTE_ENGINE) load -i $(REMOTE_BASE_DIR)/$(IMAGES_IMAGE_TAR)"
	ssh $(REMOTE_CONN) "$(REMOTE_ENGINE) rm -f $(IMAGES_CONTAINER_NAME) >/dev/null 2>&1 || true"
	ssh $(REMOTE_CONN) "$(REMOTE_ENGINE) run -d --restart unless-stopped --read-only --tmpfs $(REMOTE_TMPFS) --platform $(REMOTE_PLATFORM) -p $(IMAGES_HOST_PORT):$(IMAGES_CONTAINER_PORT) $(REMOTE_EXTRA_RUN_ARGS) --name $(IMAGES_CONTAINER_NAME) $(IMAGES_IMAGE_NAME)"
	ssh $(REMOTE_CONN) 'set -euo pipefail; uid=$$(id -u); export XDG_RUNTIME_DIR="/run/user/$$uid"; export DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/$$uid/bus"; mkdir -p ~/.config/systemd/user; $(REMOTE_ENGINE) generate systemd --name $(IMAGES_CONTAINER_NAME) --files --new; mv -f container-$(IMAGES_CONTAINER_NAME).service ~/.config/systemd/user/; systemctl --user daemon-reload; systemctl --user enable --now container-$(IMAGES_CONTAINER_NAME).service'
	ssh $(REMOTE_CONN) "rm -f $(REMOTE_BASE_DIR)/$(IMAGES_IMAGE_TAR)"

run-remote-images: run-remote-static

deploy-to-production: test save-image send-image run-remote run-remote-static delete-local-tars

remote-logs:
	ssh $(REMOTE_CONN) "$(REMOTE_ENGINE) logs -f $(APP_BASENAME)_1"

remote-logs-static:
	ssh $(REMOTE_CONN) "$(REMOTE_ENGINE) logs -f $(IMAGES_CONTAINER_NAME)"

remote-logs-images: remote-logs-static
