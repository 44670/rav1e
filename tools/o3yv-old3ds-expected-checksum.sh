#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
usage: tools/o3yv-old3ds-expected-checksum.sh [input.o3yv] [iterations]

Computes the host-side decoded/output checksum expected from o3yvbench.3dsx.

Defaults:
  input.o3yv  tmp/reencode_lazy128_current.o3yv
  iterations  8
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

input=${1:-tmp/reencode_lazy128_current.o3yv}
iterations=${2:-8}

if [[ ! -f "$input" ]]; then
  echo "missing input stream: $input" >&2
  exit 1
fi
if [[ ! "$iterations" =~ ^[0-9]+$ || "$iterations" == 0 ]]; then
  echo "iterations must be a positive integer" >&2
  exit 1
fi

output=$(
  cargo run --release -q -p minidecoder --bin minidecoder \
    --no-default-features --features std \
    -- "$input" --output-checksum "$iterations" 2>&1
)

printf '%s\n' "$output"
frames=$(
  printf '%s\n' "$output" \
    | awk -F= '/^frames_per_iteration=/ { print $2; exit }'
)
checksum=$(
  printf '%s\n' "$output" | awk -F= '/^checksum=/ { print $2; exit }'
)

if [[ -z "$frames" || -z "$checksum" ]]; then
  echo "failed to parse checksum output" >&2
  exit 1
fi

printf 'old3ds_check_args target_us=15000 frames_per_iteration=%s checksum=%s\n' \
  "$frames" "$checksum"
