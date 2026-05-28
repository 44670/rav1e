#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
usage: tools/o3yv-old3ds-readiness.sh [representative.o3yv]

Checks whether the decoder core still builds without Rust std and whether this
machine has enough Old3DS homebrew tooling installed to build and run a real
hardware decoder timing harness. This does not replace hardware timing; it
makes missing prerequisites explicit.

Defaults:
  representative.o3yv   tmp/reencode_lazy128_current.o3yv
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

input=${1:-tmp/reencode_lazy128_current.o3yv}
missing=0
have_nightly=0
have_rust_src=0

check_file() {
  local label=$1
  local path=$2
  if [[ -f "$path" ]]; then
    echo "PASS $label: $path"
  else
    echo "MISS $label: $path" >&2
    missing=1
  fi
}

check_dir() {
  local label=$1
  local path=$2
  if [[ -d "$path" ]]; then
    echo "PASS $label: $path"
  else
    echo "MISS $label: $path" >&2
    missing=1
  fi
}

check_command() {
  local command=$1
  if command -v "$command" >/dev/null 2>&1; then
    echo "PASS command: $command ($(command -v "$command"))"
  else
    echo "MISS command: $command" >&2
    missing=1
  fi
}

check_rust_nightly() {
  if ! command -v rustup >/dev/null 2>&1; then
    echo "MISS command: rustup" >&2
    missing=1
    return
  fi
  echo "PASS command: rustup ($(command -v rustup))"

  if ! command -v rustc >/dev/null 2>&1; then
    echo "MISS command: rustc" >&2
    missing=1
    return
  fi
  echo "PASS command: rustc ($(command -v rustc))"

  if rustup toolchain list | awk '{ print $1 }' \
    | grep -Eq '^nightly($|-)'; then
    echo "PASS Rust nightly toolchain"
    have_nightly=1
  else
    echo "MISS Rust nightly toolchain" >&2
    missing=1
  fi

  if (( have_nightly != 0 )); then
    if rustup component list --toolchain nightly --installed \
      2>/dev/null | awk '{ print $1 }' | grep -Fxq rust-src; then
      echo "PASS nightly rust-src component"
      have_rust_src=1
    else
      echo "MISS nightly rust-src component" >&2
      missing=1
    fi
  else
    echo "MISS nightly rust-src component (nightly missing)" >&2
  fi

  if rustc --print target-list | grep -Fxq armv6k-nintendo-3ds; then
    echo "PASS rustc target descriptor: armv6k-nintendo-3ds"
  else
    echo "MISS rustc target descriptor: armv6k-nintendo-3ds" >&2
    missing=1
  fi
}

check_old3ds_rust_staticlib() {
  local log=/tmp/o3yv-minidecoder-3dsffi-armv6k-build.log
  if cargo +nightly build \
    --manifest-path old3ds/minidecoder-3dsffi/Cargo.toml \
    --release \
    --no-default-features \
    --target armv6k-nintendo-3ds \
    -Zbuild-std=core,alloc \
    --target-dir old3ds/build/readiness-rust-target \
    >"$log" 2>&1; then
    echo "PASS Old3DS Rust staticlib build"
  else
    echo "MISS Old3DS Rust staticlib build; see $log" >&2
    cat "$log" >&2
    missing=1
  fi
}

check_old3ds_ffi_host() {
  local log=/tmp/o3yv-minidecoder-3dsffi-check.log
  if cargo check --manifest-path old3ds/minidecoder-3dsffi/Cargo.toml \
    >"$log" 2>&1; then
    echo "PASS Old3DS Rust FFI host check"
  else
    echo "MISS Old3DS Rust FFI host check; see $log" >&2
    cat "$log" >&2
    missing=1
  fi
}

check_old3ds_host_c() {
  local log=/tmp/o3yv-old3ds-host-c-check.log
  if tools/o3yv-old3ds-host-c-check.sh >"$log" 2>&1; then
    echo "PASS Old3DS harness host C syntax check"
  else
    echo "MISS Old3DS harness host C syntax check; see $log" >&2
    cat "$log" >&2
    missing=1
  fi
}

