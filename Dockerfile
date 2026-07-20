# Stage 1: Build stage
FROM ubuntu:latest AS builder

# Avoid prompts from apt
ENV DEBIAN_FRONTEND=noninteractive

# Update and install packages for image processing and optimization
RUN apt-get update && \
    apt-get install -y rename imagemagick jpegoptim jpeginfo file clamav && \
    apt-get clean && \
    rm -rf /var/lib/apt/lists/*

RUN freshclam

# Set working directory
WORKDIR /app

# Copy the tempimages subfolder
COPY tempimages/ /app/

# Fail the build if clamav flags anything
RUN clamscan --recursive --infected --no-summary . && echo "clamscan: clean"

# Normalise JPG/JPEG/PNG/WEBP files to fully-lowercase names
RUN find . -depth -iregex ".*\.\(jpg\|jpeg\|png\|webp\)" -type f -exec bash -c 'base=${0%.*} ext=${0##*.} a=${base,,}.${ext,,}; [ "$a" != "$0" ] && mv -- "$0" "$a"' {} \;

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

# Fail if a jpeg is corrupted based on jpeginfo exit code
RUN find . -type f -name "*.jpg" -exec sh -c \
  'for f; do jpeginfo -c "$f" > /dev/null || { echo "CORRUPT: $f"; exit 1; }; done' _ {} +

# Remove EXIF data and convert to sRGB colorspace
RUN find . -type f -name "*.jpg" -exec mogrify -auto-orient -strip -colorspace sRGB {} \;

# Bring resolution down to 1080 maximum on x and y, 70% qual on images over 500K big
RUN find . -type f -regex ".*\.\(jpg\|jpeg\)" -size +500k -exec convert {} -resize 1080x1080 -quality 70% {} \;

# Optimize jpegs
RUN find . -type f -regex ".*\.\(jpg\|jpeg\)" -exec jpegoptim --quiet --preserve --all-progressive --strip-all {} \;

# Verify every final file is genuinely a JPEG by magic bytes
RUN find . -type f -exec sh -c \
  'for f; do file --mime-type -b "$f" | grep -q "^image/jpeg$" || { echo "BAD: $f"; exit 1; }; done' _ {} +

# Exit if we still have files which aren't jpegs
#RUN non_jpg_files=$(find . -type f ! -name "*.jpg" ! -name "*.jpeg") && \
RUN non_jpg_files=$(find . -type f ! -name "*.jpg") && \
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
  RUSTFLAGS='-C target-cpu=skylake -C tune-cpu=skylake' \
  cargo build --release --target x86_64-unknown-linux-musl && \
  install -Dm755 /app/target/x86_64-unknown-linux-musl/release/dog-ceo-rust /usr/local/bin/dog-ceo-rust

# Stage 3: Minimal runtime image with only the static binary
FROM scratch AS runtime

COPY --from=rust-builder /usr/local/bin/dog-ceo-rust /usr/local/bin/dog-ceo-rust

EXPOSE 3000
USER 65532:65532
CMD ["/usr/local/bin/dog-ceo-rust"]

# Stage 4: Hardened static image host for JPG files only
FROM nginxinc/nginx-unprivileged:stable-alpine AS images

COPY nginx/nginx.conf /etc/nginx/nginx.conf
COPY nginx/images.conf /etc/nginx/conf.d/default.conf
COPY --from=builder /app/ /usr/share/nginx/html/

EXPOSE 8080