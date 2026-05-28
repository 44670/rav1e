#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
usage: tools/o3yv-old3ds-prepare-stream.sh [input.o3yv] [output-dir]

Generates the tiny C header and assembly incbin source used by the Old3DS
timing harness.

Defaults:
  input.o3yv  tmp/reencode_lazy128_current.o3yv
  output-dir  old3ds/generated
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

input=${1:-tmp/reencode_lazy128_current.o3yv}
output_dir=${2:-old3ds/generated}
header=${output_dir}/o3yv_stream.h
asm=${output_dir}/o3yv_stream.s

if [[ ! -f "$input" ]]; then
  echo "missing input stream: $input" >&2
  exit 1
fi

mkdir -p "$output_dir"
input_abs=$(realpath "$input")

cat >"$header" <<'HEADER'
#pragma once

#include <stddef.h>
#include <stdint.h>

extern const uint8_t O3YV_STREAM[];
extern const uint8_t O3YV_STREAM_END[];

#define O3YV_STREAM_LEN ((size_t)(O3YV_STREAM_END - O3YV_STREAM))
HEADER

cat >"$asm" <<ASM
  .section .rodata.o3yv_stream, "a", %progbits
  .global O3YV_STREAM
  .global O3YV_STREAM_END
  .balign 4
O3YV_STREAM:
  .incbin "$input_abs"
O3YV_STREAM_END:
ASM

echo "wrote $header"
echo "wrote $asm"
