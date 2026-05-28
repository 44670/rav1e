#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
usage: tools/o3yv-old3ds-build-harness.sh [representative.o3yv]

Builds the Old3DS hardware timing harness. This requires devkitPro/devkitARM,
3dsxtool, Rust nightly, and the rust-src component.

Defaults:
  representative.o3yv   tmp/reencode_lazy128_current.o3yv
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

input=${1:-tmp/reencode_lazy128_current.o3yv}
target=armv6k-nintendo-3ds
rust_crate=old3ds/minidecoder-3dsffi/Cargo.toml
rust_lib=old3ds/build/rust/libminidecoder_3dsffi.a

require_command() {
  local command=$1
  if ! command -v "$command" >/dev/null 2>&1; then
    echo "missing command: $command" >&2
    exit 1
  fi
}

if [[ ! -f "$input" ]]; then
  echo "missing input stream: $input" >&2
  exit 1
fi
if [[ -z "${DEVKITPRO:-}" || -z "${DEVKITARM:-}" ]]; then
  echo "DEVKITPRO and DEVKITARM must be set" >&2
  exit 1
fi

require_command cargo
require_command rustup
require_command arm-none-eabi-gcc
require_command 3dsxtool
require_command make

if ! rustup toolchain list | awk '{ print $1 }' \
  | grep -Eq '^nightly($|-)'; then
  echo "missing Rust nightly toolchain" >&2
  echo "run: rustup toolchain install nightly" >&2
  exit 1
fi
if ! rustup component list --toolchain nightly --installed \
  | awk '{ print $1 }' | grep -Fxq rust-src; then
  echo "missing rust-src for nightly" >&2
  echo "run: rustup component add rust-src --toolchain nightly" >&2
  exit 1
fi
if ! rustc +nightly --print target-list | grep -Fxq "$target"; then
  echo "nightly rustc does not know target: $target" >&2
  exit 1
fi

tools/o3yv-old3ds-prepare-stream.sh "$input" old3ds/generated 8
mkdir -p old3ds/build/rust

cargo +nightly build \
  --manifest-path "$rust_crate" \
  --release \
  --no-default-features \
  --target "$target" \
  -Zbuild-std=core,alloc \
  --target-dir old3ds/build/rust-target

cp \
  "old3ds/build/rust-target/${target}/release/libminidecoder_3dsffi.a" \
  "$rust_lib"

make -C old3ds RUST_LIB="$(pwd)/$rust_lib"

echo "built old3ds/build/o3yvbench.3dsx"
