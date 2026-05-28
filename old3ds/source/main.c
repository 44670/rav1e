#include <3ds.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "minidecoder_3dsffi.h"
#include "o3yv_stream.h"

#define ITERATIONS 8
#define TARGET_US 15000ULL

static u64 ticks_to_us(u64 ticks) {
  return (ticks * 1000000ULL) / (u64)SYSCLOCK_ARM11;
}

static void print_us_as_ms(const char *label, u64 us) {
  printf("%s: %llu.%03llu\n",
      label,
      (unsigned long long)(us / 1000ULL),
      (unsigned long long)(us % 1000ULL));
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
  u32 frames_per_iteration = 0;
  u64 total_ticks = 0;
  u64 worst_ticks = 0;
  u32 worst_iter = 0;
  u32 worst_frame_no = 0;
  u8 worst_frame_type = 0;
  O3yvFrameInfo info;

  for (int iter = 0; iter < ITERATIONS; iter++) {
    rc = o3yv_decoder_reset(decoder);
    if (rc != 0) {
      printf("reset failed: %ld\n", (long)rc);
      goto wait_exit;
    }

    u32 iter_frames = 0;
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
      iter_frames++;
      total_ticks += elapsed;
      if (elapsed > worst_ticks) {
        worst_ticks = elapsed;
        worst_iter = (u32)iter;
        worst_frame_no = info.frame_no;
        worst_frame_type = info.frame_type;
      }
    }

    if (iter == 0) {
      frames_per_iteration = iter_frames;
    } else if (iter_frames != frames_per_iteration) {
      printf("frame count changed: first=%lu iter_%d=%lu\n",
          (unsigned long)frames_per_iteration,
          iter,
          (unsigned long)iter_frames);
      goto wait_exit;
    }
  }

  if (total_frames == 0 || frames_per_iteration == 0) {
    printf("no frames decoded\n");
    goto wait_exit;
  }

  const u64 mean_us = ticks_to_us(total_ticks) / (u64)total_frames;
  const u64 worst_us = ticks_to_us(worst_ticks);
  const int pass = worst_us <= TARGET_US;

  printf("iterations: %d\n", ITERATIONS);
  printf("frames: %lu\n", (unsigned long)total_frames);
  printf("frames_per_iteration: %lu\n", (unsigned long)frames_per_iteration);
  print_us_as_ms("mean_ms_per_frame", mean_us);
  print_us_as_ms("worst_frame_ms", worst_us);
  print_us_as_ms("target_worst_ms", TARGET_US);
  printf("worst_iter: %lu\n", (unsigned long)worst_iter);
  printf("worst_frame_no: %lu\n", (unsigned long)worst_frame_no);
  printf("worst_frame_type: %u\n", (unsigned)worst_frame_type);
  printf("bench_result status=%s iterations=%d frames=%lu "
         "frames_per_iteration=%lu mean_us=%llu worst_us=%llu "
         "target_us=%llu worst_iter=%lu worst_frame_no=%lu "
         "worst_frame_type=%u\n",
      pass ? "pass" : "fail",
      ITERATIONS,
      (unsigned long)total_frames,
      (unsigned long)frames_per_iteration,
      (unsigned long long)mean_us,
      (unsigned long long)worst_us,
      (unsigned long long)TARGET_US,
      (unsigned long)worst_iter,
      (unsigned long)worst_frame_no,
      (unsigned)worst_frame_type);
  printf("%s\n", pass ? "PASS" : "FAIL");

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
