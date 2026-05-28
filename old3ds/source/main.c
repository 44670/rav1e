#include <3ds.h>
#include <stdbool.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "minidecoder_3dsffi.h"
#include "o3yv_stream.h"

#define MAX_BENCH_SAMPLES 4096
#define MAX_BENCH_FRAMES 512
#define TOP_FRAME_REPORT_COUNT 10
#define LOG_PATH "sdmc:/o3yvbench.log"
#define EYE_W 400
#define EYE_H 240
#define CHROMA_W 200
#define CHROMA_H 120
#define PLAYBACK_TARGET_FPS 24ULL
#define PLAYBACK_TARGET_US (1000000ULL / PLAYBACK_TARGET_FPS)
#define RGB24_FRAME_BYTES (EYE_W * EYE_H * 3)
#define Y2R_TIMEOUT_NS 1000000000LL

#ifndef O3YV_BENCH_ITERATIONS
#define O3YV_BENCH_ITERATIONS 8
#endif

#ifndef O3YV_TARGET_US
#define O3YV_TARGET_US 15000ULL
#endif

static FILE *g_log_file;
static u64 g_bench_samples[MAX_BENCH_SAMPLES];
static u64 g_frame_total_ticks[MAX_BENCH_FRAMES];
static u64 g_frame_min_ticks[MAX_BENCH_FRAMES];
static u64 g_frame_max_ticks[MAX_BENCH_FRAMES];
static u64 g_frame_decode_total_ticks[MAX_BENCH_FRAMES];
static u64 g_frame_decode_max_ticks[MAX_BENCH_FRAMES];
static u64 g_frame_output_total_ticks[MAX_BENCH_FRAMES];
static u64 g_frame_output_max_ticks[MAX_BENCH_FRAMES];
static u32 g_frame_sample_count[MAX_BENCH_FRAMES];
static u32 g_frame_no[MAX_BENCH_FRAMES];
static u8 g_frame_type[MAX_BENCH_FRAMES];
static u8 g_frame_ranked[MAX_BENCH_FRAMES];
static int g_y2r_initialized;
static int g_y2r_available;
static int g_y2r_warned;

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

static u64 us_to_ticks(u64 us) {
  return (us * (u64)SYSCLOCK_ARM11) / 1000000ULL;
}

static void print_us_as_ms(const char *label, u64 us) {
  bench_log("%s: %llu.%03llu\n",
      label,
      (unsigned long long)(us / 1000ULL),
      (unsigned long long)(us % 1000ULL));
}

static int compare_u64(const void *a, const void *b) {
  const u64 av = *(const u64 *)a;
  const u64 bv = *(const u64 *)b;
  if (av < bv) {
    return -1;
  }
  if (av > bv) {
    return 1;
  }
  return 0;
}

static u64 percentile_ticks(const u64 *samples, u32 sample_count, u32 pct) {
  const u32 rank = ((pct * sample_count) + 99) / 100;
  return samples[rank == 0 ? 0 : rank - 1];
}

static void update_frame_timing(
    u32 index, u32 frame_no, u8 frame_type, u64 total_elapsed,
    u64 decode_elapsed, u64 output_elapsed) {
  if (g_frame_sample_count[index] == 0) {
    g_frame_no[index] = frame_no;
    g_frame_type[index] = frame_type;
    g_frame_min_ticks[index] = total_elapsed;
    g_frame_max_ticks[index] = total_elapsed;
    g_frame_decode_max_ticks[index] = decode_elapsed;
    g_frame_output_max_ticks[index] = output_elapsed;
  } else {
    if (total_elapsed < g_frame_min_ticks[index]) {
      g_frame_min_ticks[index] = total_elapsed;
    }
    if (total_elapsed > g_frame_max_ticks[index]) {
      g_frame_max_ticks[index] = total_elapsed;
    }
    if (decode_elapsed > g_frame_decode_max_ticks[index]) {
      g_frame_decode_max_ticks[index] = decode_elapsed;
    }
    if (output_elapsed > g_frame_output_max_ticks[index]) {
      g_frame_output_max_ticks[index] = output_elapsed;
    }
  }
  g_frame_total_ticks[index] += total_elapsed;
  g_frame_decode_total_ticks[index] += decode_elapsed;
  g_frame_output_total_ticks[index] += output_elapsed;
  g_frame_sample_count[index]++;
}

