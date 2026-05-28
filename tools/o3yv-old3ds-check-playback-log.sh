#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
usage: tools/o3yv-old3ds-check-playback-log.sh <old3ds-bench.log> [target_frame_us] [expected_frames] [expected_fps] [expected_renderer]

Checks the playback_result line written by o3yvbench.3dsx after the decoder
benchmark. This validates whether the first rendered playback pass stayed
inside the intended frame budget.

Defaults:
  target_frame_us   41666
  expected_frames   unset
  expected_fps      24
  expected_renderer y2r
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

log=${1:?missing Old3DS bench log}
target_frame_us=${2:-41666}
expected_frames=${3:-}
expected_fps=${4:-24}
expected_renderer=${5:-y2r}

if [[ ! -f "$log" ]]; then
  echo "missing log: $log" >&2
  exit 1
fi

line=$(grep '^playback_result ' "$log" | tail -1 || true)
if [[ -z "$line" ]]; then
  echo "FAIL no playback_result line found" >&2
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

is_uint() {
  [[ "$1" =~ ^[0-9]+$ ]]
}

status=$(value status)
frames=$(value frames)
fps=$(value fps)
renderer=$(value renderer)
reported_target_frame_us=$(value target_frame_us)
mean_work_us=$(value mean_work_us)
mean_decode_us=$(value mean_decode_us)
mean_output_us=$(value mean_output_us)
mean_render_us=$(value mean_render_us)
worst_work_us=$(value worst_work_us)
worst_decode_us=$(value worst_decode_us)
worst_output_us=$(value worst_output_us)
worst_render_us=$(value worst_render_us)
late_frames=$(value late_frames)

for item in status frames fps renderer reported_target_frame_us mean_work_us \
  mean_decode_us mean_output_us mean_render_us worst_work_us worst_decode_us \
  worst_output_us worst_render_us late_frames; do
  if [[ -z "${!item}" ]]; then
    echo "FAIL missing $item in playback_result" >&2
    exit 1
  fi
done

for item in frames fps reported_target_frame_us mean_work_us mean_decode_us \
  mean_output_us mean_render_us worst_work_us worst_decode_us worst_output_us \
  worst_render_us late_frames target_frame_us expected_fps; do
  if ! is_uint "${!item}"; then
    echo "FAIL non-numeric $item=${!item}" >&2
    exit 1
  fi
done

if [[ -n "$expected_frames" ]] && ! is_uint "$expected_frames"; then
  echo "FAIL non-numeric expected_frames=$expected_frames" >&2
  exit 1
fi

if [[ "$status" != "pass" ]]; then
  echo "FAIL playback status=$status" >&2
  exit 1
fi
if [[ "$renderer" != "$expected_renderer" ]]; then
  echo "FAIL renderer=$renderer expected=$expected_renderer" >&2
  exit 1
fi
if (( frames <= 0 )); then
  echo "FAIL invalid playback frame count: $frames" >&2
  exit 1
fi
if [[ -n "$expected_frames" ]] && (( frames != expected_frames )); then
  echo "FAIL frames=$frames expected=$expected_frames" >&2
  exit 1
fi
if (( fps != expected_fps )); then
  echo "FAIL fps=$fps expected=$expected_fps" >&2
  exit 1
fi
if (( reported_target_frame_us != target_frame_us )); then
  echo "FAIL target_frame_us mismatch: log=$reported_target_frame_us expected=$target_frame_us" >&2
  exit 1
fi
if (( late_frames != 0 )); then
  echo "FAIL late_frames=$late_frames" >&2
  exit 1
fi
if (( worst_work_us > target_frame_us )); then
  echo "FAIL worst_work_us=$worst_work_us > target_frame_us=$target_frame_us" >&2
  exit 1
fi
if (( mean_work_us > worst_work_us )); then
  echo "FAIL mean_work_us=$mean_work_us > worst_work_us=$worst_work_us" >&2
  exit 1
fi
if (( mean_decode_us > mean_work_us )); then
  echo "FAIL mean_decode_us=$mean_decode_us > mean_work_us=$mean_work_us" >&2
  exit 1
fi
if (( mean_output_us > mean_work_us )); then
  echo "FAIL mean_output_us=$mean_output_us > mean_work_us=$mean_work_us" >&2
  exit 1
fi
if (( mean_render_us > mean_work_us )); then
  echo "FAIL mean_render_us=$mean_render_us > mean_work_us=$mean_work_us" >&2
  exit 1
fi
if (( worst_decode_us > worst_work_us )); then
  echo "FAIL worst_decode_us=$worst_decode_us > worst_work_us=$worst_work_us" >&2
  exit 1
fi
if (( worst_output_us > worst_work_us )); then
  echo "FAIL worst_output_us=$worst_output_us > worst_work_us=$worst_work_us" >&2
  exit 1
fi
if (( worst_render_us > worst_work_us )); then
  echo "FAIL worst_render_us=$worst_render_us > worst_work_us=$worst_work_us" >&2
  exit 1
fi

printf 'PASS Old3DS playback: frames=%s fps=%s renderer=%s mean_work_us=%s mean_decode_us=%s mean_output_us=%s mean_render_us=%s worst_work_us=%s worst_decode_us=%s worst_output_us=%s worst_render_us=%s late_frames=%s target_frame_us=%s\n' \
  "$frames" "$fps" "$renderer" "$mean_work_us" "$mean_decode_us" \
  "$mean_output_us" "$mean_render_us" "$worst_work_us" "$worst_decode_us" \
  "$worst_output_us" "$worst_render_us" "$late_frames" "$target_frame_us"
