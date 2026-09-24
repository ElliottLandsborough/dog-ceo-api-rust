#!/usr/bin/env bash

set -euo pipefail

# Keep these values easy to update when the public API or test image changes.
JSON_URL="${JSON_URL:-https://dog.ceo/api/breeds/image/random}"
IMAGE_URL="${IMAGE_URL:-https://images.dog.ceo/breeds/pinscher-miniature/n02107312_579.jpg}"

if ! command -v curl >/dev/null 2>&1; then
  echo "Missing required command: curl" >&2
  exit 2
fi

if ! command -v python3 >/dev/null 2>&1; then
  echo "Missing required command: python3" >&2
  exit 2
fi

TEMP_DIR="$(mktemp -d)"
trap 'rm -rf "${TEMP_DIR}"' EXIT

check_response() {
  local name="$1"
  local url="$2"
  local body_file="${TEMP_DIR}/${name}.body"
  local headers_file="${TEMP_DIR}/${name}.headers"
  local http_code

  http_code="$(curl -sS -L -D "${headers_file}" -o "${body_file}" -w '%{http_code}' "${url}")"
  if [[ "${http_code}" != 2* ]]; then
    echo "FAIL ${name}: ${url} returned HTTP ${http_code}" >&2
    return 1
  fi

  python3 - "${name}" "${headers_file}" <<'PY'
import sys

name, headers_path = sys.argv[1:]
text = open(headers_path, encoding="utf-8").read()
blocks = [block for block in text.split("\nHTTP/") if block.strip()]
headers = blocks[-1].splitlines()
values = {}
for line in headers[1:]:
    if ":" in line:
        key, value = line.split(":", 1)
        values[key.strip().lower()] = value.strip().lower()

required = {
    "access-control-allow-origin": "*",
}
if name == "image":
  required["cross-origin-resource-policy"] = "cross-origin"

failed = False
for key, expected in required.items():
    actual = values.get(key)
    if actual != expected:
        print(f"FAIL {name}: {key}: expected {expected!r}, got {actual!r}", file=sys.stderr)
        failed = True

if failed:
    sys.exit(1)

print(f"OK   {name}: required headers present")
PY
}

check_response "json" "${JSON_URL}"
python3 - "${TEMP_DIR}/json.body" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as file:
    response = json.load(file)

if response.get("status") != "success" or not isinstance(response.get("message"), str):
    print("FAIL json: expected a successful response with a string message", file=sys.stderr)
    sys.exit(1)

print(f"OK   json: valid payload with image URL {response['message']}")
PY

check_response "image" "${IMAGE_URL}"
echo "Public header check passed"