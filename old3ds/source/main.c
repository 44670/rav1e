#include <3ds.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "minidecoder_3dsffi.h"
#include "o3yv_stream.h"

#define ITERATIONS 8
#define TARGET_US 15000ULL
#define LOG_PATH "sdmc:/o3yvbench.log"

static FILE *g_log_file;

static void bench_log(const char *fmt, ...) {
  char buffer[512];
  va_list args;
  va_start(args, fmt);
  vsnprintf(buffer, sizeof(buffer), fmt, args);
  va_end(args);

  fputs(buffer, stdout);
  if (g_log_file) {
    fputs(buffer, g_log_file);
    fflush(g_log_file);
  }
}

static u64 ticks_to_us(u64 ticks) {
  return (ticks * 1000000ULL) / (u64)SYSCLOCK_ARM11;
}

static void print_us_as_ms(const char *label, u64 us) {
  bench_log("%s: %llu.%03llu\n",
      label,
      (unsigned long long)(us / 1000ULL),
      (unsigned long long)(us % 1000ULL));
}

int main(int argc, char **argv) {
  (void)argc;
  (void)argv;

  gfxInitDefault();
  consoleInit(GFX_TOP, NULL);
  g_log_file = fopen(LOG_PATH, "w");

  bench_log("O3YV Old3DS decoder bench\n");
  bench_log("stream bytes: %lu\n", (unsigned long)O3YV_STREAM_LEN);
  if (g_log_file) {
    bench_log("log_path: %s\n", LOG_PATH);
  } else {
    bench_log("log_path: unavailable\n");
  }

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
    bench_log("allocation failed\n");
    goto wait_exit;
  }
  memset(decoder, 0, decoder_size);

  int rc = o3yv_decoder_init(
      decoder, decoder_size, O3YV_STREAM, O3YV_STREAM_LEN);
  if (rc != 0) {
    bench_log("init failed: %ld\n", (long)rc);
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
      bench_log("reset failed: %ld\n", (long)rc);
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
        bench_log("decode failed: %ld\n", (long)rc);
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
      bench_log("frame count changed: first=%lu iter_%d=%lu\n",
          (unsigned long)frames_per_iteration,
          iter,
          (unsigned long)iter_frames);
      goto wait_exit;
    }
  }

  if (total_frames == 0 || frames_per_iteration == 0) {
    bench_log("no frames decoded\n");
    goto wait_exit;
  }

  const u64 mean_us = ticks_to_us(total_ticks) / (u64)total_frames;
  const u64 worst_us = ticks_to_us(worst_ticks);
  const int pass = worst_us <= TARGET_US;

  bench_log("iterations: %d\n", ITERATIONS);
  bench_log("frames: %lu\n", (unsigned long)total_frames);
  bench_log(
      "frames_per_iteration: %lu\n", (unsigned long)frames_per_iteration);
  print_us_as_ms("mean_ms_per_frame", mean_us);
  print_us_as_ms("worst_frame_ms", worst_us);
  print_us_as_ms("target_worst_ms", TARGET_US);
  bench_log("worst_iter: %lu\n", (unsigned long)worst_iter);
  bench_log("worst_frame_no: %lu\n", (unsigned long)worst_frame_no);
  bench_log("worst_frame_type: %u\n", (unsigned)worst_frame_type);
  bench_log("bench_result status=%s iterations=%d frames=%lu "
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
  bench_log("%s\n", pass ? "PASS" : "FAIL");

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
  if (g_log_file) {
    fclose(g_log_file);
    g_log_file = NULL;
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
