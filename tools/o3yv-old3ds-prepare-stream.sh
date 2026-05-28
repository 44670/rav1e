#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
usage: tools/o3yv-old3ds-prepare-stream.sh [input.o3yv] [output.h]

Generates the C header embedded by the Old3DS timing harness.

Defaults:
  input.o3yv  tmp/reencode_lazy128_current.o3yv
  output.h    old3ds/generated/o3yv_stream.h
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

input=${1:-tmp/reencode_lazy128_current.o3yv}
output=${2:-old3ds/generated/o3yv_stream.h}

if [[ ! -f "$input" ]]; then
  echo "missing input stream: $input" >&2
  exit 1
fi

mkdir -p "$(dirname "$output")"
{
  cat <<'HEADER'
#pragma once

#include <stdint.h>

static const uint8_t O3YV_STREAM[] = {
HEADER
  od -An -v -tx1 "$input" | awk '
    {
      printf "  "
      for (i = 1; i <= NF; i++) {
        printf "0x%s,", $i
        if (i < NF) {
          printf " "
        }
      }
      printf "\n"
    }
  '
  cat <<'FOOTER'
};

static const uint32_t O3YV_STREAM_LEN = sizeof(O3YV_STREAM);
FOOTER
} >"$output"

echo "wrote $output"
