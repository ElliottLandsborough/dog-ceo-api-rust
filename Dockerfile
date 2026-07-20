# Stage 1: Build stage
FROM ubuntu:latest AS builder

# Avoid prompts from apt
ENV DEBIAN_FRONTEND=noninteractive

# Update and install tmux
RUN apt-get update && \
    apt-get install -y rename imagemagick jpegoptim && \
    apt-get clean && \
    rm -rf /var/lib/apt/lists/*

# Set working directory
WORKDIR /app

# Copy the tempimages subfolder
COPY tempimages/ /app/

# Find uppercase 'JPG', 'JPEG' or 'PNG' and rename to lowercase equivalents
RUN find . -depth -regex ".*\.\(JPG\|JPEG\|PNG\)" -type f -exec bash -c 'base=${0%.*} ext=${0##*.} a=${base,,}.${ext,,}; [ "$a" != "$0" ] && mv -- "$0" "$a"' {} \;

# Convert png files to jpg
RUN find . -name "*.png" -exec mogrify -format jpg {} \;

# Convert webp files to jpg
RUN find . -name "*.webp" -type f -exec sh -c 'convert "$1" "${1%.webp}.jpg" && rm "$1"' _ {} \;

# Delete png files
RUN find . -name "*.png" -delete;

# Rename jpeg to jpg
RUN find . -type f -name '*.jpeg' -print0 | xargs -0 rename 's/\.jpeg/\.jpg/';

# Replace spaces with underscores
RUN find . -depth -name "* *" -type f -execdir bash -c 'for file; do mv -n "$file" "${file// /_}"; done' bash {} +;

# Bring resolution down to 1080 maximum on x and y, 70% qual on images over 500K big
RUN find . -type f -regex ".*\.\(jpg\|jpeg\)" -size +500k -exec convert {} -resize 1080x1080 -quality 70% {} \;

# Optimize jpegs
RUN find . -type f -regex ".*\.\(jpg\|jpeg\)" -exec jpegoptim --quiet --preserve --all-progressive --strip-all {} \;

# Exit if we still have files which aren't jpegs
RUN non_jpg_files=$(find . -type f ! -name "*.jpg" ! -name "*.jpeg") && \
    if [ -n "$non_jpg_files" ]; then \
      echo "ERROR: Found non-JPG files:" && \
      echo "$non_jpg_files" && \
      exit 1; \
    else \
      echo "SUCCESS: Only JPG files found"; \
    fi

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
COPY --from=builder /app/ /images/

WORKDIR /app

# Copy the Rust application source
COPY Cargo.toml Cargo.lock ./
COPY src/ ./src/

# Embed the processed image inventory and build a static Linux x86_64 binary.
RUN find /images -type f -printf 'dog-api-images/%P\0' > /app/manifest.nul && \
  rustup target add x86_64-unknown-linux-musl && \
  CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld \
  cargo build --release --target x86_64-unknown-linux-musl && \
  install -Dm755 /app/target/x86_64-unknown-linux-musl/release/dog-ceo-rust /usr/local/bin/dog-ceo-rust

# Stage 3: Minimal runtime image with only the static binary
FROM scratch AS runtime

COPY --from=rust-builder /usr/local/bin/dog-ceo-rust /usr/local/bin/dog-ceo-rust

EXPOSE 3000
USER 65532:65532
CMD ["/usr/local/bin/dog-ceo-rust"]