static void print_top_frame_timings(u32 frame_count) {
  const u32 report_count =
      frame_count < TOP_FRAME_REPORT_COUNT ? frame_count : TOP_FRAME_REPORT_COUNT;
  memset(g_frame_ranked, 0, sizeof(g_frame_ranked));
  bench_log("frame_timing_top_by_max count=%lu\n", (unsigned long)report_count);

  for (u32 rank = 0; rank < report_count; rank++) {
    u32 best = MAX_BENCH_FRAMES;
    for (u32 index = 0; index < frame_count; index++) {
      if (g_frame_ranked[index] || g_frame_sample_count[index] == 0) {
        continue;
      }
      if (best == MAX_BENCH_FRAMES
          || g_frame_max_ticks[index] > g_frame_max_ticks[best]
          || (g_frame_max_ticks[index] == g_frame_max_ticks[best]
              && g_frame_total_ticks[index] > g_frame_total_ticks[best])) {
        best = index;
      }
    }
    if (best == MAX_BENCH_FRAMES) {
      break;
    }

    g_frame_ranked[best] = 1;
    const u64 sample_count = (u64)g_frame_sample_count[best];
    const u64 mean_us =
        ticks_to_us(g_frame_total_ticks[best]) / sample_count;
    const u64 min_us = ticks_to_us(g_frame_min_ticks[best]);
    const u64 max_us = ticks_to_us(g_frame_max_ticks[best]);
    const u64 decode_mean_us =
        ticks_to_us(g_frame_decode_total_ticks[best]) / sample_count;
    const u64 decode_max_us = ticks_to_us(g_frame_decode_max_ticks[best]);
    const u64 output_mean_us =
        ticks_to_us(g_frame_output_total_ticks[best]) / sample_count;
    const u64 output_max_us = ticks_to_us(g_frame_output_max_ticks[best]);
    bench_log("frame_timing rank=%lu index=%lu no=%lu type=%u "
              "samples=%lu min_us=%llu mean_us=%llu max_us=%llu "
              "decode_mean_us=%llu decode_max_us=%llu "
              "output_mean_us=%llu output_max_us=%llu\n",
        (unsigned long)rank,
        (unsigned long)best,
        (unsigned long)g_frame_no[best],
        (unsigned)g_frame_type[best],
        (unsigned long)g_frame_sample_count[best],
        (unsigned long long)min_us,
        (unsigned long long)mean_us,
        (unsigned long long)max_us,
        (unsigned long long)decode_mean_us,
        (unsigned long long)decode_max_us,
        (unsigned long long)output_mean_us,
        (unsigned long long)output_max_us);
  }
}

static void checksum_update_byte(u64 *state, u8 byte) {
  *state ^= (u64)byte;
  *state *= 1099511628211ULL;
}

static void checksum_update_u32(u64 *state, u32 value) {
  checksum_update_byte(state, (u8)(value & 0xff));
  checksum_update_byte(state, (u8)((value >> 8) & 0xff));
  checksum_update_byte(state, (u8)((value >> 16) & 0xff));
  checksum_update_byte(state, (u8)((value >> 24) & 0xff));
}

static void checksum_update_bytes(u64 *state, const u8 *bytes, size_t len) {
  for (size_t i = 0; i < len; i++) {
    checksum_update_byte(state, bytes[i]);
  }
}

