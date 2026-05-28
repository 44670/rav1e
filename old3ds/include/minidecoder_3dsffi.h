#pragma once

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct O3yvFrameInfo {
  uint32_t frame_no;
  uint8_t frame_type;
  uint8_t reserved[3];
} O3yvFrameInfo;

size_t o3yv_decoder_size(void);
size_t o3yv_decoder_align(void);
size_t o3yv_eye_frame_bytes(void);

int32_t o3yv_decoder_init(
    void *decoder, size_t decoder_len, const uint8_t *stream,
    size_t stream_len);
int32_t o3yv_decoder_reset(void *decoder);
int32_t o3yv_decoder_next_frame_yuv420p(
    void *decoder, uint8_t *left_yuv420p, size_t left_len,
    uint8_t *right_yuv420p, size_t right_len, O3yvFrameInfo *info);
void o3yv_decoder_drop(void *decoder);

#ifdef __cplusplus
}
#endif
