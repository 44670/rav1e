#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
usage: tools/o3yv-old3ds-host-c-check.sh

Syntax-checks old3ds/source/main.c on the host with a small stub 3ds.h.
This does not replace a real devkitPro/libctru build; it catches harness C
errors before the final .3dsx packaging step is available.
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

cc=${CC:-cc}
if ! command -v "$cc" >/dev/null 2>&1; then
  echo "missing C compiler: $cc" >&2
  exit 1
fi

tmpd=$(mktemp -d /tmp/o3yv-old3ds-c-check-XXXXXX)
trap 'rm -rf "$tmpd"' EXIT

cat >"$tmpd/3ds.h" <<'HEADER'
#pragma once

#include <stddef.h>
#include <stdint.h>
#include <stdlib.h>

typedef uint8_t u8;
typedef uint32_t u32;
typedef uint64_t u64;

#define GFX_TOP 0
#define KEY_START 0x00000008u
#define SYSCLOCK_ARM11 268123480u

static inline void gfxInitDefault(void) {}
static inline void gfxExit(void) {}
static inline void consoleInit(int screen, void *console) {
  (void)screen;
  (void)console;
}
static inline void *linearMemAlign(size_t size, size_t alignment) {
  (void)alignment;
  return malloc(size);
}
static inline void *linearAlloc(size_t size) {
  return malloc(size);
}
static inline void linearFree(void *ptr) {
  free(ptr);
}
static inline u64 svcGetSystemTick(void) {
  return 0;
}
static inline int aptMainLoop(void) {
  return 0;
}
static inline void hidScanInput(void) {}
static inline u32 hidKeysDown(void) {
  return KEY_START;
}
static inline void gfxFlushBuffers(void) {}
static inline void gfxSwapBuffers(void) {}
static inline void gspWaitForVBlank(void) {}
HEADER

cat >"$tmpd/o3yv_stream.h" <<'HEADER'
#pragma once

#include <stddef.h>
#include <stdint.h>

extern const uint8_t O3YV_STREAM[];
extern const uint8_t O3YV_STREAM_END[];

#define O3YV_STREAM_LEN ((size_t)(O3YV_STREAM_END - O3YV_STREAM))
HEADER

"$cc" \
  -std=c11 \
  -Wall \
  -Wextra \
  -Werror \
  -D__3DS__ \
  -I"$tmpd" \
  -Iold3ds/include \
  -fsyntax-only \
  old3ds/source/main.c

echo "PASS Old3DS harness host C syntax check"
