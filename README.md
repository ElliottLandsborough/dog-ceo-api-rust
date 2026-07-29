# dog-ceo-api-rust

Makefile quick guide

Run all commands from this folder:
/Users/elliott/projects/dog-ceo-api-rust

One-time setup (or when you want to refresh dog-api-images)

1. First clone of image data
make fetch-images

2. Refresh image data later
make refresh-images

3. Remove local image data clone
make cleanup-images

Local build commands

1. Build API runtime image
make build-runtime-image

2. Build static files image
make build-static-image

3. Build and save both images to local tar files
make save-image

4. Mirror processed images to Cloudflare R2
AWS_ACCESS_KEY_ID=... AWS_SECRET_ACCESS_KEY=... make upload-r2 \
	R2_ENDPOINT_URL=https://ACCOUNT_ID.r2.cloudflarestorage.com \
	R2_BUCKET=BUCKET_NAME

The default R2_PREFIX is `breeds`, matching the API image URL path. The upload mirrors
the processed image set, including deleting objects under that prefix that no longer
exist locally. A nonempty prefix is required so `--delete` cannot affect unrelated
objects across the bucket.

Local Docker test workflow

1. Build both images
make build-runtime-image
make build-static-image

2. Start API container locally
docker rm -f dog_ceo_api_rust_local >/dev/null 2>&1 || true
docker run --platform linux/amd64 -d --name dog_ceo_api_rust_local --read-only --tmpfs /tmp:rw,noexec,nosuid,nodev,size=64m -p 3000:3000 dog-ceo-api-rust:runtime

3. Start static container locally
docker rm -f dog_ceo_api_images_local >/dev/null 2>&1 || true
docker run --platform linux/amd64 -d --name dog_ceo_api_images_local --read-only --tmpfs /tmp:rw,noexec,nosuid,nodev,size=64m -p 8080:8080 dog-ceo-static:images

4. Smoke test endpoints
curl -i http://localhost:3000/
curl -i http://localhost:8080/

5. View logs
docker logs -f dog_ceo_api_rust_local
docker logs -f dog_ceo_api_images_local

6. Clean up local test containers
docker rm -f dog_ceo_api_rust_local dog_ceo_api_images_local

Remote deploy commands (AlmaLinux, rootless podman)

1. Full deploy flow (test, build, upload to R2, ship, run runtime + static)
make fetch-images `or` make refresh-images
AWS_ACCESS_KEY_ID=... AWS_SECRET_ACCESS_KEY=... make deploy-to-production \
	R2_ENDPOINT_URL=https://ACCOUNT_ID.r2.cloudflarestorage.com \
	R2_BUCKET=BUCKET_NAME

2. Follow runtime logs
make remote-logs

3. Follow static logs
make remote-logs-static

Compatibility aliases

The old static target names still work:

1. run-remote-images is an alias for run-remote-static
2. remote-logs-images is an alias for remote-logs-static
