#!/usr/bin/env bash

set -euo pipefail

rm -rf tempimages
git clone --depth 1 --single-branch https://github.com/jigsawpieces/dog-api-images.git tempimages
rm -rf tempimages/.git
rm -rf tempimages/.gitignore
rm -rf tempimages/README.md
rm -rf tempimages/LICENSE

podman build --target runtime -t dog-ceo-api-rust:runtime .
podman build --target images -t dog-ceo-api-rust:images .

rm -rf tempimages