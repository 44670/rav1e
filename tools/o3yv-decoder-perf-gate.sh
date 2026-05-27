#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
usage: tools/o3yv-decoder-perf-gate.sh [input.o3yv] [bench_iters] [frame_iters] [stress_iters]

Builds minidecoder, reports workload stats for a representative stream, and
runs the ARM11 qemu proxy benches used for the Old3DS decoder budget.
The gate enforces conservative proxy ceilings; override them with the
O3YV_GATE_* environment variables below when intentionally retuning.

Defaults:
  input.o3yv   tmp/reencode_lazy128_current.o3yv
  bench_iters  240
  frame_iters  160
  stress_iters 160

Default ceilings:
  O3YV_GATE_MAX_P_WORK        1000000 estimated units
  O3YV_GATE_REP_MEDIAN_MS     0.60 ms/frame
  O3YV_GATE_REP_OUTPUT_MS     0.75 ms/frame
  O3YV_GATE_WORST_FRAME_MS    1.20 ms
  O3YV_GATE_STRESS_*_MS       per stress kind, see script
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

input=${1:-tmp/reencode_lazy128_current.o3yv}
bench_iters=${2:-240}
frame_iters=${3:-160}
stress_iters=${4:-160}

if [[ ! -f "$input" ]]; then
  echo "missing input stream: $input" >&2
  exit 1
fi

if ! command -v qemu-arm >/dev/null 2>&1; then
  echo "qemu-arm is required for the ARM11 proxy benches" >&2
  exit 1
fi

arm_target=arm-unknown-linux-musleabi
arm_decoder=target/${arm_target}/release/minidecoder
stress_dir=tmp/decoder-perf-gate
stress_kinds=(all-skip prefill-shift copy16 raw-mb dc6000 ac6000 raw4x4)

max_p_work=${O3YV_GATE_MAX_P_WORK:-1000000}
rep_median_ms=${O3YV_GATE_REP_MEDIAN_MS:-0.60}
rep_output_ms=${O3YV_GATE_REP_OUTPUT_MS:-0.75}
worst_frame_ms=${O3YV_GATE_WORST_FRAME_MS:-1.20}

metric() {
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
  '
}

check_le() {
  local name=$1
  local value=$2
  local limit=$3
  awk -v value="$value" -v limit="$limit" 'BEGIN { exit !(value <= limit) }' \
    || {
      echo "FAIL $name: $value > $limit" >&2
      exit 1
    }
  echo "PASS $name: $value <= $limit"
}

stress_limit_ms() {
  case "$1" in
    all-skip) echo "${O3YV_GATE_STRESS_ALL_SKIP_MS:-0.03}" ;;
    prefill-shift) echo "${O3YV_GATE_STRESS_PREFILL_SHIFT_MS:-0.15}" ;;
    copy16) echo "${O3YV_GATE_STRESS_COPY16_MS:-0.35}" ;;
    raw-mb) echo "${O3YV_GATE_STRESS_RAW_MB_MS:-0.30}" ;;
    dc6000) echo "${O3YV_GATE_STRESS_DC6000_MS:-0.55}" ;;
    ac6000) echo "${O3YV_GATE_STRESS_AC6000_MS:-1.40}" ;;
    raw4x4) echo "${O3YV_GATE_STRESS_RAW4X4_MS:-0.60}" ;;
    *) echo "unknown stress kind: $1" >&2; exit 1 ;;
  esac
}

ensure_rust_target() {
  if ! command -v rustup >/dev/null 2>&1; then
    return
  fi
  if rustup target list --installed | grep -Fxq "$arm_target"; then
    return
  fi
  echo "installing Rust target: $arm_target"
  rustup target add "$arm_target"
}

echo "== native stats =="
cargo build --release -p minidecoder --no-default-features --features stats
stats_output=$(target/release/minidecoder "$input" --stats 2>&1)
echo "$stats_output"
p_max=$(printf '%s\n' "$stats_output" | metric p_max)
check_le "estimated max P work" "$p_max" "$max_p_work"

echo
echo "== arm qemu representative stream =="
ensure_rust_target
cargo build --release -p minidecoder --no-default-features --features std --target "$arm_target"
bench_output=$(
  qemu-arm -cpu arm11mpcore "$arm_decoder" "$input" --bench "$bench_iters" 2>&1
)
echo "$bench_output"
rep_median=$(printf '%s\n' "$bench_output" | metric median)
check_le "representative median ms/frame" "$rep_median" "$rep_median_ms"

output_bench_output=$(
  qemu-arm -cpu arm11mpcore "$arm_decoder" "$input" \
    --bench-output "$bench_iters" 2>&1
)
echo "$output_bench_output"
rep_output=$(printf '%s\n' "$output_bench_output" | metric median)
check_le "representative output median ms/frame" \
  "$rep_output" "$rep_output_ms"

frame_output=$(
  qemu-arm -cpu arm11mpcore "$arm_decoder" "$input" \
    --bench-frames "$frame_iters" 2>&1
)
echo "$frame_output"
worst_median=$(
  printf '%s\n' "$frame_output" | awk '/^frame index=/ { print }' | head -1 \
    | metric median
)
check_le "worst-frame median ms" "$worst_median" "$worst_frame_ms"

echo
echo "== arm qemu stress streams =="
cargo build --release -p minidecoder --no-default-features --features stress
mkdir -p "$stress_dir"
for kind in "${stress_kinds[@]}"; do
  stream="$stress_dir/${kind}.o3yv"
  target/release/o3yv-stress --kind "$kind" --output "$stream" --frames 100
  echo "-- $kind --"
  stress_output=$(
    qemu-arm -cpu arm11mpcore "$arm_decoder" "$stream" --bench "$stress_iters" 2>&1
  )
  echo "$stress_output"
  stress_median=$(printf '%s\n' "$stress_output" | metric median)
  check_le "$kind stress median ms/frame" \
    "$stress_median" "$(stress_limit_ms "$kind")"
done