static void checksum_update_frame(
    u64 *state, u32 frame_no, u8 frame_type, const u8 *left,
    const u8 *right, size_t eye_bytes) {
  checksum_update_u32(state, frame_no);
  checksum_update_byte(state, frame_type);
  checksum_update_bytes(state, left, eye_bytes);
  checksum_update_bytes(state, right, eye_bytes);
}

static u8 clip_i32_to_u8(int value) {
  if (value < 0) {
    return 0;
  }
  if (value > 255) {
    return 255;
  }
  return (u8)value;
}

static void put_yuv_pixel_bgr8(
    u8 *fb, int x, int y, u8 y_sample, int cb, int cr) {
  int yy = (int)y_sample - 16;
  if (yy < 0) {
    yy = 0;
  }

  const int r = (19077 * yy + 29372 * cr + 8192) >> 14;
  const int g = (19077 * yy - 3494 * cb - 8739 * cr + 8192) >> 14;
  const int b = (19077 * yy + 34610 * cb + 8192) >> 14;
  const u32 dst = (u32)((EYE_H - 1 - y) + x * EYE_H) * 3u;
  fb[dst] = clip_i32_to_u8(b);
  fb[dst + 1] = clip_i32_to_u8(g);
  fb[dst + 2] = clip_i32_to_u8(r);
}

static void render_eye_bgr8(const u8 *eye, gfx3dSide_t side) {
  u8 *fb = gfxGetFramebuffer(GFX_TOP, side, NULL, NULL);
  if (!fb) {
    return;
  }

  const u8 *y_plane = eye;
  const u8 *cb_plane = y_plane + EYE_W * EYE_H;
  const u8 *cr_plane = cb_plane + CHROMA_W * CHROMA_H;

  for (int y = 0; y < EYE_H; y += 2) {
    const int y_next = y + 1;
    const u8 *y_row0 = y_plane + y * EYE_W;
    const u8 *y_row1 = y_plane + y_next * EYE_W;
    const u8 *cb_row = cb_plane + (y / 2) * CHROMA_W;
    const u8 *cr_row = cr_plane + (y / 2) * CHROMA_W;

    for (int x = 0; x < EYE_W; x += 2) {
      const int cx = x / 2;
      const int cb = (int)cb_row[cx] - 128;
      const int cr = (int)cr_row[cx] - 128;
      put_yuv_pixel_bgr8(fb, x, y, y_row0[x], cb, cr);
      put_yuv_pixel_bgr8(fb, x + 1, y, y_row0[x + 1], cb, cr);
      put_yuv_pixel_bgr8(fb, x, y_next, y_row1[x], cb, cr);
      put_yuv_pixel_bgr8(fb, x + 1, y_next, y_row1[x + 1], cb, cr);
    }
  }
}

static int y2r_result_ok(Result rc) {
  return R_SUCCEEDED(rc);
}

static int init_y2r_playback(void) {
  Result rc = y2rInit();
  if (!y2r_result_ok(rc)) {
    bench_log("playback_y2r: init unavailable rc=0x%08lx\n", (unsigned long)rc);
    return 0;
  }

  const Y2RU_ConversionParams params = {
      .input_format = INPUT_YUV420_INDIV_8,
      .output_format = OUTPUT_RGB_24,
      .rotation = ROTATION_CLOCKWISE_90,
      .block_alignment = BLOCK_LINE,
      .input_line_width = EYE_W,
      .input_lines = EYE_H,
      .standard_coefficient = COEFFICIENT_ITU_R_BT_709_SCALING,
      .unused = 0,
      .alpha = 0xff,
  };

  rc = Y2RU_SetConversionParams(&params);
  if (!y2r_result_ok(rc)) {
    bench_log("playback_y2r: params failed rc=0x%08lx\n", (unsigned long)rc);
    y2rExit();
    return 0;
  }
  rc = Y2RU_SetTransferEndInterrupt(true);
  if (!y2r_result_ok(rc)) {
    bench_log(
        "playback_y2r: interrupt failed rc=0x%08lx\n", (unsigned long)rc);
    y2rExit();
    return 0;
  }

  g_y2r_initialized = 1;
  bench_log("playback_y2r: enabled\n");
  return 1;
}

