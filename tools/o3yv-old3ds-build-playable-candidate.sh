#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
usage: tools/o3yv-old3ds-build-playable-candidate.sh <input.yuv> [stream.o3yv] [bundle_dir] [frames] [keyint] [iterations] [target_us]

Encodes the current Old3DS direct-plane playback candidate, validates it with
closed-loop decode, and packages an o3yvbench.3dsx run bundle.

Defaults:
  stream.o3yv  tmp/reencode_old3ds_directpass.o3yv
  bundle_dir   tmp/o3yv-old3ds-playable
  frames       100
  keyint       16
  iterations   8
  target_us    15000
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

input_yuv=${1:?missing input YUV420 SBS file}
stream=${2:-tmp/reencode_old3ds_directpass.o3yv}
bundle_dir=${3:-tmp/o3yv-old3ds-playable}
frames=${4:-100}
keyint=${5:-16}
iterations=${6:-8}
target_us=${7:-15000}

if [[ ! -f "$input_yuv" ]]; then
  echo "missing input YUV420 SBS file: $input_yuv" >&2
  exit 1
fi
if [[ "$stream" == "-" ]]; then
  echo "stream output must be a file path" >&2
  exit 1
fi
for item in frames keyint iterations target_us; do
  if [[ ! "${!item}" =~ ^[0-9]+$ || "${!item}" == 0 ]]; then
    echo "$item must be a positive integer" >&2
    exit 1
  fi
done

mkdir -p "$(dirname "$stream")"

cargo run --release -p rav1e --bin rav1e-o3yv -- \
  --input "$input_yuv" \
  --output "$stream" \
  --frames "$frames" \
  --keyint "$keyint" \
  --loopback

tools/o3yv-old3ds-package-run.sh "$stream" "$bundle_dir" \
  "$iterations" "$target_us"
