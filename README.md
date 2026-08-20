# dog-ceo-api-rust

Run all commands from:
`/Users/elliott/projects/dog-ceo-api-rust`

## Dev Flow

This is the local development loop for code and quick checks.

1. Check and test Rust code.
```bash
make check
make test
```

2. Run the app locally (non-containerized).
```bash
make run
```

3. Optional parity checks.
```bash
make parity
make parity-start
```

4. Build runtime container locally for inspection/testing.
```bash
make build-runtime-image
```

5. Optional local static image build and local logs.
```bash
make build-static-image
make remote-logs-static
```

## Prod Flow

This is the production deployment path currently used by `deploy-to-production`.

1. Ensure image data repo is present (first time or refresh).
```bash
make fetch-images
# or
make refresh-images
```

2. Log into the registry (example).
```bash
docker login https://lhr.vultrcr.com/dogceo -u <username> -p <token>
```

3. Build and push prepared images artifact (multi-arch under `:latest`).
```bash
make push-prepared-images
```

4. Deploy runtime app to remote host.
```bash
make deploy-to-production
```

5. Check remote logs.
```bash
make remote-logs
```

## R2 Sync (Separate From Deploy)

R2 upload is a separate command flow.

```bash
AWS_ACCESS_KEY_ID=... AWS_SECRET_ACCESS_KEY=... make deploy-to-r2 \
  R2_ENDPOINT_URL=https://ACCOUNT_ID.r2.cloudflarestorage.com \
  R2_BUCKET=BUCKET_NAME
```

The default `R2_PREFIX` is `breeds`. A non-empty prefix is required because sync uses `--delete`.

## Key Variables

1. `PREPARED_IMAGES_IMAGE`
Default: `lhr.vultrcr.com/dogceo/dog-ceo-images-prep:latest`

2. `PREPARED_IMAGE_PLATFORMS`
Default: `linux/amd64,linux/arm64`

3. `REMOTE_PLATFORM`
Default: `linux/amd64`

4. `LOCAL_PLATFORM`
Default: `linux/arm64`
