#!/usr/bin/env bash
set -euo pipefail

HOSTS=(golf)
ENDPOINT_PATH="/api/breeds/image/random"

pick_tool() {
  if command -v hey >/dev/null 2>&1; then
    echo "hey"
    return
  fi
  if command -v wrk >/dev/null 2>&1; then
    echo "wrk"
    return
  fi
  if command -v ab >/dev/null 2>&1; then
    echo "ab"
    return
  fi
  if command -v curl >/dev/null 2>&1; then
    echo "curl"
    return
  fi
  echo ""
}

run_benchmark() {
  local tool="$1"
  local url="$2"

  case "$tool" in
    hey)
      hey -z 30s -c 100 "$url"
      ;;
    wrk)
      wrk -t4 -c100 -d30s "$url"
      ;;
    ab)
      ab -n 5000 -c 100 "$url"
      ;;
    curl)
      for _ in {1..20}; do
        curl -s -o /dev/null -w "status=%{http_code} total=%{time_total}s connect=%{time_connect}s ttfb=%{time_starttransfer}s\n" "$url"
      done
      ;;
    *)
      return 1
      ;;
  esac
}

TOOL="$(pick_tool)"
if [ -z "$TOOL" ]; then
  echo "No benchmark tool found. Install one of: hey, wrk, ab (ApacheBench), or use curl." >&2
  echo "On macOS with Homebrew: brew install hey   (or: brew install wrk)" >&2
  exit 1
fi

echo "Using benchmark tool: $TOOL"
for h in "${HOSTS[@]}"; do
  ip="$(ssh -G "$h" | awk '/^hostname /{print $2}')"
  url="http://$ip$ENDPOINT_PATH"
  echo "=================== $h ($ip) ==================="
  run_benchmark "$TOOL" "$url" | tee "bench-nginx-$h.txt"
done