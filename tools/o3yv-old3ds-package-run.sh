#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
usage: tools/o3yv-old3ds-package-run.sh [input.o3yv] [out_dir] [iterations] [target_us]

Builds the Old3DS timing harness and writes a small run bundle containing:
  - o3yvbench.3dsx
  - expected host checksum metadata
  - the exact log-verification command to run after copying sdmc:/o3yvbench.log

Defaults:
  input.o3yv  tmp/reencode_lazy128_current.o3yv
  out_dir     tmp/o3yv-old3ds-run
  iterations  8
  target_us   15000
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

input=${1:-tmp/reencode_lazy128_current.o3yv}
out_dir=${2:-tmp/o3yv-old3ds-run}
iterations=${3:-8}
target_us=${4:-15000}

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

tools/o3yv-old3ds-build-harness.sh "$input" "$iterations" "$target_us"

if [[ ! -f old3ds/build/o3yvbench.3dsx ]]; then
  echo "build did not produce old3ds/build/o3yvbench.3dsx" >&2
  exit 1
fi

mkdir -p "$out_dir"
cp old3ds/build/o3yvbench.3dsx "$out_dir/o3yvbench.3dsx"

expected=$(
  tools/o3yv-old3ds-expected-checksum.sh "$input" "$iterations"
)
printf '%s\n' "$expected" >"$out_dir/expected.txt"
sha256sum "$out_dir/o3yvbench.3dsx" >"$out_dir/o3yvbench.3dsx.sha256"

cat >"$out_dir/RUN_ON_OLD3DS.txt" <<EOF
Copy o3yvbench.3dsx to the Old3DS SD card and launch it from the Homebrew
Launcher. The harness writes its machine-readable log to:

  sdmc:/o3yvbench.log

After the run, copy that log back beside this bundle as old3ds-bench.log and
verify timing/output determinism from the repository root with:

  tools/o3yv-old3ds-verify-log.sh "$out_dir/old3ds-bench.log" "$input" "$iterations" "$target_us"

Passing target:

  worst_us <= $target_us
EOF

echo "wrote Old3DS run bundle: $out_dir"
cat "$out_dir/o3yvbench.3dsx.sha256"
