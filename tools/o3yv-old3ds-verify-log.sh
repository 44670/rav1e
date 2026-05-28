#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
usage: tools/o3yv-old3ds-verify-log.sh <old3ds-bench.log> [input.o3yv] [iterations] [target_us]

Recomputes the host-side expected frame count/checksum for a representative
O3YV stream, then validates a captured sdmc:/o3yvbench.log from Old3DS
hardware against those values and the timing target.

Defaults:
  input.o3yv  tmp/reencode_lazy128_current.o3yv
  iterations  8
  target_us   15000
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

log=${1:?missing Old3DS bench log}
input=${2:-tmp/reencode_lazy128_current.o3yv}
iterations=${3:-8}
target_us=${4:-15000}

if [[ ! -f "$log" ]]; then
  echo "missing log: $log" >&2
  exit 1
fi
if [[ ! -f "$input" ]]; then
  echo "missing input stream: $input" >&2
  exit 1
fi
if [[ ! "$iterations" =~ ^[0-9]+$ || "$iterations" == 0 ]]; then
  echo "iterations must be a positive integer" >&2
  exit 1
fi
if [[ ! "$target_us" =~ ^[0-9]+$ || "$target_us" == 0 ]]; then
  echo "target_us must be a positive integer" >&2
  exit 1
fi

expected=$(
  tools/o3yv-old3ds-expected-checksum.sh "$input" "$iterations"
)
printf '%s\n' "$expected"

frames_per_iteration=$(
  awk -F'[ =]' '
    /^old3ds_check_args / {
      for (i = 1; i <= NF; i++) {
        if ($i == "frames_per_iteration") {
          print $(i + 1)
          exit
        }
      }
    }
  ' <<<"$expected"
)
checksum=$(
  awk -F'[ =]' '
    /^old3ds_check_args / {
      for (i = 1; i <= NF; i++) {
        if ($i == "checksum") {
          print $(i + 1)
          exit
        }
      }
    }
  ' <<<"$expected"
)

if [[ -z "$frames_per_iteration" || -z "$checksum" ]]; then
  echo "failed to parse expected metadata" >&2
  exit 1
fi

tools/o3yv-old3ds-check-log.sh \
  "$log" "$target_us" "$frames_per_iteration" "$checksum"
