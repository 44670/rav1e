#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
usage: tools/o3yv-old3ds-readiness.sh [representative.o3yv]

Checks whether the decoder core still builds without Rust std and whether this
machine has enough Old3DS homebrew tooling installed to build and run a real
hardware decoder timing harness. This does not replace hardware timing; it
makes missing prerequisites explicit.

Defaults:
  representative.o3yv   tmp/reencode_lazy128_current.o3yv
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

input=${1:-tmp/reencode_lazy128_current.o3yv}
missing=0

check_file() {
  local label=$1
  local path=$2
  if [[ -f "$path" ]]; then
    echo "PASS $label: $path"
  else
    echo "MISS $label: $path" >&2
    missing=1
  fi
}

check_dir() {
  local label=$1
  local path=$2
  if [[ -d "$path" ]]; then
    echo "PASS $label: $path"
  else
    echo "MISS $label: $path" >&2
    missing=1
  fi
}

check_command() {
  local command=$1
  if command -v "$command" >/dev/null 2>&1; then
    echo "PASS command: $command ($(command -v "$command"))"
  else
    echo "MISS command: $command" >&2
    missing=1
  fi
}

check_minidecoder_nostd() {
  local log=/tmp/o3yv-minidecoder-nostd-check.log
  if cargo check -p minidecoder --lib --no-default-features \
    >"$log" 2>&1; then
    echo "PASS minidecoder no_std+alloc lib build"
  else
    echo "MISS minidecoder no_std+alloc lib build; see $log" >&2
    cat "$log" >&2
    missing=1
  fi
}

check_minidecoder_alloc_free_decode() {
  local input=$1
  local log=/tmp/o3yv-minidecoder-alloc-check.log
  if cargo run --release -q -p minidecoder --no-default-features \
    --features alloc-check --bin o3yv-alloc-check -- "$input" \
    >"$log" 2>&1; then
    echo "PASS minidecoder reusable-state decode alloc check: $(cat "$log")"
  else
    echo "MISS minidecoder reusable-state decode alloc check; see $log" >&2
    cat "$log" >&2
    missing=1
  fi
}

check_file "representative stream" "$input"
check_command cargo
if command -v cargo >/dev/null 2>&1; then
  check_minidecoder_nostd
  if [[ -f "$input" ]]; then
    check_minidecoder_alloc_free_decode "$input"
  fi
fi
check_command arm-none-eabi-gcc
check_command makerom
check_command 3dsxtool

if [[ -n "${DEVKITPRO:-}" ]]; then
  check_dir DEVKITPRO "$DEVKITPRO"
else
  echo "MISS env: DEVKITPRO" >&2
  missing=1
fi

if [[ -n "${DEVKITARM:-}" ]]; then
  check_dir DEVKITARM "$DEVKITARM"
else
  echo "MISS env: DEVKITARM" >&2
  missing=1
fi

if [[ -n "${DEVKITPRO:-}" ]]; then
  check_file "libctru 3ds.h" "$DEVKITPRO/libctru/include/3ds.h"
fi

if (( missing != 0 )); then
  cat >&2 <<'EOF'

Old3DS hardware timing is not ready on this machine. Install devkitPro with
devkitARM, libctru, makerom, and 3dsxtool, then add a 3DS timing harness that
loads the representative stream and measures decode frame time on hardware.
EOF
  exit 1
fi

echo "Old3DS homebrew toolchain prerequisites are present."
