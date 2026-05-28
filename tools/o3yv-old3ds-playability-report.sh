#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
usage: tools/o3yv-old3ds-playability-report.sh <old3ds-bench.log> [input.o3yv] [iterations] [bench_target_us] [playback_target_us] [evidence_label]

Summarizes an o3yvbench.3dsx log for Old3DS playability. This does not replace
the strict decoder verifier; it reports both:

  - deterministic decoded output and strict decoder benchmark status
  - rendered 24 fps playback status

Defaults:
  input.o3yv          tmp/reencode_lazy128_current.o3yv
  iterations          8
  bench_target_us     15000
  playback_target_us  41666
  evidence_label      unknown
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

log=${1:?missing Old3DS bench log}
input=${2:-tmp/reencode_lazy128_current.o3yv}
iterations=${3:-8}
bench_target_us=${4:-15000}
playback_target_us=${5:-41666}
evidence_label=${6:-unknown}

if [[ ! -f "$log" ]]; then
  echo "missing log: $log" >&2
  exit 1
fi
if [[ ! -f "$input" ]]; then
  echo "missing input stream: $input" >&2
  exit 1
fi
for item in iterations bench_target_us playback_target_us; do
  if [[ ! "${!item}" =~ ^[0-9]+$ || "${!item}" == 0 ]]; then
    echo "$item must be a positive integer" >&2
    exit 1
  fi
done

kv_value() {
  local line=$1
  local key=$2
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

require_uint() {
  local name=$1
  local value=$2
  if [[ ! "$value" =~ ^[0-9]+$ ]]; then
    echo "FAIL non-numeric $name=$value" >&2
    exit 1
  fi
}

require_hex_u64() {
  local name=$1
  local value=$2
  if [[ ! "$value" =~ ^[0-9A-Fa-f]{16}$ ]]; then
    echo "FAIL invalid $name=$value" >&2
    exit 1
  fi
}

expected=$(
  tools/o3yv-old3ds-expected-checksum.sh "$input" "$iterations"
)
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
expected_checksum=$(
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

if [[ -z "$frames_per_iteration" || -z "$expected_checksum" ]]; then
  echo "failed to parse expected metadata" >&2
  exit 1
fi
require_uint frames_per_iteration "$frames_per_iteration"
require_hex_u64 expected_checksum "$expected_checksum"

bench_line=$(grep '^bench_result ' "$log" | tail -1 || true)
if [[ -z "$bench_line" ]]; then
  echo "FAIL no bench_result line found" >&2
  exit 1
fi

bench_status=$(kv_value "$bench_line" status)
bench_frames=$(kv_value "$bench_line" frames)
bench_frames_per_iteration=$(kv_value "$bench_line" frames_per_iteration)
bench_target_reported=$(kv_value "$bench_line" target_us)
bench_worst_us=$(kv_value "$bench_line" worst_us)
bench_decode_worst_us=$(kv_value "$bench_line" worst_decode_us)
bench_output_worst_us=$(kv_value "$bench_line" worst_output_us)
bench_checksum=$(kv_value "$bench_line" checksum)
bench_timing_status=$(kv_value "$bench_line" timing_status)
bench_output_status=$(kv_value "$bench_line" output_status)

for item in bench_status bench_frames bench_frames_per_iteration \
  bench_target_reported bench_worst_us bench_checksum bench_timing_status \
  bench_output_status; do
  if [[ -z "${!item}" ]]; then
    echo "FAIL missing $item in bench_result" >&2
    exit 1
  fi
done
for item in bench_frames bench_frames_per_iteration bench_target_reported \
  bench_worst_us; do
  require_uint "$item" "${!item}"
done
for item in bench_decode_worst_us bench_output_worst_us; do
  if [[ -n "${!item}" ]]; then
    require_uint "$item" "${!item}"
  fi
done
require_hex_u64 bench_checksum "$bench_checksum"

bench_output_ok=pass
if [[ "$bench_output_status" != "pass" ]]; then
  bench_output_ok=fail
fi
if (( bench_frames_per_iteration != frames_per_iteration )); then
  bench_output_ok=fail
fi
if (( bench_frames != iterations * frames_per_iteration )); then
  bench_output_ok=fail
fi
if [[ "${bench_checksum,,}" != "${expected_checksum,,}" ]]; then
  bench_output_ok=fail
fi
if (( bench_target_reported != bench_target_us )); then
  bench_output_ok=fail
fi

bench_timing_ok=fail
if [[ "$bench_timing_status" == "pass" ]] && (( bench_worst_us <= bench_target_us )); then
  bench_timing_ok=pass
fi

playback_line=$(grep '^playback_result ' "$log" | tail -1 || true)
if [[ -z "$playback_line" ]]; then
  echo "FAIL no playback_result line found" >&2
  exit 1
fi

playback_status=$(kv_value "$playback_line" status)
playback_frames=$(kv_value "$playback_line" frames)
playback_fps=$(kv_value "$playback_line" fps)
playback_renderer=$(kv_value "$playback_line" renderer)
playback_output_mode=$(kv_value "$playback_line" output_mode)
playback_target_reported=$(kv_value "$playback_line" target_frame_us)
playback_mean_work_us=$(kv_value "$playback_line" mean_work_us)
playback_worst_work_us=$(kv_value "$playback_line" worst_work_us)
playback_worst_decode_us=$(kv_value "$playback_line" worst_decode_us)
playback_worst_output_us=$(kv_value "$playback_line" worst_output_us)
playback_worst_render_us=$(kv_value "$playback_line" worst_render_us)
playback_late_frames=$(kv_value "$playback_line" late_frames)

for item in playback_status playback_frames playback_fps playback_renderer \
  playback_output_mode playback_target_reported playback_mean_work_us \
  playback_worst_work_us playback_late_frames; do
  if [[ -z "${!item}" ]]; then
    echo "FAIL missing $item in playback_result" >&2
    exit 1
  fi
done
for item in playback_frames playback_fps playback_target_reported \
  playback_mean_work_us playback_worst_work_us playback_late_frames; do
  require_uint "$item" "${!item}"
done
for item in playback_worst_decode_us playback_worst_output_us \
  playback_worst_render_us; do
  if [[ -n "${!item}" ]]; then
    require_uint "$item" "${!item}"
  fi
done

playback_ok=fail
if [[ "$playback_status" == "pass" ]] \
  && [[ "$playback_renderer" == "y2r" ]] \
  && [[ "$playback_output_mode" == "direct_planes" ]] \
  && (( playback_frames == frames_per_iteration )) \
  && (( playback_fps == 24 )) \
  && (( playback_target_reported == playback_target_us )) \
  && (( playback_worst_work_us <= playback_target_us )) \
  && (( playback_late_frames == 0 )); then
  playback_ok=pass
fi

if [[ "$bench_output_ok" == "pass" && "$playback_ok" == "pass" ]]; then
  case "$evidence_label" in
    real_old3ds | old3ds_hardware | hardware | real_hardware)
      playability_status=pass_hardware
      ;;
    *)
      playability_status=plausible
      ;;
  esac