static Result wait_y2r_done(void) {
  Handle end_event = 0;
  Result rc = Y2RU_GetTransferEndEvent(&end_event);
  if (y2r_result_ok(rc) && end_event) {
    return svcWaitSynchronization(end_event, Y2R_TIMEOUT_NS);
  }

  for (int i = 0; i < 10000; i++) {
    bool busy = true;
    rc = Y2RU_IsBusyConversion(&busy);
    if (!y2r_result_ok(rc)) {
      return rc;
    }
    if (!busy) {
      return 0;
    }
    svcSleepThread(100000LL);
  }
  return -1;
}

static int render_eye_y2r(const u8 *eye, gfx3dSide_t side) {
  u8 *fb = gfxGetFramebuffer(GFX_TOP, side, NULL, NULL);
  if (!fb) {
    return -1;
  }

  const u8 *y_plane = eye;
  const u8 *cb_plane = y_plane + EYE_W * EYE_H;
  const u8 *cr_plane = cb_plane + CHROMA_W * CHROMA_H;

  GSPGPU_FlushDataCache((void *)eye, (u32)o3yv_eye_frame_bytes());
  GSPGPU_FlushDataCache(fb, RGB24_FRAME_BYTES);

  Result rc = Y2RU_SetSendingY(
      y_plane, EYE_W * EYE_H, EYE_W * 8, 0);
  if (!y2r_result_ok(rc)) {
    return (int)rc;
  }
  rc = Y2RU_SetSendingU(
      cb_plane, CHROMA_W * CHROMA_H, CHROMA_W * 4, 0);
  if (!y2r_result_ok(rc)) {
    return (int)rc;
  }
  rc = Y2RU_SetSendingV(
      cr_plane, CHROMA_W * CHROMA_H, CHROMA_W * 4, 0);
  if (!y2r_result_ok(rc)) {
    return (int)rc;
  }
  rc = Y2RU_SetReceiving(
      fb, RGB24_FRAME_BYTES, EYE_H * 3 * 8, 0);
  if (!y2r_result_ok(rc)) {
    return (int)rc;
  }
  rc = Y2RU_StartConversion();
  if (!y2r_result_ok(rc)) {
    return (int)rc;
  }
  rc = wait_y2r_done();
  if (!y2r_result_ok(rc)) {
    return (int)rc;
  }
  return 0;
}

static const char *playback_renderer_name(void) {
  return g_y2r_available ? "y2r" : "software_bgr8";
}

static void render_frame_yuv420p(const u8 *left, const u8 *right) {
  if (g_y2r_available) {
    const int left_rc = render_eye_y2r(left, GFX_LEFT);
    const int right_rc = left_rc == 0 ? render_eye_y2r(right, GFX_RIGHT) : 0;
    if (left_rc == 0 && right_rc == 0) {
      gfxSwapBuffers();
      return;
    }

    if (!g_y2r_warned) {
      bench_log("playback_y2r: render failed left=%ld right=%ld; "
                "falling back to software\n",
          (long)left_rc,
          (long)right_rc);
      g_y2r_warned = 1;
    }
    g_y2r_available = 0;
  }

  render_eye_bgr8(left, GFX_LEFT);
  render_eye_bgr8(right, GFX_RIGHT);
  gfxFlushBuffers();
  gfxSwapBuffers();
}

static void pace_frame(u64 frame_start_ticks) {
  const u64 target_ticks = us_to_ticks(PLAYBACK_TARGET_US);
  const u64 elapsed = svcGetSystemTick() - frame_start_ticks;
  if (elapsed >= target_ticks) {
    return;
  }

  const u64 remain_us = ticks_to_us(target_ticks - elapsed);
  if (remain_us > 1000ULL) {
    svcSleepThread((s64)((remain_us - 500ULL) * 1000ULL));
  }
  gspWaitForVBlank();
}

