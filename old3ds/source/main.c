#include <3ds.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "minidecoder_3dsffi.h"
#include "o3yv_stream.h"

#define ITERATIONS 8
#define TARGET_MS 15.0

static double ticks_to_ms(u64 ticks) {
  return ((double)ticks * 1000.0) / (double)SYSCLOCK_ARM11;
}

int main(int argc, char **argv) {
  (void)argc;
  (void)argv;

  gfxInitDefault();
  consoleInit(GFX_TOP, NULL);

  printf("O3YV Old3DS decoder bench\n");
  printf("stream bytes: %lu\n", (unsigned long)O3YV_STREAM_LEN);

  const size_t decoder_size = o3yv_decoder_size();
  const size_t decoder_align = o3yv_decoder_align();
  const size_t eye_bytes = o3yv_eye_frame_bytes();
  const size_t decoder_alloc_align =
      decoder_align < 0x80 ? 0x80 : decoder_align;

  void *decoder = linearMemAlign(decoder_size, decoder_alloc_align);
  u8 *left = linearAlloc(eye_bytes);
  u8 *right = linearAlloc(eye_bytes);
  int decoder_initialized = 0;
  if (!decoder || !left || !right) {
    printf("allocation failed\n");
    goto wait_exit;
  }
  memset(decoder, 0, decoder_size);

  int rc = o3yv_decoder_init(
      decoder, decoder_size, O3YV_STREAM, O3YV_STREAM_LEN);
  if (rc != 0) {
    printf("init failed: %ld\n", (long)rc);
    goto wait_exit;
  }
  decoder_initialized = 1;

  u32 total_frames = 0;
  u64 total_ticks = 0;
  u64 worst_ticks = 0;
  O3yvFrameInfo info;

  for (int iter = 0; iter < ITERATIONS; iter++) {
    rc = o3yv_decoder_reset(decoder);
    if (rc != 0) {
      printf("reset failed: %ld\n", (long)rc);
      goto wait_exit;
    }

    for (;;) {
      const u64 start = svcGetSystemTick();
      rc = o3yv_decoder_next_frame_yuv420p(
          decoder, left, eye_bytes, right, eye_bytes, &info);
      const u64 elapsed = svcGetSystemTick() - start;

      if (rc == 0) {
        break;
      }
      if (rc < 0) {
        printf("decode failed: %ld\n", (long)rc);
        goto wait_exit;
      }

      total_frames++;
      total_ticks += elapsed;
      if (elapsed > worst_ticks) {
        worst_ticks = elapsed;
      }
    }
  }

  if (total_frames == 0) {
    printf("no frames decoded\n");
    goto wait_exit;
  }

  const double mean_ms = ticks_to_ms(total_ticks) / (double)total_frames;
  const double worst_ms = ticks_to_ms(worst_ticks);
  printf("iterations: %d\n", ITERATIONS);
  printf("frames: %lu\n", (unsigned long)total_frames);
  printf("mean_ms_per_frame: %.3f\n", mean_ms);
  printf("worst_frame_ms: %.3f\n", worst_ms);
  printf("target_worst_ms: %.3f\n", TARGET_MS);
  printf("%s\n", worst_ms <= TARGET_MS ? "PASS" : "FAIL");

wait_exit:
  if (decoder && decoder_initialized) {
    o3yv_decoder_drop(decoder);
  }
  if (decoder) {
    linearFree(decoder);
  }
  if (left) {
    linearFree(left);
  }
  if (right) {
    linearFree(right);
  }

  printf("Press START to exit.\n");
  while (aptMainLoop()) {
    hidScanInput();
    if (hidKeysDown() & KEY_START) {
      break;
    }
    gfxFlushBuffers();
    gfxSwapBuffers();
    gspWaitForVBlank();
  }

  gfxExit();
  return 0;
}
