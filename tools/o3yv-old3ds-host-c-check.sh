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
#include <stdbool.h>
#include <stdlib.h>

typedef uint8_t u8;
typedef uint32_t u32;
typedef uint64_t u64;
typedef int64_t s64;
typedef int gfx3dSide_t;
typedef int32_t Result;
typedef int Handle;
typedef int Y2RU_InputFormat;
typedef int Y2RU_OutputFormat;
typedef int Y2RU_Rotation;
typedef int Y2RU_BlockAlignment;
typedef int Y2RU_StandardCoefficient;

typedef struct {
  Y2RU_InputFormat input_format;
  Y2RU_OutputFormat output_format;
  Y2RU_Rotation rotation;
  Y2RU_BlockAlignment block_alignment;
  int16_t input_line_width;
  int16_t input_lines;
  Y2RU_StandardCoefficient standard_coefficient;
  uint8_t unused;
  uint16_t alpha;
} Y2RU_ConversionParams;

#define GFX_TOP 0
#define GFX_BOTTOM 1
#define GFX_LEFT 0
#define GFX_RIGHT 1
#define KEY_START 0x00000008u
#define SYSCLOCK_ARM11 268123480u
#define INPUT_YUV420_INDIV_8 1
#define OUTPUT_RGB_24 1
#define ROTATION_CLOCKWISE_90 1
#define BLOCK_LINE 0
#define COEFFICIENT_ITU_R_BT_709_SCALING 3
#define R_SUCCEEDED(rc) ((rc) >= 0)

static inline void gfxInitDefault(void) {}
static inline void gfxSet3D(int enable) {
  (void)enable;
}
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
static inline void svcSleepThread(s64 ns) {
  (void)ns;
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
static inline u8 *gfxGetFramebuffer(
    int screen, gfx3dSide_t side, void *width, void *height) {
  static u8 fb[400 * 240 * 3];
  (void)screen;
  (void)side;
  (void)width;
  (void)height;
  return fb;
}
static inline Result y2rInit(void) {
  return -1;
}
static inline void y2rExit(void) {}
static inline Result Y2RU_SetConversionParams(
    const Y2RU_ConversionParams *params) {
  (void)params;
  return 0;
}
static inline Result Y2RU_SetTransferEndInterrupt(int should_interrupt) {
  (void)should_interrupt;
  return 0;
}
static inline Result Y2RU_GetTransferEndEvent(Handle *end_event) {
  *end_event = 0;
  return 0;
}
static inline Result svcWaitSynchronization(Handle handle, s64 ns) {
  (void)handle;
  (void)ns;
  return 0;
}
static inline Result Y2RU_IsBusyConversion(bool *busy) {
  *busy = 0;
  return 0;
}
static inline Result GSPGPU_FlushDataCache(void *address, u32 size) {
  (void)address;
  (void)size;
  return 0;
}
static inline Result Y2RU_SetSendingY(
    const void *src, u32 image_size, s64 transfer_unit, s64 transfer_gap) {
  (void)src;
  (void)image_size;
  (void)transfer_unit;
  (void)transfer_gap;
  return 0;
}
static inline Result Y2RU_SetSendingU(
    const void *src, u32 image_size, s64 transfer_unit, s64 transfer_gap) {
  (void)src;
  (void)image_size;
  (void)transfer_unit;
  (void)transfer_gap;
  return 0;
}
static inline Result Y2RU_SetSendingV(
    const void *src, u32 image_size, s64 transfer_unit, s64 transfer_gap) {
  (void)src;
  (void)image_size;
  (void)transfer_unit;
  (void)transfer_gap;
  return 0;
}
static inline Result Y2RU_SetReceiving(
    void *dst, u32 image_size, s64 transfer_unit, s64 transfer_gap) {
  (void)dst;
  (void)image_size;
  (void)transfer_unit;
  (void)transfer_gap;
  return 0;
}
static inline Result Y2RU_StartConversion(void) {
  return 0;
}
HEADER

cat >"$tmpd/o3yv_stream.h" <<'HEADER'
#pragma once

#include <stddef.h>
#include <stdint.h>

extern const uint8_t O3YV_STREAM[];
extern const uint8_t O3YV_STREAM_END[];

#define O3YV_STREAM_LEN ((size_t)(O3YV_STREAM_END - O3YV_STREAM))
#define O3YV_BENCH_ITERATIONS 8
#define O3YV_EXPECTED_FRAMES_PER_ITERATION 100u
#define O3YV_EXPECTED_CHECKSUM 0x2bf2aba9994f6b15ULL
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