static int decode_render_frame(
    void *decoder, u8 *left, u8 *right, size_t eye_bytes,
    u64 *decode_ticks, u64 *output_ticks, u64 *render_ticks) {
  O3yvFrameInfo info;
  const u64 start = svcGetSystemTick();
  int rc = o3yv_decoder_next_frame(decoder, &info);
  const u64 after_decode = svcGetSystemTick();
  if (rc <= 0) {
    return rc;
  }

  rc = o3yv_decoder_write_current_yuv420p(
      decoder, left, eye_bytes, right, eye_bytes);
  const u64 after_output = svcGetSystemTick();
  if (rc != 0) {
    return rc;
  }

  render_frame_yuv420p(left, right);
  const u64 after_render = svcGetSystemTick();

  *decode_ticks = after_decode - start;
  *output_ticks = after_output - after_decode;
  *render_ticks = after_render - after_output;
  return 1;
}

static int play_one_pass(
    void *decoder, u8 *left, u8 *right, size_t eye_bytes, int collect_stats) {
  int rc = o3yv_decoder_reset(decoder);
  if (rc != 0) {
    if (collect_stats) {
      bench_log("playback_result status=fail reason=reset rc=%ld\n", (long)rc);
    }
    return rc;
  }

  u32 frames = 0;
  u32 late_frames = 0;
  u64 total_work_ticks = 0;
  u64 total_decode_ticks = 0;
  u64 total_output_ticks = 0;
  u64 total_render_ticks = 0;
  u64 worst_work_ticks = 0;
  u64 worst_decode_ticks = 0;
  u64 worst_output_ticks = 0;
  u64 worst_render_ticks = 0;

  while (aptMainLoop()) {
    hidScanInput();
    if (hidKeysDown() & KEY_START) {
      return 0;
    }

    const u64 frame_start = svcGetSystemTick();
    u64 decode_ticks = 0;
    u64 output_ticks = 0;
    u64 render_ticks = 0;
    rc = decode_render_frame(
        decoder, left, right, eye_bytes,
        &decode_ticks, &output_ticks, &render_ticks);
    if (rc == 0) {
      break;
    }
    if (rc < 0) {
      if (collect_stats) {
        bench_log("playback_result status=fail reason=decode rc=%ld\n",
            (long)rc);
      }
      return rc;
    }

    const u64 work_ticks = decode_ticks + output_ticks + render_ticks;
    total_work_ticks += work_ticks;
    total_decode_ticks += decode_ticks;
    total_output_ticks += output_ticks;
    total_render_ticks += render_ticks;
    if (work_ticks > worst_work_ticks) {
      worst_work_ticks = work_ticks;
    }
    if (decode_ticks > worst_decode_ticks) {
      worst_decode_ticks = decode_ticks;
    }
    if (output_ticks > worst_output_ticks) {
      worst_output_ticks = output_ticks;
    }
    if (render_ticks > worst_render_ticks) {
      worst_render_ticks = render_ticks;
    }
    if (ticks_to_us(work_ticks) > PLAYBACK_TARGET_US) {
      late_frames++;
    }
    frames++;
    pace_frame(frame_start);
  }

  if (collect_stats) {
    if (frames == 0) {
      bench_log("playback_result status=fail reason=no_frames\n");
      return -1;
    }

    const u64 mean_work_us = ticks_to_us(total_work_ticks) / frames;
    const u64 mean_decode_us = ticks_to_us(total_decode_ticks) / frames;
    const u64 mean_output_us = ticks_to_us(total_output_ticks) / frames;
    const u64 mean_render_us = ticks_to_us(total_render_ticks) / frames;
    const u64 worst_work_us = ticks_to_us(worst_work_ticks);
    const u64 worst_decode_us = ticks_to_us(worst_decode_ticks);
    const u64 worst_output_us = ticks_to_us(worst_output_ticks);
    const u64 worst_render_us = ticks_to_us(worst_render_ticks);
    const int timing_ok = worst_work_us <= PLAYBACK_TARGET_US;
    bench_log("playback_result status=%s frames=%lu fps=%llu "
              "renderer=%s target_frame_us=%llu mean_work_us=%llu "
              "mean_decode_us=%llu mean_output_us=%llu mean_render_us=%llu "
              "worst_work_us=%llu worst_decode_us=%llu "
              "worst_output_us=%llu worst_render_us=%llu late_frames=%lu\n",
        timing_ok ? "pass" : "fail",
        (unsigned long)frames,
        (unsigned long long)PLAYBACK_TARGET_FPS,
        playback_renderer_name(),
        (unsigned long long)PLAYBACK_TARGET_US,
        (unsigned long long)mean_work_us,
        (unsigned long long)mean_decode_us,
        (unsigned long long)mean_output_us,
        (unsigned long long)mean_render_us,
        (unsigned long long)worst_work_us,
        (unsigned long long)worst_decode_us,
        (unsigned long long)worst_output_us,
        (unsigned long long)worst_render_us,
        (unsigned long)late_frames);
  }

  return 0;
}

