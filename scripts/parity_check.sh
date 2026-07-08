#!/usr/bin/env bash

set -euo pipefail

export PATH="/Users/elliott/.cargo/bin:/usr/bin:/bin:/usr/sbin:/sbin:${PATH:-}"

LIVE_BASE="https://dog.ceo/api"
LOCAL_BASE="http://127.0.0.1:3000"
START_LOCAL=0

usage() {
  echo "Usage: $0 [--start-local] [--live-base URL] [--local-base URL]"
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --start-local)
      START_LOCAL=1
      shift
      ;;
    --live-base)
      LIVE_BASE="$2"
      shift 2
      ;;
    --local-base)
      LOCAL_BASE="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1"
      usage
      exit 2
      ;;
  esac
done

if ! command -v /usr/bin/curl >/dev/null 2>&1; then
  echo "Missing required command: /usr/bin/curl"
  exit 2
fi

if ! command -v /usr/bin/python3 >/dev/null 2>&1; then
  echo "Missing required command: /usr/bin/python3"
  exit 2
fi

SERVER_PID=""
cleanup() {
  if [[ -n "${SERVER_PID}" ]]; then
    kill "${SERVER_PID}" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

if [[ "${START_LOCAL}" -eq 1 ]]; then
  echo "Starting local server with /bin/bash ./run-prod.sh"
  /bin/bash ./run-prod.sh >/tmp/dog-ceo-rust-parity-server.log 2>&1 &
  SERVER_PID=$!

  echo "Waiting for local server at ${LOCAL_BASE}"
  for ((i = 0; i < 60; i++)); do
    if /usr/bin/curl -sS "${LOCAL_BASE}/breeds/list/all" >/dev/null 2>&1; then
      break
    fi
    /bin/sleep 0.5
  done

  if ! /usr/bin/curl -sS "${LOCAL_BASE}/breeds/list/all" >/dev/null 2>&1; then
    echo "Local server did not become ready. See /tmp/dog-ceo-rust-parity-server.log"
    exit 1
  fi
fi

# format: path|mode
# mode controls how parity is validated.
TESTS=$(cat <<'EOF'
/breeds/image/random|shape_string
/breeds/image/random/3|shape_array
/breeds/list/all|shape_object
/breeds/list|shape_array
/breeds/list/random|shape_string
/breeds/list/random/1|shape_array
/breeds/list/random/3|shape_array
/breeds/list/all/random|shape_object
/breeds/list/all/random/3|shape_object
/breed/hound/images|shape_array
/breed/hound/images/random|shape_string
/breed/hound/images/random/3|shape_array
/breed/hound/list|shape_array
/breed/hound/list/random|shape_string
/breed/hound/list/random/3|shape_array
/breed/hound/afghan/images|shape_array
/breed/hound/afghan/images/random|shape_string
/breed/hound/afghan/images/random/3|shape_array
/breed/hound|error_no_info
/breed/hound/afghan|error_no_info
/breed/notabreed|error_main_missing
/breed/notabreed/images|error_main_missing
/breed/notabreed/images/random|error_main_missing
/breed/notabreed/images/random/3|error_main_missing
/breed/notabreed/list|error_main_missing
/breed/notabreed/list/random|error_main_missing
/breed/notabreed/list/random/3|error_main_missing
/breed/hound/notasub|error_sub_missing
/breed/hound/notasub/images|error_sub_missing
/breed/hound/notasub/images/random|error_sub_missing
/breed/hound/notasub/images/random/3|error_sub_missing
/breed/beagle/list/random|error_no_sub_breeds
/breed/beagle/list/random/3|error_no_sub_breeds
EOF
)

PASS=0
FAIL=0

while IFS='|' read -r path mode; do
  [[ -z "${path}" ]] && continue

  live_file=$(/usr/bin/mktemp)
  local_file=$(/usr/bin/mktemp)

  live_code=$(/usr/bin/curl -sS -o "${live_file}" -w "%{http_code}" "${LIVE_BASE}${path}")
  local_code=$(/usr/bin/curl -sS -o "${local_file}" -w "%{http_code}" "${LOCAL_BASE}${path}")

  if /usr/bin/python3 - "${path}" "${mode}" "${live_code}" "${local_code}" "${live_file}" "${local_file}" <<'PY'
import json
import sys

path, mode, live_code, local_code, live_file, local_file = sys.argv[1:7]

with open(live_file, "r", encoding="utf-8") as f:
    live = json.load(f)
with open(local_file, "r", encoding="utf-8") as f:
    local = json.load(f)

def fail(msg):
    print(f"FAIL {path} :: {msg}")
    sys.exit(1)

def ok(msg):
    print(f"OK   {path} :: {msg}")
    sys.exit(0)

if live_code != local_code:
    fail(f"http code live={live_code} local={local_code}")

if mode == "shape_string":
    if live.get("status") != local.get("status"):
        fail("status field mismatch")
    if not isinstance(local.get("message"), str):
        fail("local message is not string")
    ok("string shape matched")

if mode == "shape_array":
    if live.get("status") != local.get("status"):
        fail("status field mismatch")
    if not isinstance(local.get("message"), list):
        fail("local message is not array")
    if not isinstance(live.get("message"), list):
        fail("live message is not array")
    if len(local.get("message")) != len(live.get("message")):
        fail(f"array length mismatch live={len(live.get('message'))} local={len(local.get('message'))}")
    ok("array shape and length matched")

if mode == "shape_object":
    if live.get("status") != local.get("status"):
        fail("status field mismatch")
    if not isinstance(local.get("message"), dict):
        fail("local message is not object")
    if not isinstance(live.get("message"), dict):
        fail("live message is not object")
    if len(local.get("message")) != len(live.get("message")):
        fail(f"object entry count mismatch live={len(live.get('message'))} local={len(local.get('message'))}")
    ok("object shape and count matched")

error_map = {
    "error_main_missing": "Breed not found (main breed does not exist)",
    "error_sub_missing": "Breed not found (sub breed does not exist)",
    "error_no_sub_breeds": "Breed not found (no sub breeds exist for this main breed)",
    "error_no_info": "Breed not found (No info file for this breed exists)",
}

if mode in error_map:
    expected = {
        "status": "error",
        "message": error_map[mode],
        "code": 404,
    }
    if local != expected:
        fail(f"local error payload mismatch expected={expected} local={local}")
    if live != expected:
        fail(f"live payload changed from expected={expected} live={live}")
    ok("error payload matched exactly")

fail(f"unknown mode {mode}")
PY
  then
    PASS=$((PASS + 1))
  else
    FAIL=$((FAIL + 1))
  fi

  /bin/rm -f "${live_file}" "${local_file}"
done <<< "${TESTS}"

echo
if [[ "${FAIL}" -eq 0 ]]; then
  echo "Parity check passed: ${PASS} passed, ${FAIL} failed"
  exit 0
fi

echo "Parity check failed: ${PASS} passed, ${FAIL} failed"
exit 1
