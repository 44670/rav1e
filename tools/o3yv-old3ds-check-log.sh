#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
usage: tools/o3yv-old3ds-check-log.sh <old3ds-bench.log> [target_us] [expected_frames_per_iteration] [expected_checksum]

Checks a captured o3yvbench.3dsx log, normally copied from
sdmc:/o3yvbench.log after a hardware run. The harness must print a
machine-readable line like:

  bench_result status=pass iterations=8 frames=800 frames_per_iteration=100 ...

Defaults:
  target_us                       15000
  expected_frames_per_iteration  unset
  expected_checksum              unset
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

log=${1:?missing Old3DS bench log}
target_us=${2:-15000}
expected_frames_per_iteration=${3:-}
expected_checksum=${4:-}

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
min_us=$(value min_us)
median_us=$(value median_us)
p95_us=$(value p95_us)
worst_us=$(value worst_us)
reported_target_us=$(value target_us)
worst_frame_no=$(value worst_frame_no)
checksum=$(value checksum)
expected_checksum_from_log=$(value expected_checksum)
expected_frames_from_log=$(value expected_frames_per_iteration)
timing_status=$(value timing_status)
output_status=$(value output_status)

is_uint() {
  [[ "$1" =~ ^[0-9]+$ ]]
}

is_hex_u64() {
  [[ "$1" =~ ^[0-9A-Fa-f]{16}$ ]]
}

for item in status iterations frames frames_per_iteration min_us mean_us \
  median_us p95_us worst_us reported_target_us worst_frame_no checksum \
  timing_status output_status; do
  if [[ -z "${!item}" ]]; then
    echo "FAIL missing $item in bench_result" >&2
    exit 1
  fi
done
for item in iterations frames frames_per_iteration min_us mean_us median_us \
  p95_us worst_us reported_target_us worst_frame_no; do
  if ! is_uint "${!item}"; then
    echo "FAIL non-numeric $item=${!item}" >&2
    exit 1
  fi
done
if [[ -n "$expected_frames_per_iteration" ]] \
  && ! is_uint "$expected_frames_per_iteration"; then
  echo "FAIL non-numeric expected_frames_per_iteration=$expected_frames_per_iteration" >&2
  exit 1
fi
if ! is_hex_u64 "$checksum"; then
  echo "FAIL invalid checksum=$checksum" >&2
  exit 1
fi
if [[ -n "$expected_checksum_from_log" ]] \
  && ! is_hex_u64 "$expected_checksum_from_log"; then
  echo "FAIL invalid expected_checksum=$expected_checksum_from_log" >&2
  exit 1
fi
if [[ -n "$expected_checksum" ]] && ! is_hex_u64 "$expected_checksum"; then
  echo "FAIL invalid expected_checksum=$expected_checksum" >&2
  exit 1
fi
if [[ -n "$expected_frames_from_log" ]] \
  && ! is_uint "$expected_frames_from_log"; then
  echo "FAIL invalid expected_frames_per_iteration=$expected_frames_from_log" >&2
  exit 1
fi

if [[ "$status" != "pass" ]]; then
  echo "FAIL harness status=$status" >&2
  exit 1
fi
if [[ "$timing_status" != "pass" ]]; then
  echo "FAIL timing_status=$timing_status" >&2
  exit 1
fi
if [[ "$output_status" != "pass" ]]; then
  echo "FAIL output_status=$output_status" >&2
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
if [[ -n "$expected_frames_per_iteration" ]] \
  && (( frames_per_iteration != expected_frames_per_iteration )); then
  echo "FAIL frames_per_iteration=$frames_per_iteration expected=$expected_frames_per_iteration" >&2
  exit 1
fi
if [[ -n "$expected_frames_from_log" ]] \
  && (( frames_per_iteration != expected_frames_from_log )); then
  echo "FAIL frames_per_iteration=$frames_per_iteration log_expected=$expected_frames_from_log" >&2
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
if (( min_us > mean_us )); then
  echo "FAIL min_us=$min_us > mean_us=$mean_us" >&2
  exit 1
fi
if (( min_us > median_us )); then
  echo "FAIL min_us=$min_us > median_us=$median_us" >&2
  exit 1
fi
if (( median_us > worst_us )); then
  echo "FAIL median_us=$median_us > worst_us=$worst_us" >&2
  exit 1
fi
if (( p95_us > worst_us )); then
  echo "FAIL p95_us=$p95_us > worst_us=$worst_us" >&2
  exit 1
fi
if (( mean_us > worst_us )); then
  echo "FAIL mean_us=$mean_us > worst_us=$worst_us" >&2
  exit 1
fi
if [[ -n "$expected_checksum" ]] \
  && [[ "${checksum,,}" != "${expected_checksum,,}" ]]; then
  echo "FAIL checksum=$checksum expected=$expected_checksum" >&2
  exit 1
fi
if [[ -n "$expected_checksum_from_log" ]] \
  && [[ "${checksum,,}" != "${expected_checksum_from_log,,}" ]]; then
  echo "FAIL checksum=$checksum log_expected=$expected_checksum_from_log" >&2
  exit 1
fi

printf 'PASS Old3DS bench: frames=%s min_us=%s mean_us=%s median_us=%s p95_us=%s worst_us=%s worst_frame_no=%s target_us=%s checksum=%s\n' \
  "$frames" "$min_us" "$mean_us" "$median_us" "$p95_us" "$worst_us" "$worst_frame_no" "$target_us" "$checksum"
