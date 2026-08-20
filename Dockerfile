ARG PREPARED_IMAGES_IMAGE=dog-ceo-images-prep:latest
FROM ${PREPARED_IMAGES_IMAGE} AS prepared-images

# Stage 2: Build the Rust binary
FROM rust:bookworm AS rust-builder

# Avoid prompts from apt
ENV DEBIAN_FRONTEND=noninteractive

# Install runtime utilities and certificates
RUN apt-get update && \
    apt-get install -y ca-certificates && \
    apt-get clean && \
    rm -rf /var/lib/apt/lists/*

# Set working directory
WORKDIR /images

# Copy all files from builder stage
COPY --from=prepared-images /app/ /images/

WORKDIR /app

# Copy the Rust application source
COPY Cargo.toml Cargo.lock ./
COPY src/ ./src/

ARG RUST_TARGET_CPU=x86-64-v3

# Embed the processed image inventory and build a static Linux x86_64 binary.
RUN find /images -type f -printf 'dog-api-images/%P\0' > /app/manifest.nul && \
  rustup target add x86_64-unknown-linux-musl && \
  CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld \
  RUSTFLAGS="-C target-cpu=${RUST_TARGET_CPU}" \
  cargo build --release --target x86_64-unknown-linux-musl && \
  install -Dm755 /app/target/x86_64-unknown-linux-musl/release/dog-ceo-api-rust /usr/local/bin/dog-ceo-api-rust

# Stage 3: Minimal runtime image with only the static binary
FROM scratch AS runtime

COPY --from=rust-builder /usr/local/bin/dog-ceo-api-rust /usr/local/bin/dog-ceo-api-rust

EXPOSE 3000
USER 65532:65532
CMD ["/usr/local/bin/dog-ceo-api-rust"]

# Stage 4: Hardened static image host for JPG files only
FROM nginxinc/nginx-unprivileged:stable-alpine AS images

COPY nginx/nginx.conf /etc/nginx/nginx.conf
COPY nginx/images.conf /etc/nginx/conf.d/images.conf
COPY --from=prepared-images /app/ /usr/share/nginx/html/

EXPOSE 8080

# Stage 5: Upload the validated JPG files to an S3-compatible object store.
FROM amazon/aws-cli:latest AS r2-uploader

COPY --from=prepared-images /app/ /images/

ENTRYPOINT ["aws"]