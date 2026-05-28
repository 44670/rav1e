#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
usage: tools/o3yv-azahar-repeat-bench.sh <o3yvbench.3dsx> [out_dir] [runs] [timeout_seconds] [input.o3yv] [iterations] [bench_target_us] [playback_target_us]

Runs the Old3DS benchmark bundle in Azahar multiple times, stores each full
o3yvbench.log plus playability report, and prints a compact stability summary.
This is emulator evidence only; final proof still requires a real Old3DS log.

Defaults:
  out_dir             tmp/azahar-repeat-bench
  runs                3
  timeout_seconds     120
  input.o3yv          tmp/reencode_lazy128_current.o3yv
  iterations          8
  bench_target_us     15000
  playback_target_us  41666
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

rom=${1:?missing o3yvbench.3dsx}
out_dir=${2:-tmp/azahar-repeat-bench}
runs=${3:-3}
timeout_seconds=${4:-120}
input=${5:-tmp/reencode_lazy128_current.o3yv}
iterations=${6:-8}
bench_target_us=${7:-15000}
playback_target_us=${8:-41666}

if [[ ! -f "$rom" ]]; then
  echo "missing .3dsx: $rom" >&2
  exit 1
fi
if [[ ! -f "$input" ]]; then
  echo "missing input stream: $input" >&2
  exit 1
fi
for item in runs timeout_seconds iterations bench_target_us playback_target_us; do
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

require_line() {
  local name=$1
  local line=$2
  local log=$3
  if [[ -z "$line" ]]; then
    echo "missing $name in $log" >&2
    exit 1
  fi
}

max_uint() {
  local a=$1
  local b=$2
  if (( b > a )); then
    printf '%s\n' "$b"
  else
    printf '%s\n' "$a"
  fi
}

mkdir -p "$out_dir"

playback_pass=0
bench_output_pass=0
bench_timing_pass=0
direct_timing_pass=0
max_bench_worst_us=0
max_direct_worst_us=0
max_playback_worst_work_us=0
max_late_frames=0
checksum=
checksum_status=pass

for run in $(seq 1 "$runs"); do
  log="$out_dir/run-${run}.log"
  report="$out_dir/report-${run}.txt"
  tools/o3yv-azahar-run-bench.sh \
    "$rom" "$log" "$timeout_seconds" playback_result
  tools/o3yv-old3ds-playability-report.sh \
    "$log" "$input" "$iterations" "$bench_target_us" \
    "$playback_target_us" azahar_old3ds >"$report"

  bench_line=$(grep '^bench_result ' "$log" | tail -1 || true)
  direct_line=$(grep '^direct_bench_result ' "$log" | tail -1 || true)
  playback_line=$(grep '^playback_result ' "$log" | tail -1 || true)
  require_line bench_result "$bench_line" "$log"
  require_line direct_bench_result "$direct_line" "$log"
  require_line playback_result "$playback_line" "$log"

  bench_output_status=$(kv_value "$bench_line" output_status)
  bench_timing_status=$(kv_value "$bench_line" timing_status)
  bench_worst_us=$(kv_value "$bench_line" worst_us)
  run_checksum=$(kv_value "$bench_line" checksum)
  direct_timing_status=$(kv_value "$direct_line" timing_status)
  direct_worst_us=$(kv_value "$direct_line" worst_us)
  playback_status=$(kv_value "$playback_line" status)
  playback_worst_work_us=$(kv_value "$playback_line" worst_work_us)
  late_frames=$(kv_value "$playback_line" late_frames)

  if [[ "$bench_output_status" == "pass" ]]; then
    bench_output_pass=$((bench_output_pass + 1))
  fi
  if [[ "$bench_timing_status" == "pass" ]]; then
    bench_timing_pass=$((bench_timing_pass + 1))
  fi
  if [[ "$direct_timing_status" == "pass" ]]; then
    direct_timing_pass=$((direct_timing_pass + 1))
  fi
  if [[ "$playback_status" == "pass" && "$late_frames" == "0" ]]; then
    playback_pass=$((playback_pass + 1))
  fi
  if [[ -z "$checksum" ]]; then
    checksum=$run_checksum
  elif [[ "${checksum,,}" != "${run_checksum,,}" ]]; then
    checksum_status=fail
  fi

  max_bench_worst_us=$(max_uint "$max_bench_worst_us" "$bench_worst_us")
  max_direct_worst_us=$(max_uint "$max_direct_worst_us" "$direct_worst_us")
  max_playback_worst_work_us=$(
    max_uint "$max_playback_worst_work_us" "$playback_worst_work_us"
  )
  max_late_frames=$(max_uint "$max_late_frames" "$late_frames")

  printf 'azahar_repeat_run run=%s log=%s report=%s bench_output_status=%s bench_timing_status=%s bench_worst_us=%s direct_timing_status=%s direct_worst_us=%s playback_status=%s playback_worst_work_us=%s late_frames=%s checksum=%s\n' \
    "$run" "$log" "$report" "$bench_output_status" "$bench_timing_status" \
    "$bench_worst_us" "$direct_timing_status" "$direct_worst_us" \
    "$playback_status" "$playback_worst_work_us" "$late_frames" \
    "$run_checksum"
done

status=fail
if (( playback_pass == runs && bench_output_pass == runs )) \
  && [[ "$checksum_status" == "pass" ]]; then
  if (( bench_timing_pass == runs && direct_timing_pass == runs )); then
    status=strict_pass
  else
    status=plausible
  fi
fi

summary="$out_dir/summary.txt"
{
  printf 'azahar_repeat_summary status=%s runs=%s playback_pass=%s bench_output_pass=%s bench_timing_pass=%s direct_timing_pass=%s max_bench_worst_us=%s max_direct_worst_us=%s max_playback_worst_work_us=%s max_late_frames=%s checksum_status=%s checksum=%s out_dir=%s\n' \
    "$status" "$runs" "$playback_pass" "$bench_output_pass" \
    "$bench_timing_pass" "$direct_timing_pass" "$max_bench_worst_us" \
    "$max_direct_worst_us" "$max_playback_worst_work_us" \
    "$max_late_frames" "$checksum_status" "${checksum:-unknown}" "$out_dir"
} >"$summary"

cat "$summary"
if [[ "$status" == "fail" ]]; then
  exit 1
fi