check_minidecoder_nostd() {
  local log=/tmp/o3yv-minidecoder-nostd-check.log
  if cargo check -p minidecoder --lib --no-default-features \
    >"$log" 2>&1; then
    echo "PASS minidecoder no_std+alloc lib build"
  else
    echo "MISS minidecoder no_std+alloc lib build; see $log" >&2
    cat "$log" >&2
    missing=1
  fi
}

check_minidecoder_alloc_free_decode() {
  local input=$1
  local log=/tmp/o3yv-minidecoder-alloc-check.log
  if cargo run --release -q -p minidecoder --no-default-features \
    --features alloc-check --bin o3yv-alloc-check -- "$input" \
    >"$log" 2>&1; then
    echo "PASS minidecoder reusable-state decode alloc check: $(cat "$log")"
  else
    echo "MISS minidecoder reusable-state decode alloc check; see $log" >&2
    cat "$log" >&2
    missing=1
  fi
}

check_file "representative stream" "$input"
check_file "Old3DS harness C source" "old3ds/source/main.c"
check_file "Old3DS Rust FFI crate" "old3ds/minidecoder-3dsffi/Cargo.toml"
check_file "Old3DS build script" "tools/o3yv-old3ds-build-harness.sh"
check_file "Old3DS devkitPro image fetcher" "tools/o3yv-old3ds-fetch-devkitpro-image.sh"
check_file "Old3DS run bundle helper" "tools/o3yv-old3ds-package-run.sh"
check_file "Old3DS bench log checker" "tools/o3yv-old3ds-check-log.sh"
check_file "Old3DS playback log checker" "tools/o3yv-old3ds-check-playback-log.sh"
check_file "Old3DS expected checksum tool" "tools/o3yv-old3ds-expected-checksum.sh"
check_file "Old3DS bench log verifier" "tools/o3yv-old3ds-verify-log.sh"
check_file "Old3DS host C checker" "tools/o3yv-old3ds-host-c-check.sh"
check_command cargo
if command -v cargo >/dev/null 2>&1; then
  check_minidecoder_nostd
  if [[ -f old3ds/minidecoder-3dsffi/Cargo.toml ]]; then
    check_old3ds_ffi_host
  fi
  if [[ -f "$input" ]]; then
    check_minidecoder_alloc_free_decode "$input"
  fi
fi
check_old3ds_host_c
check_rust_nightly
if (( have_nightly != 0 && have_rust_src != 0 )); then
  check_old3ds_rust_staticlib
fi
check_command arm-none-eabi-gcc
check_command 3dsxtool
check_command make

if [[ -n "${DEVKITPRO:-}" ]]; then
  check_dir DEVKITPRO "$DEVKITPRO"
else
  echo "MISS env: DEVKITPRO" >&2
  missing=1
fi

if [[ -n "${DEVKITARM:-}" ]]; then
  check_dir DEVKITARM "$DEVKITARM"
else
  echo "MISS env: DEVKITARM" >&2
  missing=1
fi

if [[ -n "${DEVKITPRO:-}" ]]; then
  check_file "libctru 3ds.h" "$DEVKITPRO/libctru/include/3ds.h"
fi

if (( missing != 0 )); then
  cat >&2 <<'EOF'

Old3DS hardware timing is not ready on this machine. Install devkitPro with
devkitARM, libctru, 3dsxtool, Rust nightly, and nightly rust-src.
Then run:

  tools/o3yv-old3ds-build-harness.sh

and time the generated old3ds/build/o3yvbench.3dsx on actual Old3DS hardware.
EOF
  exit 1
fi

echo "Old3DS homebrew toolchain prerequisites are present."
echo "Build the hardware timing harness with tools/o3yv-old3ds-build-harness.sh"
