FROM ubuntu:latest AS prep-builder

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && \
    apt-get install -y rename imagemagick jpegoptim jpeginfo file clamav && \
    apt-get clean && \
    rm -rf /var/lib/apt/lists/*

RUN freshclam

WORKDIR /app

COPY tempimages/ /app/

RUN clamscan --recursive --infected --no-summary . && echo "clamscan: clean"

RUN find . -depth -iregex ".*\.\(jpg\|jpeg\|png\|webp\)" -type f -exec bash -c 'base=${0%.*} ext=${0##*.} a=${base,,}.${ext,,}; [ "$a" != "$0" ] && mv -- "$0" "$a"' {} \;

RUN find . -name "*.png" -exec mogrify -format jpg {} \;

RUN find . -name "*.webp" -type f -exec sh -c 'convert "$1" "${1%.webp}.jpg" && rm "$1"' _ {} \;

RUN find . -name "*.png" -delete

RUN find . -type f -name '*.jpeg' -print0 | xargs -0 rename 's/\.jpeg/\.jpg/'

RUN find . -depth -name "* *" -type f -execdir bash -c 'for file; do mv -n "$file" "${file// /_}"; done' bash {} +

RUN find . -type f -name "*.jpg" -exec sh -c \
  'for f; do jpeginfo -c "$f" > /dev/null || { echo "CORRUPT: $f"; exit 1; }; done' _ {} +

RUN find . -type f -name "*.jpg" -exec mogrify -auto-orient -strip -colorspace sRGB {} \;

RUN find . -type f -regex ".*\.\(jpg\|jpeg\)" -size +500k -exec convert {} -resize 1080x1080 -quality 70% {} \;

RUN find . -type f -regex ".*\.\(jpg\|jpeg\)" -exec jpegoptim --quiet --preserve --all-progressive --strip-all {} \;

RUN find . -type f -exec sh -c \
  'for f; do file --mime-type -b "$f" | grep -q "^image/jpeg$" || { echo "BAD: $f"; exit 1; }; done' _ {} +

RUN non_jpg_files=$(find . -type f ! -name "*.jpg") && \
    if [ -n "$non_jpg_files" ]; then \
      echo "ERROR: Found non-JPG files:" && \
      echo "$non_jpg_files" && \
      exit 1; \
    else \
      echo "SUCCESS: Only JPG files found"; \
    fi

FROM scratch AS prepared-images
COPY --from=prep-builder /app/ /app/
