#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
usage: tools/o3yv-old3ds-check-log.sh <old3ds-bench.log> [target_us]

Checks a captured o3yvbench.3dsx log, normally copied from
sdmc:/o3yvbench.log after a hardware run. The harness must print a
machine-readable line like:

  bench_result status=pass iterations=8 frames=800 frames_per_iteration=100 ...

Defaults:
  target_us  15000
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

log=${1:?missing Old3DS bench log}
target_us=${2:-15000}

if [[ ! -f "$log" ]]; then
  echo "missing log: $log" >&2
  exit 1
fi

line=$(grep '^bench_result ' "$log" | tail -1 || true)
if [[ -z "$line" ]]; then
  echo "FAIL no bench_result line found" >&2
  exit 1
fi

value() {
  local key=$1
  awk -v key="$key" '
    {
      for (i = 1; i <= NF; i++) {
        split($i, kv, "=")
        if (kv[1] == key) {
          print kv[2]
          exit
        }
      }
    }
  ' <<<"$line"
}

status=$(value status)
iterations=$(value iterations)
frames=$(value frames)
frames_per_iteration=$(value frames_per_iteration)
mean_us=$(value mean_us)
worst_us=$(value worst_us)
reported_target_us=$(value target_us)
worst_frame_no=$(value worst_frame_no)

for item in status iterations frames frames_per_iteration mean_us worst_us \
  reported_target_us worst_frame_no; do
  if [[ -z "${!item}" ]]; then
    echo "FAIL missing $item in bench_result" >&2
    exit 1
  fi
done

if [[ "$status" != "pass" ]]; then
  echo "FAIL harness status=$status" >&2
  exit 1
fi
if (( iterations <= 0 || frames <= 0 || frames_per_iteration <= 0 )); then
  echo "FAIL invalid frame counts in bench_result" >&2
  exit 1
fi
if (( frames != iterations * frames_per_iteration )); then
  echo "FAIL frames=$frames does not match iterations*frames_per_iteration" >&2
  exit 1
fi
if (( reported_target_us != target_us )); then
  echo "FAIL target_us mismatch: log=$reported_target_us expected=$target_us" >&2
  exit 1
fi
if (( worst_us > target_us )); then
  echo "FAIL worst_us=$worst_us > target_us=$target_us" >&2
  exit 1
fi

printf 'PASS Old3DS bench: frames=%s mean_us=%s worst_us=%s worst_frame_no=%s target_us=%s\n' \
  "$frames" "$mean_us" "$worst_us" "$worst_frame_no" "$target_us"
