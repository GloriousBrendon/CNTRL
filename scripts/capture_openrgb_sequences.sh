#!/usr/bin/env bash
set -euo pipefail

# Capture OpenRGB device traffic for CNTRL backend reverse-engineering.
# This script avoids the UI by applying a repeatable mode/color sequence from CLI,
# while strace records file descriptors and ioctl/read/write payload metadata.

need_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "Missing required command: $1" >&2
    exit 1
  fi
}

need_cmd openrgb
need_cmd strace

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${OUT_DIR:-$ROOT/captures/openrgb}"
mkdir -p "$OUT_DIR"

TS="$(date +%Y%m%d_%H%M%S)"
RED="${RED_COLOR_HEX:-FF0000}"

# Prefer numeric device ids (stable and avoids string match failures).
GPU_DEVICE_ID="${GPU_DEVICE_ID:-0}"
ITE_DEVICE_ID="${ITE_DEVICE_ID:-1}"

GPU_DIRECT_MODE="${GPU_DIRECT_MODE:-Direct}"
GPU_ALT_MODE="${GPU_ALT_MODE:-Pulse}"
ITE_DIRECT_MODE="${ITE_DIRECT_MODE:-Direct}"
ITE_ALT_MODE="${ITE_ALT_MODE:-Breathing}"

TRACE_EVENTS="openat,close,read,write,ioctl"

run_traced() {
  local label="$1"
  shift
  local log="$OUT_DIR/${TS}_${label}.strace.log"
  local stdout_log="$OUT_DIR/${TS}_${label}.stdout.log"
  echo ""
  echo "== $label =="
  echo "Command: openrgb $*"
  # Keep OpenRGB stdout/stderr in a file so traced writes are not redirected to /dev/null.
  strace -f -xx -yy -s 512 -o "$log" -e "trace=$TRACE_EVENTS" openrgb "$@" >"$stdout_log" 2>&1 || true
  echo "Trace: $log"
  echo "Stdout: $stdout_log"
}

echo "Output directory: $OUT_DIR"
echo "Timestamp prefix: $TS"
echo "Using RED color: $RED"
echo "GPU device id: $GPU_DEVICE_ID"
echo "ITE device id: $ITE_DEVICE_ID"

run_traced "00_list_devices" --list-devices --verbose --loglevel trace --print-source

# GPU sequence
run_traced "10_gpu_direct_red" -d "$GPU_DEVICE_ID" -m "$GPU_DIRECT_MODE" -c "$RED" -b 100 --verbose --loglevel trace --print-source
run_traced "11_gpu_alt_red" -d "$GPU_DEVICE_ID" -m "$GPU_ALT_MODE" -c "$RED" -b 100 --verbose --loglevel trace --print-source

# Motherboard/case (ITE) sequence
run_traced "20_ite_direct_red" -d "$ITE_DEVICE_ID" -m "$ITE_DIRECT_MODE" -c "$RED" -b 100 --verbose --loglevel trace --print-source
run_traced "21_ite_alt_red" -d "$ITE_DEVICE_ID" -m "$ITE_ALT_MODE" -c "$RED" -b 100 --verbose --loglevel trace --print-source

# Optional "apply all devices" behavior to mimic UI fallback.
run_traced "30_all_devices_direct_red" -m "$ITE_DIRECT_MODE" -c "$RED" -b 100 --verbose --loglevel trace --print-source

echo ""
echo "Done. Captured logs:"
ls -1 "$OUT_DIR" | sed "s#^#$OUT_DIR/#"
echo ""
echo "Next: share the newest *.strace.log files for parsing."