static void playback_loop(
    void *decoder, u8 *left, u8 *right, size_t eye_bytes) {
  g_y2r_available = init_y2r_playback();
  bench_log("playback: top stereo %s, %llu fps, START exits\n",
      playback_renderer_name(),
      (unsigned long long)PLAYBACK_TARGET_FPS);
  (void)play_one_pass(decoder, left, right, eye_bytes, 1);
  bench_log("playback_loop: looping until START\n");
  while (aptMainLoop()) {
    if (play_one_pass(decoder, left, right, eye_bytes, 0) != 0) {
      break;
    }
  }
}

int main(int argc, char **argv) {
  (void)argc;
  (void)argv;

  gfxInitDefault();
  gfxSet3D(true);
  consoleInit(GFX_BOTTOM, NULL);
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
  u32 sample_count = 0;
  u64 total_ticks = 0;
  u64 total_decode_ticks = 0;
  u64 total_output_ticks = 0;
  u64 min_ticks = ~0ULL;
  u64 worst_ticks = 0;
  u64 worst_decode_ticks = 0;
  u64 worst_output_ticks = 0;
  u64 output_checksum = 14695981039346656037ULL;
  u32 worst_iter = 0;
  u32 worst_frame_index = 0;
  u32 worst_frame_no = 0;
  u8 worst_frame_type = 0;
  O3yvFrameInfo info;

  for (int iter = 0; iter < O3YV_BENCH_ITERATIONS; iter++) {
    rc = o3yv_decoder_reset(decoder);
    if (rc != 0) {
      bench_log("reset failed: %ld\n", (long)rc);
      goto wait_exit;
    }

    u32 iter_frames = 0;
    for (;;) {
      const u64 start = svcGetSystemTick();
      rc = o3yv_decoder_next_frame(decoder, &info);
      const u64 after_decode = svcGetSystemTick();

      if (rc == 0) {
        break;
      }
      if (rc < 0) {
        bench_log("decode failed: %ld\n", (long)rc);
        goto wait_exit;
      }

      rc = o3yv_decoder_write_current_yuv420p(
          decoder, left, eye_bytes, right, eye_bytes);
      const u64 after_output = svcGetSystemTick();
      if (rc != 0) {
        bench_log("output failed: %ld\n", (long)rc);
        goto wait_exit;
      }

      const u64 decode_elapsed = after_decode - start;
      const u64 output_elapsed = after_output - after_decode;
      const u64 elapsed = after_output - start;
      if (sample_count >= MAX_BENCH_SAMPLES) {
        bench_log("too many bench samples: max=%u\n", MAX_BENCH_SAMPLES);
        goto wait_exit;
      }
      if (iter_frames >= MAX_BENCH_FRAMES) {
        bench_log("too many frames per iteration: max=%u\n", MAX_BENCH_FRAMES);
        goto wait_exit;
      }
      if (iter > 0
          && (g_frame_no[iter_frames] != info.frame_no
              || g_frame_type[iter_frames] != info.frame_type)) {
        bench_log("frame identity changed: index=%lu expected_no=%lu "
                  "actual_no=%lu expected_type=%u actual_type=%u\n",
            (unsigned long)iter_frames,
            (unsigned long)g_frame_no[iter_frames],
            (unsigned long)info.frame_no,
            (unsigned)g_frame_type[iter_frames],
            (unsigned)info.frame_type);
        goto wait_exit;
      }

      const u32 frame_index = iter_frames;
      g_bench_samples[sample_count++] = elapsed;
      update_frame_timing(
          frame_index,
          info.frame_no,
          info.frame_type,
          elapsed,
          decode_elapsed,
          output_elapsed);
      checksum_update_frame(
          &output_checksum,
          info.frame_no,
          info.frame_type,
          left,
          right,
          eye_bytes);
      total_frames++;
      total_ticks += elapsed;
      total_decode_ticks += decode_elapsed;
      total_output_ticks += output_elapsed;
      if (elapsed < min_ticks) {
        min_ticks = elapsed;
      }
      if (decode_elapsed > worst_decode_ticks) {
        worst_decode_ticks = decode_elapsed;
      }
      if (output_elapsed > worst_output_ticks) {
        worst_output_ticks = output_elapsed;
      }
      if (elapsed > worst_ticks) {
        worst_ticks = elapsed;
        worst_iter = (u32)iter;
        worst_frame_index = frame_index;
        worst_frame_no = info.frame_no;
        worst_frame_type = info.frame_type;
      }
      iter_frames++;
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

  qsort(g_bench_samples, sample_count, sizeof(g_bench_samples[0]), compare_u64);

  const u64 mean_us = ticks_to_us(total_ticks) / (u64)total_frames;
  const u64 decode_mean_us =
      ticks_to_us(total_decode_ticks) / (u64)total_frames;
  const u64 output_mean_us =
      ticks_to_us(total_output_ticks) / (u64)total_frames;
  const u64 min_us = ticks_to_us(min_ticks);
  const u64 median_us =
      ticks_to_us(percentile_ticks(g_bench_samples, sample_count, 50));
  const u64 p95_us =
      ticks_to_us(percentile_ticks(g_bench_samples, sample_count, 95));
  const u64 worst_us = ticks_to_us(worst_ticks);
  const u64 worst_decode_us = ticks_to_us(worst_decode_ticks);
  const u64 worst_output_us = ticks_to_us(worst_output_ticks);
  int output_ok = 1;
#ifdef O3YV_EXPECTED_FRAMES_PER_ITERATION
  output_ok = output_ok
      && frames_per_iteration == O3YV_EXPECTED_FRAMES_PER_ITERATION;
#endif
#ifdef O3YV_EXPECTED_CHECKSUM
  output_ok = output_ok && output_checksum == O3YV_EXPECTED_CHECKSUM;
#endif
  const int timing_ok = worst_us <= O3YV_TARGET_US;
  const int pass = timing_ok && output_ok;

  bench_log("iterations: %d\n", O3YV_BENCH_ITERATIONS);
  bench_log("frames: %lu\n", (unsigned long)total_frames);
  bench_log(
      "frames_per_iteration: %lu\n", (unsigned long)frames_per_iteration);
  print_us_as_ms("min_ms_per_frame", min_us);
  print_us_as_ms("mean_ms_per_frame", mean_us);
  print_us_as_ms("mean_decode_ms_per_frame", decode_mean_us);
  print_us_as_ms("mean_output_ms_per_frame", output_mean_us);
  print_us_as_ms("median_ms_per_frame", median_us);
  print_us_as_ms("p95_ms_per_frame", p95_us);
  print_us_as_ms("worst_frame_ms", worst_us);
  print_us_as_ms("worst_decode_ms", worst_decode_us);
  print_us_as_ms("worst_output_ms", worst_output_us);
  print_us_as_ms("target_worst_ms", O3YV_TARGET_US);
  bench_log("worst_iter: %lu\n", (unsigned long)worst_iter);
  bench_log("worst_frame_index: %lu\n", (unsigned long)worst_frame_index);
  bench_log("worst_frame_no: %lu\n", (unsigned long)worst_frame_no);
  bench_log("worst_frame_type: %u\n", (unsigned)worst_frame_type);
  print_top_frame_timings(frames_per_iteration);
  bench_log(
      "output_checksum: %016llx\n", (unsigned long long)output_checksum);
#ifdef O3YV_EXPECTED_CHECKSUM
  bench_log("expected_checksum: %016llx\n",
      (unsigned long long)O3YV_EXPECTED_CHECKSUM);
#else
  bench_log("expected_checksum: unavailable\n");
#endif
#ifdef O3YV_EXPECTED_FRAMES_PER_ITERATION
  bench_log("expected_frames_per_iteration: %u\n",
      (unsigned)O3YV_EXPECTED_FRAMES_PER_ITERATION);
#else
  bench_log("expected_frames_per_iteration: unavailable\n");
#endif
  bench_log("timing_status: %s\n", timing_ok ? "pass" : "fail");
  bench_log("output_status: %s\n", output_ok ? "pass" : "fail");
  bench_log("bench_result status=%s iterations=%d frames=%lu "
            "frames_per_iteration=%lu min_us=%llu mean_us=%llu "
            "decode_mean_us=%llu output_mean_us=%llu "
            "median_us=%llu p95_us=%llu worst_us=%llu "
            "worst_decode_us=%llu worst_output_us=%llu target_us=%llu "
            "worst_iter=%lu worst_frame_index=%lu "
            "worst_frame_no=%lu worst_frame_type=%u "
            "checksum=%016llx "
#ifdef O3YV_EXPECTED_CHECKSUM
            "expected_checksum=%016llx "
#endif
#ifdef O3YV_EXPECTED_FRAMES_PER_ITERATION
            "expected_frames_per_iteration=%u "
#endif
            "timing_status=%s output_status=%s\n",
      pass ? "pass" : "fail",
      O3YV_BENCH_ITERATIONS,
      (unsigned long)total_frames,
      (unsigned long)frames_per_iteration,
      (unsigned long long)min_us,
      (unsigned long long)mean_us,
      (unsigned long long)decode_mean_us,
      (unsigned long long)output_mean_us,
      (unsigned long long)median_us,
      (unsigned long long)p95_us,
      (unsigned long long)worst_us,
      (unsigned long long)worst_decode_us,
      (unsigned long long)worst_output_us,
      (unsigned long long)O3YV_TARGET_US,
      (unsigned long)worst_iter,
      (unsigned long)worst_frame_index,
      (unsigned long)worst_frame_no,
      (unsigned)worst_frame_type,
      (unsigned long long)output_checksum,
#ifdef O3YV_EXPECTED_CHECKSUM
      (unsigned long long)O3YV_EXPECTED_CHECKSUM,
#endif
#ifdef O3YV_EXPECTED_FRAMES_PER_ITERATION
      (unsigned)O3YV_EXPECTED_FRAMES_PER_ITERATION,
#endif
      timing_ok ? "pass" : "fail",
      output_ok ? "pass" : "fail");
  bench_log("%s\n", pass ? "PASS" : "FAIL");
  playback_loop(decoder, left, right, eye_bytes);

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
  if (g_y2r_initialized) {
    y2rExit();
    g_y2r_initialized = 0;
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
