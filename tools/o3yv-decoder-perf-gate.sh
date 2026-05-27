#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
usage: tools/o3yv-decoder-perf-gate.sh [input.o3yv] [bench_iters] [frame_iters] [stress_iters]

Builds minidecoder, reports workload stats for a representative stream, and
runs the ARM11 qemu proxy benches used for the Old3DS decoder budget.

Defaults:
  input.o3yv   tmp/reencode_workcap1000.o3yv
  bench_iters  240
  frame_iters  160
  stress_iters 160
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

input=${1:-tmp/reencode_workcap1000.o3yv}
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

arm_decoder=target/arm-unknown-linux-musleabi/release/minidecoder
stress_dir=tmp/decoder-perf-gate
stress_kinds=(all-skip prefill-shift copy16 raw-mb dc6000 ac6000 raw4x4)

echo "== native stats =="
cargo build --release -p minidecoder --features stats
target/release/minidecoder "$input" --stats

echo
echo "== arm qemu representative stream =="
cargo build --release -p minidecoder --target arm-unknown-linux-musleabi
qemu-arm -cpu arm11mpcore "$arm_decoder" "$input" --bench "$bench_iters"
qemu-arm -cpu arm11mpcore "$arm_decoder" "$input" --bench-frames "$frame_iters"

echo
echo "== arm qemu stress streams =="
cargo build --release -p minidecoder --features stress
mkdir -p "$stress_dir"
for kind in "${stress_kinds[@]}"; do
  stream="$stress_dir/${kind}.o3yv"
  target/release/o3yv-stress --kind "$kind" --output "$stream" --frames 100
  echo "-- $kind --"
  qemu-arm -cpu arm11mpcore "$arm_decoder" "$stream" --bench "$stress_iters"
done