else
  playability_status=fail
fi

printf 'O3YV Old3DS playability report\n'
printf 'evidence_label=%s log=%s input=%s\n' \
  "$evidence_label" "$log" "$input"
printf 'expected frames_per_iteration=%s checksum=%s iterations=%s\n' \
  "$frames_per_iteration" "$expected_checksum" "$iterations"
printf 'bench_output status=%s frames=%s frames_per_iteration=%s checksum=%s output_status=%s\n' \
  "$bench_output_ok" "$bench_frames" "$bench_frames_per_iteration" \
  "$bench_checksum" "$bench_output_status"
printf 'bench_timing status=%s harness_status=%s timing_status=%s worst_us=%s target_us=%s worst_decode_us=%s worst_output_us=%s\n' \
  "$bench_timing_ok" "$bench_status" "$bench_timing_status" \
  "$bench_worst_us" "$bench_target_us" \
  "${bench_decode_worst_us:-unknown}" "${bench_output_worst_us:-unknown}"
printf 'playback status=%s renderer=%s output_mode=%s frames=%s fps=%s mean_work_us=%s worst_work_us=%s target_frame_us=%s late_frames=%s worst_decode_us=%s worst_output_us=%s worst_render_us=%s\n' \
  "$playback_ok" "$playback_renderer" "$playback_output_mode" \
  "$playback_frames" "$playback_fps" \
  "$playback_mean_work_us" "$playback_worst_work_us" \
  "$playback_target_us" "$playback_late_frames" \
  "${playback_worst_decode_us:-unknown}" \
  "${playback_worst_output_us:-unknown}" \
  "${playback_worst_render_us:-unknown}"
printf 'playability_status=%s\n' "$playability_status"

if [[ "$bench_timing_ok" != "pass" ]]; then
  printf 'note=strict_decoder_budget_not_met\n'
fi
if [[ "$playability_status" == "plausible" ]]; then
  printf 'note=emulator_or_unknown_evidence_needs_real_old3ds_log_for_final_proof\n'
fi

if [[ "$playability_status" == "fail" ]]; then
  exit 1
fi
