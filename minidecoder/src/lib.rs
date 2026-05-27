#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

#[cfg(test)]
extern crate std;

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::fmt;

pub const VISIBLE_W: usize = 800;
pub const VISIBLE_H: usize = 240;
pub const EYE_W: usize = 400;
pub const EYE_H: usize = 240;
pub const CHROMA_W: usize = 200;
pub const CHROMA_H: usize = 120;
pub const MB_W: usize = 25;
pub const MB_H: usize = 15;
pub const MB_COUNT: usize = MB_W * MB_H;
pub const RAW_MB_BYTES: usize = 384;
pub const SBS_FRAME_BYTES: usize =
  VISIBLE_W * VISIBLE_H + (VISIBLE_W / 2) * (VISIBLE_H / 2) * 2;
pub const EYE_FRAME_BYTES: usize = EYE_W * EYE_H + CHROMA_W * CHROMA_H * 2;

const FILE_MAGIC: u32 = u32::from_le_bytes(*b"O3YV");
const FRAME_MAGIC: u32 = u32::from_le_bytes(*b"FRM1");
const FILE_HEADER_SIZE: u16 = 60;
const FRAME_HEADER_SIZE: usize = 28;
const TILE_HEADER_SIZE: usize = 17;
const FRAGMENT_HEADER_SIZE: usize = 28;
pub const LAZY_BASE_COPY_MAX_MB_PER_TILE: usize = 128;
pub const TILE_FLAG_LAZY_BASE_COPY: u16 = 0x0001;
const SUPPORTED_TILE_FLAGS: u16 = TILE_FLAG_LAZY_BASE_COPY;
const REFERENCE_REUSE_FRAME_PAYLOAD_MAX: usize = 512;
const DC_CLIP_DELTA_MIN: i32 = -16;
const DC_CLIP_DELTA_MAX: i32 = 16;
const DC_CLIP_TABLE: [[u8; 256]; 33] = build_dc_clip_table();

pub const FRAME_TYPE_KEY_RAW: u8 = 0;
pub const FRAME_TYPE_P: u8 = 2;

pub const MODE_BASE_RES: u8 = 0;
pub const MODE_COPY16: u8 = 1;
pub const MODE_COPY16_RES: u8 = 2;
pub const MODE_COPY16X8: u8 = 3;
pub const MODE_COPY16X8_RES: u8 = 4;
pub const MODE_COPY8X16: u8 = 5;
pub const MODE_COPY8X16_RES: u8 = 6;
pub const MODE_COPY8X8: u8 = 7;
pub const MODE_COPY8X8_RES: u8 = 8;
pub const MODE_RAW_MB: u8 = 10;

pub const TAG_DC_ONLY_S8: u8 = 0x00;
pub const TAG_DC_ONLY_S16: u8 = 0x40;
pub const TAG_AC_MASK_S8: u8 = 0x80;
pub const TAG_AC_MASK_S16: u8 = 0xC0;
pub const TAG_RAW_4X4: u8 = 0xE0;

const ZIGZAG: [usize; 16] =
  [0, 1, 4, 8, 5, 2, 3, 6, 9, 12, 13, 10, 7, 11, 14, 15];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EyeFrame {
  pub y: Vec<u8>,
  pub cb: Vec<u8>,
  pub cr: Vec<u8>,
}

impl EyeFrame {
  pub fn new() -> Self {
    Self {
      y: vec![0; EYE_W * EYE_H],
      cb: vec![128; CHROMA_W * CHROMA_H],
      cr: vec![128; CHROMA_W * CHROMA_H],
    }
  }
}

impl Default for EyeFrame {
  fn default() -> Self {
    Self::new()
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SbsFrame {
  pub left: EyeFrame,
  pub right: EyeFrame,
}

impl SbsFrame {
  pub fn new() -> Self {
    Self { left: EyeFrame::new(), right: EyeFrame::new() }
  }

  pub fn from_yuv420_sbs(bytes: &[u8]) -> Result<Self> {
    if bytes.len() != SBS_FRAME_BYTES {
      return Err(Error::Invalid(format!(
        "expected {SBS_FRAME_BYTES} YUV420 bytes, got {}",
        bytes.len()
      )));
    }

    let y_plane = &bytes[..VISIBLE_W * VISIBLE_H];
    let cb_start = VISIBLE_W * VISIBLE_H;
    let cb_len = (VISIBLE_W / 2) * (VISIBLE_H / 2);
    let cb_plane = &bytes[cb_start..cb_start + cb_len];
    let cr_plane = &bytes[cb_start + cb_len..];

    let mut frame = Self::new();
    for row in 0..EYE_H {
      let src = &y_plane[row * VISIBLE_W..(row + 1) * VISIBLE_W];
      frame.left.y[row * EYE_W..(row + 1) * EYE_W]
        .copy_from_slice(&src[..EYE_W]);
      frame.right.y[row * EYE_W..(row + 1) * EYE_W]
        .copy_from_slice(&src[EYE_W..]);
    }

    for row in 0..CHROMA_H {
      let src_cb =
        &cb_plane[row * (VISIBLE_W / 2)..(row + 1) * (VISIBLE_W / 2)];
      let src_cr =
        &cr_plane[row * (VISIBLE_W / 2)..(row + 1) * (VISIBLE_W / 2)];
      frame.left.cb[row * CHROMA_W..(row + 1) * CHROMA_W]
        .copy_from_slice(&src_cb[..CHROMA_W]);
      frame.right.cb[row * CHROMA_W..(row + 1) * CHROMA_W]
        .copy_from_slice(&src_cb[CHROMA_W..]);
      frame.left.cr[row * CHROMA_W..(row + 1) * CHROMA_W]
        .copy_from_slice(&src_cr[..CHROMA_W]);
      frame.right.cr[row * CHROMA_W..(row + 1) * CHROMA_W]
        .copy_from_slice(&src_cr[CHROMA_W..]);
    }

    Ok(frame)
  }

  pub fn to_yuv420_sbs(&self) -> Vec<u8> {
    let mut out = vec![0; SBS_FRAME_BYTES];
    let cb_start = VISIBLE_W * VISIBLE_H;
    let cb_len = (VISIBLE_W / 2) * (VISIBLE_H / 2);
    let cr_start = cb_start + cb_len;

    for row in 0..EYE_H {
      let dst = &mut out[row * VISIBLE_W..(row + 1) * VISIBLE_W];
      dst[..EYE_W]
        .copy_from_slice(&self.left.y[row * EYE_W..(row + 1) * EYE_W]);
      dst[EYE_W..]
        .copy_from_slice(&self.right.y[row * EYE_W..(row + 1) * EYE_W]);
    }

    let (cb_and_y, cr_plane) = out.split_at_mut(cr_start);
    let cb_plane = &mut cb_and_y[cb_start..];
    for row in 0..CHROMA_H {
      let dst_cb =
        &mut cb_plane[row * (VISIBLE_W / 2)..(row + 1) * (VISIBLE_W / 2)];
      let dst_cr =
        &mut cr_plane[row * (VISIBLE_W / 2)..(row + 1) * (VISIBLE_W / 2)];
      dst_cb[..CHROMA_W]
        .copy_from_slice(&self.left.cb[row * CHROMA_W..(row + 1) * CHROMA_W]);
      dst_cb[CHROMA_W..]
        .copy_from_slice(&self.right.cb[row * CHROMA_W..(row + 1) * CHROMA_W]);
      dst_cr[..CHROMA_W]
        .copy_from_slice(&self.left.cr[row * CHROMA_W..(row + 1) * CHROMA_W]);
      dst_cr[CHROMA_W..]
        .copy_from_slice(&self.right.cr[row * CHROMA_W..(row + 1) * CHROMA_W]);
    }

    out
  }
}

impl Default for SbsFrame {
  fn default() -> Self {
    Self::new()
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedFrame {
  pub frame_no: u32,
  pub frame_type: u8,
  pub frame: SbsFrame,
}

#[derive(Debug, Clone, Copy)]
pub struct DecodedFrameRef<'a> {
  pub frame_no: u32,
  pub frame_type: u8,
  pub frame: &'a SbsFrame,
}

#[derive(Debug, Clone)]
pub struct DecoderState {
  reference: SbsFrame,
  current: SbsFrame,
  has_reference: bool,
}

impl DecoderState {
  pub fn new() -> Self {
    Self {
      reference: SbsFrame::new(),
      current: SbsFrame::new(),
      has_reference: false,
    }
  }

  pub fn reset(&mut self) {
    self.has_reference = false;
  }
}

impl Default for DecoderState {
  fn default() -> Self {
    Self::new()
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mv {
  pub x: i8,
  pub y: i8,
}

impl Mv {
  pub const ZERO: Self = Self { x: 0, y: 0 };
}

#[derive(Debug)]
pub enum Error {
  Eof,
  Invalid(String),
  Unsupported(String),
}

impl fmt::Display for Error {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Error::Eof => write!(f, "unexpected end of stream"),
      Error::Invalid(msg) => write!(f, "invalid stream: {msg}"),
      Error::Unsupported(msg) => write!(f, "unsupported stream: {msg}"),
    }
  }
}

#[cfg(feature = "std")]
impl std::error::Error for Error {}

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Debug, Clone)]
pub struct EncodedFragment {
  pub tile_id: u8,
  pub row_start: u8,
  pub row_count: u8,
  pub start_mb: u16,
  pub mb_count: u16,
  pub segment_map_stream: Vec<u8>,
  pub mode_stream: Vec<u8>,
  pub residual_stream: Vec<u8>,
  pub raw_stream: Vec<u8>,
}

impl EncodedFragment {
  pub fn full_eye(
    tile_id: u8, mode_stream: Vec<u8>, residual_stream: Vec<u8>,
    raw_stream: Vec<u8>,
  ) -> Self {
    Self {
      tile_id,
      row_start: 0,
      row_count: MB_H as u8,
      start_mb: 0,
      mb_count: MB_COUNT as u16,
      segment_map_stream: Vec::new(),
      mode_stream,
      residual_stream,
      raw_stream,
    }
  }
}

#[derive(Debug, Clone)]
pub struct EncodedTile {
  pub tile_id: u8,
  pub base_mv: Mv,
  pub segment_count: u8,
  pub fragments: Vec<EncodedFragment>,
}

pub fn write_file_header(out: &mut Vec<u8>, frame_count: u32) {
  put_u32(out, FILE_MAGIC);
  put_u16(out, 1);
  put_u16(out, 0);
  put_u16(out, FILE_HEADER_SIZE);
  put_u16(out, 0);
  put_u16(out, VISIBLE_W as u16);
  put_u16(out, VISIBLE_H as u16);
  put_u16(out, EYE_W as u16);
  put_u16(out, EYE_H as u16);
  put_u16(out, 24);
  put_u16(out, 1);
  out.extend_from_slice(&[0, 1, 1, 0]);
  put_u32(out, 16_000_000);
  put_u32(out, 1_048_576);
  put_u32(out, 131_072);
  put_u32(out, 288_000);
  put_u16(out, 48);
  put_u16(out, 0);
  put_u32(out, frame_count);
  put_u64(out, 0);
}

pub fn write_key_raw_frame(
  out: &mut Vec<u8>, frame_no: u32, frame: &SbsFrame,
) {
  let mut payload = Vec::with_capacity(SBS_FRAME_BYTES);
  write_eye_raw(&mut payload, &frame.left);
  write_eye_raw(&mut payload, &frame.right);
  write_frame_header(
    out,
    payload.len() as u32,
    frame_no,
    FRAME_TYPE_KEY_RAW,
    0,
  );
  out.extend_from_slice(&payload);
}

pub fn write_p_frame(
  out: &mut Vec<u8>, frame_no: u32, left: EncodedTile, right: EncodedTile,
) {
  let mut payload = Vec::new();
  write_tile(&mut payload, &left);
  write_tile(&mut payload, &right);
  write_frame_header(out, payload.len() as u32, frame_no, FRAME_TYPE_P, 0);
  out.extend_from_slice(&payload);
}

pub fn decode_stream(bytes: &[u8]) -> Result<Vec<SbsFrame>> {
  Ok(
    decode_stream_with_metadata(bytes)?
      .into_iter()
      .map(|decoded| decoded.frame)
      .collect(),
  )
}

pub fn decode_stream_with_metadata(bytes: &[u8]) -> Result<Vec<DecodedFrame>> {
  let mut frames = Vec::new();
  decode_stream_for_each(bytes, |decoded| {
    frames.push(DecodedFrame {
      frame_no: decoded.frame_no,
      frame_type: decoded.frame_type,
      frame: decoded.frame.clone(),
    });
  })?;
  Ok(frames)
}

pub fn decode_stream_for_each<F>(
  bytes: &[u8], mut on_frame: F,
) -> Result<usize>
where
  F: FnMut(DecodedFrameRef<'_>),
{
  let mut state = DecoderState::new();
  decode_stream_for_each_with_state(bytes, &mut state, |decoded| {
    on_frame(decoded);
  })
}

pub fn decode_stream_for_each_with_state<F>(
  bytes: &[u8], state: &mut DecoderState, mut on_frame: F,
) -> Result<usize>
where
  F: FnMut(DecodedFrameRef<'_>),
{
  let mut r = Reader::new(bytes);
  parse_file_header(&mut r)?;

  state.reset();
  let mut frame_count = 0usize;

  while r.remaining() > 0 {
    let frame_start = r.pos;
    let start_code = r.u32()?;
    if start_code != FRAME_MAGIC {
      return Err(Error::Invalid("bad frame start code".into()));
    }
    let frame_size = r.u32()? as usize;
    let frame_no = r.u32()?;
    let _pts_ticks = r.u32()?;
    let frame_type = r.u8()?;
    let tile_count = r.u8()?;
    let flags = r.u8()?;
    let reserved = r.u8()?;
    let _crc = r.u32()?;
    let _cost = r.u32()?;

    if flags != 0 || reserved != 0 {
      return Err(Error::Invalid(format!(
        "reserved frame bits set at frame {frame_no}"
      )));
    }
    let payload_start = frame_start + FRAME_HEADER_SIZE;
    let payload_end = payload_start
      .checked_add(frame_size)
      .ok_or_else(|| Error::Invalid("frame size overflow".into()))?;
    if payload_end > bytes.len() {
      return Err(Error::Eof);
    }

    match frame_type {
      FRAME_TYPE_KEY_RAW => {
        if tile_count != 2 {
          return Err(Error::Invalid("KEY_RAW tile_count must be 2".into()));
        }
        if frame_size != SBS_FRAME_BYTES {
          return Err(Error::Invalid(format!(
            "KEY_RAW payload must be {SBS_FRAME_BYTES} bytes, got {frame_size}"
          )));
        }
        read_eye_raw_into(
          &mut state.current.left,
          &bytes[payload_start..payload_start + EYE_FRAME_BYTES],
        )?;
        read_eye_raw_into(
          &mut state.current.right,
          &bytes[payload_start + EYE_FRAME_BYTES..payload_end],
        )?;
      }
      FRAME_TYPE_P => {
        if tile_count != 2 {
          return Err(Error::Invalid("P-frame tile_count must be 2".into()));
        }
        if !state.has_reference {
          return Err(Error::Invalid(
            "P-frame cannot appear before a reference frame".into(),
          ));
        }
        let reference = &state.reference;
        if frame_size <= REFERENCE_REUSE_FRAME_PAYLOAD_MAX
          && p_payload_reuses_reference(
            &bytes[payload_start..payload_end],
            tile_count,
          )
        {
          r.pos = payload_end;
          on_frame(DecodedFrameRef { frame_no, frame_type, frame: reference });
          frame_count += 1;
          continue;
        }
        let mut pr = Reader::new(&bytes[payload_start..payload_end]);
        for _ in 0..2 {
          decode_tile(&mut pr, reference, &mut state.current)?;
        }
        if pr.remaining() != 0 {
          return Err(Error::Invalid("unconsumed P-frame payload".into()));
        }
      }
      _ => return Err(Error::Unsupported(format!("frame type {frame_type}"))),
    };

    r.pos = payload_end;
    on_frame(DecodedFrameRef { frame_no, frame_type, frame: &state.current });
    core::mem::swap(&mut state.reference, &mut state.current);
    state.has_reference = true;
    frame_count += 1;
  }

  Ok(frame_count)
}

fn write_frame_header(
  out: &mut Vec<u8>, payload_len: u32, frame_no: u32, frame_type: u8,
  cost: u32,
) {
  put_u32(out, FRAME_MAGIC);
  put_u32(out, payload_len);
  put_u32(out, frame_no);
  put_u32(out, frame_no);
  out.push(frame_type);
  out.push(2);
  out.push(0);
  out.push(0);
  put_u32(out, 0);
  put_u32(out, cost);
}

fn write_tile(out: &mut Vec<u8>, tile: &EncodedTile) {
  let mut payload = Vec::new();
  for fragment in &tile.fragments {
    write_fragment(&mut payload, fragment);
  }

  out.push(tile.tile_id);
  out.push(0);
  out.push(0);
  out.push(MB_W as u8);
  out.push(MB_H as u8);
  out.push(tile.base_mv.x as u8);
  out.push(tile.base_mv.y as u8);
  out.push(64);
  out.push(64);
  out.push(tile.segment_count);
  out.push(tile.fragments.len() as u8);
  put_u16(out, tile_flags_for_encoded_tile(tile));
  put_u32(out, payload.len() as u32);
  out.extend_from_slice(&payload);
}

fn tile_flags_for_encoded_tile(tile: &EncodedTile) -> u16 {
  if encoded_tile_base_predictor_mbs(tile) <= LAZY_BASE_COPY_MAX_MB_PER_TILE {
    TILE_FLAG_LAZY_BASE_COPY
  } else {
    0
  }
}

fn encoded_tile_base_predictor_mbs(tile: &EncodedTile) -> usize {
  let mut total = 0usize;
  for fragment in &tile.fragments {
    let Ok(base_mbs) = count_base_predictor_mbs_in_modes(
      &fragment.mode_stream,
      fragment.start_mb as usize,
      fragment.mb_count as usize,
      LAZY_BASE_COPY_MAX_MB_PER_TILE.saturating_sub(total),
    ) else {
      return LAZY_BASE_COPY_MAX_MB_PER_TILE + 1;
    };
    total += base_mbs;
    if total > LAZY_BASE_COPY_MAX_MB_PER_TILE {
      return total;
    }
  }
  total
}

fn write_fragment(out: &mut Vec<u8>, fragment: &EncodedFragment) {
  out.push(fragment.tile_id);
  out.push(fragment.row_start);
  out.push(fragment.row_count);
  out.push(0);
  put_u16(out, fragment.start_mb);
  put_u16(out, fragment.mb_count);
  put_u32(out, fragment.segment_map_stream.len() as u32);
  put_u32(out, fragment.mode_stream.len() as u32);
  put_u32(out, fragment.residual_stream.len() as u32);
  put_u32(out, fragment.raw_stream.len() as u32);
  put_u32(out, 0);
  out.extend_from_slice(&fragment.segment_map_stream);
  out.extend_from_slice(&fragment.mode_stream);
  out.extend_from_slice(&fragment.residual_stream);
  out.extend_from_slice(&fragment.raw_stream);
}

fn parse_file_header(r: &mut Reader<'_>) -> Result<()> {
  if r.u32()? != FILE_MAGIC {
    return Err(Error::Invalid("bad file magic".into()));
  }
  let major = r.u16()?;
  let minor = r.u16()?;
  let header_size = r.u16()?;
  let _header_crc = r.u16()?;
  let visible_w = r.u16()?;
  let visible_h = r.u16()?;
  let eye_w = r.u16()?;
  let eye_h = r.u16()?;
  let fps_num = r.u16()?;
  let fps_den = r.u16()?;
  let pixel_format = r.u8()?;
  let stereo_mode = r.u8()?;
  let _color_matrix = r.u8()?;
  let _color_range = r.u8()?;
  let _max_video_bitrate = r.u32()?;
  let _cpb_size = r.u32()?;
  let _max_p_frame = r.u32()?;
  let _max_key_frame = r.u32()?;
  let _keyint = r.u16()?;
  let flags = r.u16()?;
  let _frame_count = r.u32()?;
  let _index_offset = r.u64()?;

  if major != 1 || minor != 0 {
    return Err(Error::Unsupported(format!("version {major}.{minor}")));
  }
  if header_size < FILE_HEADER_SIZE {
    return Err(Error::Invalid("file header too small".into()));
  }
  if visible_w as usize != VISIBLE_W
    || visible_h as usize != VISIBLE_H
    || eye_w as usize != EYE_W
    || eye_h as usize != EYE_H
  {
    return Err(Error::Unsupported(
      "only 800x240 SBS / 400x240 eyes are supported".into(),
    ));
  }
  if fps_num != 24 || fps_den != 1 || pixel_format != 0 || stereo_mode != 1 {
    return Err(Error::Unsupported("unsupported profile fields".into()));
  }
  if flags != 0 {
    return Err(Error::Unsupported(
      "file flags are not implemented yet".into(),
    ));
  }
  if header_size as usize > FILE_HEADER_SIZE as usize {
    r.skip(header_size as usize - FILE_HEADER_SIZE as usize)?;
  }
  Ok(())
}

fn decode_tile(
  r: &mut Reader<'_>, reference: &SbsFrame, current: &mut SbsFrame,
) -> Result<()> {
  if r.remaining() < TILE_HEADER_SIZE {
    return Err(Error::Eof);
  }
  let tile_id = r.u8()?;
  let mb_x = r.u8()?;
  let mb_y = r.u8()?;
  let mb_w = r.u8()?;
  let mb_h = r.u8()?;
  let base_mv = Mv { x: r.i8()?, y: r.i8()? };
  let q_y = r.u8()?;
  let q_uv = r.u8()?;
  let segment_count = r.u8()?;
  let fragment_count = r.u8()?;
  let tile_flags = r.u16()?;
  let payload_size = r.u32()? as usize;

  if tile_id > 1
    || mb_x != 0
    || mb_y != 0
    || mb_w as usize != MB_W
    || mb_h as usize != MB_H
  {
    return Err(Error::Invalid("invalid tile geometry".into()));
  }
  if q_y > 127
    || q_uv > 127
    || !(1..=4).contains(&segment_count)
    || fragment_count == 0
    || (tile_flags & !SUPPORTED_TILE_FLAGS) != 0
  {
    return Err(Error::Unsupported("unsupported tile options".into()));
  }
  if payload_size > r.remaining() {
    return Err(Error::Eof);
  }

  let payload = r.take(payload_size)?;
  let ref_eye = if tile_id == 0 { &reference.left } else { &reference.right };
  let cur_eye =
    if tile_id == 0 { &mut current.left } else { &mut current.right };
  let base_copy_mode = if (tile_flags & TILE_FLAG_LAZY_BASE_COPY) != 0 {
    BaseCopyMode::Lazy(base_mv)
  } else {
    prefill_eye(cur_eye, ref_eye, base_mv);
    BaseCopyMode::Prefilled
  };

  let mut tr = Reader::new(payload);
  for _ in 0..fragment_count {
    decode_fragment(
      &mut tr,
      tile_id,
      segment_count,
      base_copy_mode,
      ref_eye,
      cur_eye,
    )?;
  }
  if tr.remaining() != 0 {
    return Err(Error::Invalid("unconsumed tile payload".into()));
  }
  Ok(())
}

#[inline(never)]
fn p_payload_reuses_reference(payload: &[u8], tile_count: u8) -> bool {
  let mut r = Reader::new(payload);
  for _ in 0..tile_count {
    let Some(identity) = tile_reuses_reference(&mut r) else {
      return false;
    };
    if !identity {
      return false;
    }
  }
  r.remaining() == 0
}

fn tile_reuses_reference(r: &mut Reader<'_>) -> Option<bool> {
  if r.remaining() < TILE_HEADER_SIZE {
    return None;
  }
  let tile_id = r.u8().ok()?;
  let mb_x = r.u8().ok()?;
  let mb_y = r.u8().ok()?;
  let mb_w = r.u8().ok()?;
  let mb_h = r.u8().ok()?;
  let base_mv = Mv { x: r.i8().ok()?, y: r.i8().ok()? };
  let q_y = r.u8().ok()?;
  let q_uv = r.u8().ok()?;
  let segment_count = r.u8().ok()?;
  let fragment_count = r.u8().ok()?;
  let tile_flags = r.u16().ok()?;
  let payload_size = r.u32().ok()? as usize;

  if tile_id > 1
    || mb_x != 0
    || mb_y != 0
    || mb_w as usize != MB_W
    || mb_h as usize != MB_H
    || q_y > 127
    || q_uv > 127
    || !(1..=4).contains(&segment_count)
    || fragment_count == 0
    || (tile_flags & !SUPPORTED_TILE_FLAGS) != 0
  {
    return None;
  }
  if base_mv != Mv::ZERO {
    return Some(false);
  }

  let payload = r.take(payload_size).ok()?;
  Some(tile_payload_reuses_reference(
    payload,
    tile_id,
    segment_count,
    fragment_count,
  ))
}

fn tile_payload_reuses_reference(
  payload: &[u8], tile_id: u8, segment_count: u8, fragment_count: u8,
) -> bool {
  let mut r = Reader::new(payload);
  for _ in 0..fragment_count {
    let Some(identity) =
      fragment_payload_reuses_reference(&mut r, tile_id, segment_count)
    else {
      return false;
    };
    if !identity {
      return false;
    }
  }
  r.remaining() == 0
}

fn fragment_payload_reuses_reference(
  r: &mut Reader<'_>, tile_id: u8, segment_count: u8,
) -> Option<bool> {
  if r.remaining() < FRAGMENT_HEADER_SIZE {
    return None;
  }
  let frag_tile_id = r.u8().ok()?;
  let row_start = r.u8().ok()?;
  let row_count = r.u8().ok()?;
  let flags = r.u8().ok()?;
  let start_mb = r.u16().ok()? as usize;
  let mb_count = r.u16().ok()? as usize;
  let segment_map_size = r.u32().ok()? as usize;
  let mode_size = r.u32().ok()? as usize;
  let residual_size = r.u32().ok()? as usize;
  let raw_size = r.u32().ok()? as usize;
  let crc = r.u32().ok()?;

  if residual_size != 0 || raw_size != 0 {
    return Some(false);
  }

  validate_fragment_header(
    frag_tile_id,
    tile_id,
    flags,
    crc,
    row_start,
    row_count,
    start_mb,
    mb_count,
  )
  .ok()?;

  let segment_map = r.take(segment_map_size).ok()?;
  validate_segment_map(segment_map, segment_count, mb_count).ok()?;
  let mode_stream = r.take(mode_size).ok()?;
  let residual_stream = r.take(residual_size).ok()?;
  let raw_stream = r.take(raw_size).ok()?;

  Some(
    residual_stream.is_empty()
      && raw_stream.is_empty()
      && mode_stream_is_all_skip(mode_stream, mb_count),
  )
}

fn mode_stream_is_all_skip(mode_stream: &[u8], mb_count: usize) -> bool {
  let mut r = Reader::new(mode_stream);
  let mut described = 0usize;
  while described < mb_count {
    let Ok(op) = r.u8() else {
      return false;
    };
    if op == 0 || op > 0x7f {
      return false;
    }
    described += op as usize;
    if described > mb_count {
      return false;
    }
  }
  if r.remaining() == 1 && r.bytes[r.pos] == 0 {
    r.pos += 1;
  }
  r.remaining() == 0
}

#[derive(Clone, Copy)]
enum BaseCopyMode {
  Prefilled,
  Lazy(Mv),
}

fn decode_fragment(
  r: &mut Reader<'_>, tile_id: u8, segment_count: u8,
  base_copy_mode: BaseCopyMode, reference: &EyeFrame, current: &mut EyeFrame,
) -> Result<()> {
  if r.remaining() < FRAGMENT_HEADER_SIZE {
    return Err(Error::Eof);
  }
  let frag_tile_id = r.u8()?;
  let row_start = r.u8()?;
  let row_count = r.u8()?;
  let flags = r.u8()?;
  let start_mb = r.u16()? as usize;
  let mb_count = r.u16()? as usize;
  let segment_map_size = r.u32()? as usize;
  let mode_size = r.u32()? as usize;
  let residual_size = r.u32()? as usize;
  let raw_size = r.u32()? as usize;
  let crc = r.u32()?;

  validate_fragment_header(
    frag_tile_id,
    tile_id,
    flags,
    crc,
    row_start,
    row_count,
    start_mb,
    mb_count,
  )?;

  let segment_map = r.take(segment_map_size)?;
  validate_segment_map(segment_map, segment_count, mb_count)?;
  let mode_stream = r.take(mode_size)?;
  let residual_stream = r.take(residual_size)?;
  let raw_stream = r.take(raw_size)?;

  let mut mr = Reader::new(mode_stream);
  let mut rr = Reader::new(residual_stream);
  let mut raw = Reader::new(raw_stream);
  let end_mb = start_mb + mb_count;
  let mut mb_index = start_mb;

  while mb_index < end_mb {
    let op = mr.u8()?;
    if op == 0 {
      break;
    } else if op <= 0x7f {
      let run = op as usize;
      if mb_index + run > end_mb {
        return Err(Error::Invalid("skip run exceeds fragment".into()));
      }
      if let BaseCopyMode::Lazy(base_mv) = base_copy_mode {
        for skipped in mb_index..mb_index + run {
          copy_mb_from_reference(current, reference, skipped, base_mv);
        }
      }
      mb_index += run;
    } else if (op & 0xf0) == 0x80 {
      let mode = op & 0x0f;
      decode_one_mb(
        mode,
        mb_index,
        base_copy_mode,
        &mut mr,
        &mut rr,
        &mut raw,
        reference,
        current,
      )?;
      mb_index += 1;
    } else if (op & 0xf0) == 0x90 {
      let mode = op & 0x0f;
      let run = mr.u8()? as usize + 1;
      if mb_index + run > end_mb {
        return Err(Error::Invalid("mode run exceeds fragment".into()));
      }
      for _ in 0..run {
        decode_one_mb(
          mode,
          mb_index,
          base_copy_mode,
          &mut mr,
          &mut rr,
          &mut raw,
          reference,
          current,
        )?;
        mb_index += 1;
      }
    } else {
      return Err(Error::Invalid(format!("reserved mode opcode 0x{op:02x}")));
    }
  }

  if mb_index != end_mb {
    return Err(Error::Invalid(
      "fragment ended before all MBs were described".into(),
    ));
  }
  if mr.remaining() == 1 && mr.bytes[mr.pos] == 0 {
    mr.pos += 1;
  }
  if mr.remaining() != 0 || rr.remaining() != 0 || raw.remaining() != 0 {
    return Err(Error::Invalid(
      "fragment streams were not fully consumed".into(),
    ));
  }
  Ok(())
}

fn validate_segment_map(
  bytes: &[u8], segment_count: u8, mb_count: usize,
) -> Result<()> {
  if segment_count == 1 {
    if !bytes.is_empty() {
      return Err(Error::Invalid(
        "segment map present when segment_count is 1".into(),
      ));
    }
    return Ok(());
  }

  let mut r = Reader::new(bytes);
  let mut described = 0usize;
  while described < mb_count {
    let run = r.u8()? as usize + 1;
    let segment_id = r.u8()?;
    if segment_id >= segment_count {
      return Err(Error::Invalid("segment id out of range".into()));
    }
    described = described
      .checked_add(run)
      .ok_or_else(|| Error::Invalid("segment map run overflow".into()))?;
    if described > mb_count {
      return Err(Error::Invalid("segment map exceeds fragment".into()));
    }
  }
  if r.remaining() != 0 {
    return Err(Error::Invalid("segment map was not fully consumed".into()));
  }
  Ok(())
}

fn validate_fragment_header(
  frag_tile_id: u8, tile_id: u8, flags: u8, crc: u32, row_start: u8,
  row_count: u8, start_mb: usize, mb_count: usize,
) -> Result<()> {
  if frag_tile_id != tile_id || flags != 0 || crc != 0 {
    return Err(Error::Invalid("invalid fragment header".into()));
  }
  if start_mb >= MB_COUNT || mb_count == 0 || start_mb + mb_count > MB_COUNT {
    return Err(Error::Invalid("fragment MB range out of bounds".into()));
  }
  if row_start as usize >= MB_H
    || row_count == 0
    || row_start as usize + row_count as usize > MB_H
  {
    return Err(Error::Invalid("fragment row range out of bounds".into()));
  }
  if start_mb != row_start as usize * MB_W
    || mb_count != row_count as usize * MB_W
  {
    return Err(Error::Unsupported(
      "only contiguous row-band fragments are supported".into(),
    ));
  }
  Ok(())
}

fn count_base_predictor_mbs_in_modes(
  mode_stream: &[u8], start_mb: usize, mb_count: usize, limit: usize,
) -> Result<usize> {
  let mut base_mbs = 0usize;
  let mut r = Reader::new(mode_stream);
  let end_mb = start_mb + mb_count;
  let mut mb_index = start_mb;
  while mb_index < end_mb {
    let op = r.u8()?;
    if op == 0 {
      break;
    } else if op <= 0x7f {
      let run = op as usize;
      if mb_index + run > end_mb {
        return Err(Error::Invalid("skip run exceeds fragment".into()));
      }
      base_mbs += run;
      if base_mbs > limit {
        return Ok(base_mbs);
      }
      mb_index += run;
    } else if (op & 0xf0) == 0x80 {
      let mode = op & 0x0f;
      skip_mode_payload(mode, &mut r)?;
      if mode == MODE_BASE_RES {
        base_mbs += 1;
        if base_mbs > limit {
          return Ok(base_mbs);
        }
      }
      mb_index += 1;
    } else if (op & 0xf0) == 0x90 {
      let mode = op & 0x0f;
      let run = r.u8()? as usize + 1;
      if mb_index + run > end_mb {
        return Err(Error::Invalid("mode run exceeds fragment".into()));
      }
      for _ in 0..run {
        skip_mode_payload(mode, &mut r)?;
      }
      if mode == MODE_BASE_RES {
        base_mbs += run;
        if base_mbs > limit {
          return Ok(base_mbs);
        }
      }
      mb_index += run;
    } else {
      return Err(Error::Invalid(format!("reserved mode opcode 0x{op:02x}")));
    }
  }
  if mb_index != end_mb {
    return Err(Error::Invalid(
      "fragment ended before all MBs were described".into(),
    ));
  }
  if r.remaining() == 1 && r.bytes[r.pos] == 0 {
    r.pos += 1;
  }
  if r.remaining() != 0 {
    return Err(Error::Invalid("mode stream was not fully consumed".into()));
  }
  Ok(base_mbs)
}

#[inline(always)]
fn skip_mode_payload(mode: u8, r: &mut Reader<'_>) -> Result<()> {
  match mode {
    MODE_BASE_RES | MODE_RAW_MB => Ok(()),
    MODE_COPY16 | MODE_COPY16_RES => r.skip(2),
    MODE_COPY16X8 | MODE_COPY16X8_RES | MODE_COPY8X16 | MODE_COPY8X16_RES => {
      r.skip(4)
    }
    MODE_COPY8X8 | MODE_COPY8X8_RES => r.skip(8),
    _ => Err(Error::Unsupported(format!("MB mode {mode}"))),
  }
}

#[inline(always)]
fn decode_one_mb(
  mode: u8, mb_index: usize, base_copy_mode: BaseCopyMode,
  mode_stream: &mut Reader<'_>, residual: &mut Reader<'_>,
  raw: &mut Reader<'_>, reference: &EyeFrame, current: &mut EyeFrame,
) -> Result<()> {
  match mode {
    MODE_BASE_RES => {
      if let BaseCopyMode::Lazy(base_mv) = base_copy_mode {
        copy_mb_from_reference(current, reference, mb_index, base_mv);
      }
      apply_mb_residual(current, mb_index, residual)
    }
    MODE_COPY16 => {
      let mv = Mv { x: mode_stream.i8()?, y: mode_stream.i8()? };
      copy_mb_from_reference(current, reference, mb_index, mv);
      Ok(())
    }
    MODE_COPY16_RES => {
      let mv = Mv { x: mode_stream.i8()?, y: mode_stream.i8()? };
      copy_mb_from_reference(current, reference, mb_index, mv);
      apply_mb_residual(current, mb_index, residual)
    }
    MODE_COPY16X8 => {
      let mvs = [read_mv(mode_stream)?, read_mv(mode_stream)?];
      copy_vbs_from_reference(
        current,
        reference,
        mb_index,
        VbsShape::Split16x8,
        &mvs,
      );
      Ok(())
    }
    MODE_COPY16X8_RES => {
      let mvs = [read_mv(mode_stream)?, read_mv(mode_stream)?];
      copy_vbs_from_reference(
        current,
        reference,
        mb_index,
        VbsShape::Split16x8,
        &mvs,
      );
      apply_mb_residual(current, mb_index, residual)
    }
    MODE_COPY8X16 => {
      let mvs = [read_mv(mode_stream)?, read_mv(mode_stream)?];
      copy_vbs_from_reference(
        current,
        reference,
        mb_index,
        VbsShape::Split8x16,
        &mvs,
      );
      Ok(())
    }
    MODE_COPY8X16_RES => {
      let mvs = [read_mv(mode_stream)?, read_mv(mode_stream)?];
      copy_vbs_from_reference(
        current,
        reference,
        mb_index,
        VbsShape::Split8x16,
        &mvs,
      );
      apply_mb_residual(current, mb_index, residual)
    }
    MODE_COPY8X8 => {
      let mvs = [
        read_mv(mode_stream)?,
        read_mv(mode_stream)?,
        read_mv(mode_stream)?,
        read_mv(mode_stream)?,
      ];
      copy_vbs_from_reference(
        current,
        reference,
        mb_index,
        VbsShape::Split8x8,
        &mvs,
      );
      Ok(())
    }
    MODE_COPY8X8_RES => {
      let mvs = [
        read_mv(mode_stream)?,
        read_mv(mode_stream)?,
        read_mv(mode_stream)?,
        read_mv(mode_stream)?,
      ];
      copy_vbs_from_reference(
        current,
        reference,
        mb_index,
        VbsShape::Split8x8,
        &mvs,
      );
      apply_mb_residual(current, mb_index, residual)
    }
    MODE_RAW_MB => {
      let bytes = raw.take(RAW_MB_BYTES)?;
      write_raw_mb(current, mb_index, bytes);
      Ok(())
    }
    _ => Err(Error::Unsupported(format!("MB mode {mode}"))),
  }
}

#[inline(always)]
fn read_mv(r: &mut Reader<'_>) -> Result<Mv> {
  Ok(Mv { x: r.i8()?, y: r.i8()? })
}

#[inline(always)]
fn apply_mb_residual(
  current: &mut EyeFrame, mb_index: usize, residual: &mut Reader<'_>,
) -> Result<()> {
  let mask = residual.u32()?;
  if (mask & 0xff00_0000) != 0 {
    return Err(Error::Invalid("coded block mask uses reserved bits".into()));
  }

  let mb_x = mb_index % MB_W;
  let mb_y = mb_index / MB_W;
  let y_base_x = mb_x * 16;
  let y_base_y = mb_y * 16;
  let c_base_x = mb_x * 8;
  let c_base_y = mb_y * 8;
  let mut coded = mask;
  while coded != 0 {
    let block = coded.trailing_zeros() as usize;
    coded &= coded - 1;
    if block < 16 {
      let bx = y_base_x + (block & 3) * 4;
      let by = y_base_y + (block >> 2) * 4;
      apply_block(&mut current.y, EYE_W, bx, by, residual)?;
    } else if block < 20 {
      let cblock = block - 16;
      let bx = c_base_x + (cblock & 1) * 4;
      let by = c_base_y + (cblock >> 1) * 4;
      apply_block(&mut current.cb, CHROMA_W, bx, by, residual)?;
    } else {
      let cblock = block - 20;
      let bx = c_base_x + (cblock & 1) * 4;
      let by = c_base_y + (cblock >> 1) * 4;
      apply_block(&mut current.cr, CHROMA_W, bx, by, residual)?;
    }
  }
  Ok(())
}

#[inline(always)]
fn apply_block(
  plane: &mut [u8], stride: usize, x: usize, y: usize,
  residual: &mut Reader<'_>,
) -> Result<()> {
  let tag = residual.u8()?;
  match tag & 0xc0 {
    0x00 => {
      if tag != TAG_DC_ONLY_S8 {
        return Err(Error::Invalid("reserved DC_ONLY_S8 tag bits set".into()));
      }
      let coeff = residual.i8()? as i32;
      add_dc_block(plane, stride, x, y, coeff);
    }
    0x40 => {
      if tag != TAG_DC_ONLY_S16 {
        return Err(Error::Invalid(
          "reserved DC_ONLY_S16 tag bits set".into(),
        ));
      }
      let coeff = residual.i16()? as i32;
      add_dc_block(plane, stride, x, y, coeff);
    }
    0x80 => {
      if tag != TAG_AC_MASK_S8 {
        return Err(Error::Invalid("reserved AC_MASK_S8 tag bits set".into()));
      }
      let mut coeffs = [0i32; 16];
      let nz_mask = residual.u16()?;
      let mut nz = nz_mask;
      while nz != 0 {
        let zz = nz.trailing_zeros() as usize;
        nz &= nz - 1;
        coeffs[ZIGZAG[zz]] = residual.i8()? as i32;
      }
      add_idct_block(plane, stride, x, y, coeffs);
    }
    0xc0 => {
      if (tag & 0x20) == 0 {
        if tag != TAG_AC_MASK_S16 {
          return Err(Error::Invalid(
            "reserved AC_MASK_S16 tag bits set".into(),
          ));
        }
        let mut coeffs = [0i32; 16];
        let nz_mask = residual.u16()?;
        let mut nz = nz_mask;
        while nz != 0 {
          let zz = nz.trailing_zeros() as usize;
          nz &= nz - 1;
          coeffs[ZIGZAG[zz]] = residual.i16()? as i32;
        }
        add_idct_block(plane, stride, x, y, coeffs);
      } else {
        if tag != TAG_RAW_4X4 {
          return Err(Error::Invalid("reserved RAW_4X4 tag bits set".into()));
        }
        let samples = residual.take(16)?;
        for row in 0..4 {
          let dst = (y + row) * stride + x;
          plane[dst..dst + 4].copy_from_slice(&samples[row * 4..row * 4 + 4]);
        }
      }
    }
    _ => unreachable!(),
  }
  Ok(())
}

#[inline(always)]
fn add_dc_block(
  plane: &mut [u8], stride: usize, x: usize, y: usize, coeff: i32,
) {
  let delta = (coeff + 4) >> 3;
  if delta == 0 {
    return;
  }

  let base = y * stride + x;
  if (DC_CLIP_DELTA_MIN..=DC_CLIP_DELTA_MAX).contains(&delta) {
    let table = &DC_CLIP_TABLE[(delta - DC_CLIP_DELTA_MIN) as usize];
    add_dc_row4_table(plane, base, table);
    add_dc_row4_table(plane, base + stride, table);
    add_dc_row4_table(plane, base + stride * 2, table);
    add_dc_row4_table(plane, base + stride * 3, table);
  } else {
    add_dc_row4(plane, base, delta);
    add_dc_row4(plane, base + stride, delta);
    add_dc_row4(plane, base + stride * 2, delta);
    add_dc_row4(plane, base + stride * 3, delta);
  }
}

#[inline(always)]
fn add_dc_row4_table(plane: &mut [u8], idx: usize, table: &[u8; 256]) {
  plane[idx] = table[plane[idx] as usize];
  plane[idx + 1] = table[plane[idx + 1] as usize];
  plane[idx + 2] = table[plane[idx + 2] as usize];
  plane[idx + 3] = table[plane[idx + 3] as usize];
}

#[inline(always)]
fn add_dc_row4(plane: &mut [u8], idx: usize, delta: i32) {
  plane[idx] = clip_u8(plane[idx] as i32 + delta);
  plane[idx + 1] = clip_u8(plane[idx + 1] as i32 + delta);
  plane[idx + 2] = clip_u8(plane[idx + 2] as i32 + delta);
  plane[idx + 3] = clip_u8(plane[idx + 3] as i32 + delta);
}

#[inline(always)]
fn add_idct_block(
  plane: &mut [u8], stride: usize, x: usize, y: usize, coeffs: [i32; 16],
) {
  let (t0, t4, t8, t12) = idct_1d(coeffs[0], coeffs[4], coeffs[8], coeffs[12]);
  let (t1, t5, t9, t13) = idct_1d(coeffs[1], coeffs[5], coeffs[9], coeffs[13]);
  let (t2, t6, t10, t14) =
    idct_1d(coeffs[2], coeffs[6], coeffs[10], coeffs[14]);
  let (t3, t7, t11, t15) =
    idct_1d(coeffs[3], coeffs[7], coeffs[11], coeffs[15]);

  let (v0, v1, v2, v3) = idct_1d(t0, t1, t2, t3);
  add_idct_row(plane, y * stride + x, v0, v1, v2, v3);
  let (v0, v1, v2, v3) = idct_1d(t4, t5, t6, t7);
  add_idct_row(plane, (y + 1) * stride + x, v0, v1, v2, v3);
  let (v0, v1, v2, v3) = idct_1d(t8, t9, t10, t11);
  add_idct_row(plane, (y + 2) * stride + x, v0, v1, v2, v3);
  let (v0, v1, v2, v3) = idct_1d(t12, t13, t14, t15);
  add_idct_row(plane, (y + 3) * stride + x, v0, v1, v2, v3);
}

#[inline(always)]
fn idct_1d(c0: i32, c1: i32, c2: i32, c3: i32) -> (i32, i32, i32, i32) {
  let a1 = c0 + c2;
  let b1 = c0 - c2;
  let c1v = ((c1 * 35468) >> 16) - c3;
  let d1 = c1 + ((c3 * 35468) >> 16);
  (a1 + d1, b1 + c1v, b1 - c1v, a1 - d1)
}

#[inline(always)]
fn add_idct_row(
  plane: &mut [u8], idx: usize, v0: i32, v1: i32, v2: i32, v3: i32,
) {
  plane[idx] = clip_u8(plane[idx] as i32 + ((v0 + 4) >> 3));
  plane[idx + 1] = clip_u8(plane[idx + 1] as i32 + ((v1 + 4) >> 3));
  plane[idx + 2] = clip_u8(plane[idx + 2] as i32 + ((v2 + 4) >> 3));
  plane[idx + 3] = clip_u8(plane[idx + 3] as i32 + ((v3 + 4) >> 3));
}

pub fn prefill_eye(dst: &mut EyeFrame, src: &EyeFrame, mv: Mv) {
  if mv == Mv::ZERO {
    dst.y.copy_from_slice(&src.y);
    dst.cb.copy_from_slice(&src.cb);
    dst.cr.copy_from_slice(&src.cr);
    return;
  }

  motion_copy_plane(
    &mut dst.y,
    &src.y,
    EYE_W,
    EYE_H,
    mv.x as i32,
    mv.y as i32,
  );
  motion_copy_plane(
    &mut dst.cb,
    &src.cb,
    CHROMA_W,
    CHROMA_H,
    (mv.x as i32) >> 1,
    (mv.y as i32) >> 1,
  );
  motion_copy_plane(
    &mut dst.cr,
    &src.cr,
    CHROMA_W,
    CHROMA_H,
    (mv.x as i32) >> 1,
    (mv.y as i32) >> 1,
  );
}

#[inline(always)]
pub fn copy_mb_from_reference(
  dst: &mut EyeFrame, src: &EyeFrame, mb_index: usize, mv: Mv,
) {
  let mb_x = mb_index % MB_W;
  let mb_y = mb_index / MB_W;
  copy_rect(
    &mut dst.y,
    &src.y,
    EYE_W,
    EYE_H,
    mb_x * 16,
    mb_y * 16,
    16,
    16,
    mv.x as i32,
    mv.y as i32,
  );
  copy_rect(
    &mut dst.cb,
    &src.cb,
    CHROMA_W,
    CHROMA_H,
    mb_x * 8,
    mb_y * 8,
    8,
    8,
    (mv.x as i32) >> 1,
    (mv.y as i32) >> 1,
  );
  copy_rect(
    &mut dst.cr,
    &src.cr,
    CHROMA_W,
    CHROMA_H,
    mb_x * 8,
    mb_y * 8,
    8,
    8,
    (mv.x as i32) >> 1,
    (mv.y as i32) >> 1,
  );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VbsShape {
  Split16x8,
  Split8x16,
  Split8x8,
}

#[inline(always)]
pub fn copy_vbs_from_reference(
  dst: &mut EyeFrame, src: &EyeFrame, mb_index: usize, shape: VbsShape,
  mvs: &[Mv],
) {
  let mb_x = mb_index % MB_W;
  let mb_y = mb_index / MB_W;
  let base_x = mb_x * 16;
  let base_y = mb_y * 16;

  match shape {
    VbsShape::Split16x8 => {
      copy_rect(
        &mut dst.y,
        &src.y,
        EYE_W,
        EYE_H,
        base_x,
        base_y,
        16,
        8,
        mvs[0].x as i32,
        mvs[0].y as i32,
      );
      copy_rect(
        &mut dst.y,
        &src.y,
        EYE_W,
        EYE_H,
        base_x,
        base_y + 8,
        16,
        8,
        mvs[1].x as i32,
        mvs[1].y as i32,
      );
    }
    VbsShape::Split8x16 => {
      copy_rect(
        &mut dst.y,
        &src.y,
        EYE_W,
        EYE_H,
        base_x,
        base_y,
        8,
        16,
        mvs[0].x as i32,
        mvs[0].y as i32,
      );
      copy_rect(
        &mut dst.y,
        &src.y,
        EYE_W,
        EYE_H,
        base_x + 8,
        base_y,
        8,
        16,
        mvs[1].x as i32,
        mvs[1].y as i32,
      );
    }
    VbsShape::Split8x8 => {
      for part_y in 0..2 {
        for part_x in 0..2 {
          let index = part_y * 2 + part_x;
          copy_rect(
            &mut dst.y,
            &src.y,
            EYE_W,
            EYE_H,
            base_x + part_x * 8,
            base_y + part_y * 8,
            8,
            8,
            mvs[index].x as i32,
            mvs[index].y as i32,
          );
        }
      }
    }
  }

  let (avg_x, avg_y) = average_mv(mvs);
  copy_rect(
    &mut dst.cb,
    &src.cb,
    CHROMA_W,
    CHROMA_H,
    mb_x * 8,
    mb_y * 8,
    8,
    8,
    avg_x >> 1,
    avg_y >> 1,
  );
  copy_rect(
    &mut dst.cr,
    &src.cr,
    CHROMA_W,
    CHROMA_H,
    mb_x * 8,
    mb_y * 8,
    8,
    8,
    avg_x >> 1,
    avg_y >> 1,
  );
}

#[inline(always)]
fn average_mv(mvs: &[Mv]) -> (i32, i32) {
  let mut x = 0i32;
  let mut y = 0i32;
  for mv in mvs {
    x += mv.x as i32;
    y += mv.y as i32;
  }
  let n = mvs.len() as i32;
  (round_div_i32(x, n), round_div_i32(y, n))
}

#[inline(always)]
fn round_div_i32(value: i32, divisor: i32) -> i32 {
  if value >= 0 {
    (value + divisor / 2) / divisor
  } else {
    -((-value + divisor / 2) / divisor)
  }
}

fn motion_copy_plane(
  dst: &mut [u8], src: &[u8], w: usize, h: usize, mv_x: i32, mv_y: i32,
) {
  if mv_x == 0 && mv_y == 0 {
    dst.copy_from_slice(src);
    return;
  }

  for y in 0..h {
    let sy = clamp_i32(y as i32 + mv_y, 0, h as i32 - 1) as usize;
    let dst_row = &mut dst[y * w..(y + 1) * w];
    let src_row = &src[sy * w..(sy + 1) * w];
    copy_shifted_row(dst_row, src_row, mv_x);
  }
}

fn copy_shifted_row(dst: &mut [u8], src: &[u8], mv_x: i32) {
  debug_assert_eq!(dst.len(), src.len());
  let w = dst.len();
  if mv_x == 0 {
    dst.copy_from_slice(src);
  } else if mv_x > 0 {
    let shift = (mv_x as usize).min(w);
    let copy_len = w - shift;
    if copy_len > 0 {
      dst[..copy_len].copy_from_slice(&src[shift..]);
    }
    dst[copy_len..].fill(src[w - 1]);
  } else {
    let shift = ((-mv_x) as usize).min(w);
    dst[..shift].fill(src[0]);
    if shift < w {
      dst[shift..].copy_from_slice(&src[..w - shift]);
    }
  }
}

#[inline(always)]
fn copy_rect(
  dst: &mut [u8], src: &[u8], w: usize, h: usize, dst_x: usize, dst_y: usize,
  bw: usize, bh: usize, mv_x: i32, mv_y: i32,
) {
  if mv_x == 0 && mv_y == 0 {
    if bw == 16 {
      for row in 0..bh {
        let off = (dst_y + row) * w + dst_x;
        copy_16(&mut dst[off..off + 16], &src[off..off + 16]);
      }
    } else if bw == 8 {
      for row in 0..bh {
        let off = (dst_y + row) * w + dst_x;
        copy_8(&mut dst[off..off + 8], &src[off..off + 8]);
      }
    } else {
      for row in 0..bh {
        let off = (dst_y + row) * w + dst_x;
        dst[off..off + bw].copy_from_slice(&src[off..off + bw]);
      }
    }
    return;
  }

  let src_x = dst_x as i32 + mv_x;
  let src_y = dst_y as i32 + mv_y;
  if src_x >= 0
    && src_y >= 0
    && src_x as usize + bw <= w
    && src_y as usize + bh <= h
  {
    let src_x = src_x as usize;
    let src_y = src_y as usize;
    if bw == 16 {
      for row in 0..bh {
        let dst_off = (dst_y + row) * w + dst_x;
        let src_off = (src_y + row) * w + src_x;
        copy_16(&mut dst[dst_off..dst_off + 16], &src[src_off..src_off + 16]);
      }
    } else if bw == 8 {
      for row in 0..bh {
        let dst_off = (dst_y + row) * w + dst_x;
        let src_off = (src_y + row) * w + src_x;
        copy_8(&mut dst[dst_off..dst_off + 8], &src[src_off..src_off + 8]);
      }
    } else {
      for row in 0..bh {
        let dst_off = (dst_y + row) * w + dst_x;
        let src_off = (src_y + row) * w + src_x;
        for col in 0..bw {
          dst[dst_off + col] = src[src_off + col];
        }
      }
    }
    return;
  }

  for row in 0..bh {
    for col in 0..bw {
      let x = dst_x + col;
      let y = dst_y + row;
      let sx = clamp_i32(x as i32 + mv_x, 0, w as i32 - 1) as usize;
      let sy = clamp_i32(y as i32 + mv_y, 0, h as i32 - 1) as usize;
      dst[y * w + x] = src[sy * w + sx];
    }
  }
}

#[inline(always)]
fn copy_8(dst: &mut [u8], src: &[u8]) {
  dst[0] = src[0];
  dst[1] = src[1];
  dst[2] = src[2];
  dst[3] = src[3];
  dst[4] = src[4];
  dst[5] = src[5];
  dst[6] = src[6];
  dst[7] = src[7];
}

#[inline(always)]
fn copy_16(dst: &mut [u8], src: &[u8]) {
  dst[0] = src[0];
  dst[1] = src[1];
  dst[2] = src[2];
  dst[3] = src[3];
  dst[4] = src[4];
  dst[5] = src[5];
  dst[6] = src[6];
  dst[7] = src[7];
  dst[8] = src[8];
  dst[9] = src[9];
  dst[10] = src[10];
  dst[11] = src[11];
  dst[12] = src[12];
  dst[13] = src[13];
  dst[14] = src[14];
  dst[15] = src[15];
}

#[inline(always)]
fn write_raw_mb(current: &mut EyeFrame, mb_index: usize, bytes: &[u8]) {
  let mb_x = mb_index % MB_W;
  let mb_y = mb_index / MB_W;
  let mut off = 0;
  for row in 0..16 {
    let dst = (mb_y * 16 + row) * EYE_W + mb_x * 16;
    copy_16(&mut current.y[dst..dst + 16], &bytes[off..off + 16]);
    off += 16;
  }
  for row in 0..8 {
    let dst = (mb_y * 8 + row) * CHROMA_W + mb_x * 8;
    copy_8(&mut current.cb[dst..dst + 8], &bytes[off..off + 8]);
    off += 8;
  }
  for row in 0..8 {
    let dst = (mb_y * 8 + row) * CHROMA_W + mb_x * 8;
    copy_8(&mut current.cr[dst..dst + 8], &bytes[off..off + 8]);
    off += 8;
  }
}

pub fn read_raw_mb(eye: &EyeFrame, mb_index: usize, out: &mut Vec<u8>) {
  let mb_x = mb_index % MB_W;
  let mb_y = mb_index / MB_W;
  for row in 0..16 {
    let src = (mb_y * 16 + row) * EYE_W + mb_x * 16;
    out.extend_from_slice(&eye.y[src..src + 16]);
  }
  for row in 0..8 {
    let src = (mb_y * 8 + row) * CHROMA_W + mb_x * 8;
    out.extend_from_slice(&eye.cb[src..src + 8]);
  }
  for row in 0..8 {
    let src = (mb_y * 8 + row) * CHROMA_W + mb_x * 8;
    out.extend_from_slice(&eye.cr[src..src + 8]);
  }
}

fn write_eye_raw(out: &mut Vec<u8>, eye: &EyeFrame) {
  out.extend_from_slice(&eye.y);
  out.extend_from_slice(&eye.cb);
  out.extend_from_slice(&eye.cr);
}

fn read_eye_raw_into(dst: &mut EyeFrame, bytes: &[u8]) -> Result<()> {
  if bytes.len() != EYE_FRAME_BYTES {
    return Err(Error::Invalid("bad eye raw size".into()));
  }
  let y_len = EYE_W * EYE_H;
  let c_len = CHROMA_W * CHROMA_H;
  dst.y.copy_from_slice(&bytes[..y_len]);
  dst.cb.copy_from_slice(&bytes[y_len..y_len + c_len]);
  dst.cr.copy_from_slice(&bytes[y_len + c_len..]);
  Ok(())
}

fn clip_u8(v: i32) -> u8 {
  v.clamp(0, 255) as u8
}

const fn build_dc_clip_table() -> [[u8; 256]; 33] {
  let mut table = [[0u8; 256]; 33];
  let mut delta = DC_CLIP_DELTA_MIN;
  while delta <= DC_CLIP_DELTA_MAX {
    let mut sample = 0usize;
    while sample < 256 {
      let value = sample as i32 + delta;
      table[(delta - DC_CLIP_DELTA_MIN) as usize][sample] = if value < 0 {
        0
      } else if value > 255 {
        255
      } else {
        value as u8
      };
      sample += 1;
    }
    delta += 1;
  }
  table
}

fn clamp_i32(v: i32, lo: i32, hi: i32) -> i32 {
  v.max(lo).min(hi)
}

fn put_u16(out: &mut Vec<u8>, v: u16) {
  out.extend_from_slice(&v.to_le_bytes());
}

pub fn put_u32(out: &mut Vec<u8>, v: u32) {
  out.extend_from_slice(&v.to_le_bytes());
}

fn put_u64(out: &mut Vec<u8>, v: u64) {
  out.extend_from_slice(&v.to_le_bytes());
}

#[derive(Clone)]
struct Reader<'a> {
  bytes: &'a [u8],
  pos: usize,
}

impl<'a> Reader<'a> {
  #[inline(always)]
  fn new(bytes: &'a [u8]) -> Self {
    Self { bytes, pos: 0 }
  }

  #[inline(always)]
  fn remaining(&self) -> usize {
    self.bytes.len() - self.pos
  }

  #[inline(always)]
  fn take(&mut self, n: usize) -> Result<&'a [u8]> {
    if n > self.remaining() {
      return Err(Error::Eof);
    }
    let start = self.pos;
    self.pos += n;
    Ok(&self.bytes[start..start + n])
  }

  #[inline(always)]
  fn skip(&mut self, n: usize) -> Result<()> {
    self.take(n).map(|_| ())
  }

  #[inline(always)]
  fn u8(&mut self) -> Result<u8> {
    if self.pos >= self.bytes.len() {
      return Err(Error::Eof);
    }
    let value = self.bytes[self.pos];
    self.pos += 1;
    Ok(value)
  }

  #[inline(always)]
  fn i8(&mut self) -> Result<i8> {
    Ok(self.u8()? as i8)
  }

  #[inline(always)]
  fn u16(&mut self) -> Result<u16> {
    let end = self.pos + 2;
    if end > self.bytes.len() {
      return Err(Error::Eof);
    }
    let start = self.pos;
    self.pos = end;
    Ok(u16::from_le_bytes([self.bytes[start], self.bytes[start + 1]]))
  }

  #[inline(always)]
  fn i16(&mut self) -> Result<i16> {
    let end = self.pos + 2;
    if end > self.bytes.len() {
      return Err(Error::Eof);
    }
    let start = self.pos;
    self.pos = end;
    Ok(i16::from_le_bytes([self.bytes[start], self.bytes[start + 1]]))
  }

  #[inline(always)]
  fn u32(&mut self) -> Result<u32> {
    let end = self.pos + 4;
    if end > self.bytes.len() {
      return Err(Error::Eof);
    }
    let start = self.pos;
    self.pos = end;
    Ok(u32::from_le_bytes([
      self.bytes[start],
      self.bytes[start + 1],
      self.bytes[start + 2],
      self.bytes[start + 3],
    ]))
  }

  #[inline(always)]
  fn u64(&mut self) -> Result<u64> {
    let end = self.pos + 8;
    if end > self.bytes.len() {
      return Err(Error::Eof);
    }
    let start = self.pos;
    self.pos = end;
    Ok(u64::from_le_bytes([
      self.bytes[start],
      self.bytes[start + 1],
      self.bytes[start + 2],
      self.bytes[start + 3],
      self.bytes[start + 4],
      self.bytes[start + 5],
      self.bytes[start + 6],
      self.bytes[start + 7],
    ]))
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn patterned_frame(seed: u8) -> SbsFrame {
    let mut f = SbsFrame::new();
    for y in 0..EYE_H {
      for x in 0..EYE_W {
        f.left.y[y * EYE_W + x] =
          seed.wrapping_add(x as u8).wrapping_add((y * 3) as u8);
        f.right.y[y * EYE_W + x] = seed
          .wrapping_add(17)
          .wrapping_add((x * 2) as u8)
          .wrapping_add(y as u8);
      }
    }
    for y in 0..CHROMA_H {
      for x in 0..CHROMA_W {
        f.left.cb[y * CHROMA_W + x] =
          90u8.wrapping_add(seed).wrapping_add(x as u8);
        f.left.cr[y * CHROMA_W + x] =
          130u8.wrapping_add(seed).wrapping_add(y as u8);
        f.right.cb[y * CHROMA_W + x] =
          110u8.wrapping_add(seed).wrapping_add(y as u8);
        f.right.cr[y * CHROMA_W + x] =
          150u8.wrapping_add(seed).wrapping_add(x as u8);
      }
    }
    f
  }

  fn p_with_tiles(
    left_modes: Vec<u8>, left_res: Vec<u8>, left_raw: Vec<u8>,
  ) -> Vec<u8> {
    let mut bytes = Vec::new();
    write_file_header(&mut bytes, 2);
    let key = patterned_frame(3);
    write_key_raw_frame(&mut bytes, 0, &key);
    let left = EncodedTile {
      tile_id: 0,
      base_mv: Mv::ZERO,
      segment_count: 1,
      fragments: vec![EncodedFragment::full_eye(
        0, left_modes, left_res, left_raw,
      )],
    };
    let right = EncodedTile {
      tile_id: 1,
      base_mv: Mv::ZERO,
      segment_count: 1,
      fragments: vec![EncodedFragment::full_eye(
        1,
        vec![0x7f, 0x7f, 0x79, 0],
        vec![],
        vec![],
      )],
    };
    write_p_frame(&mut bytes, 1, left, right);
    bytes
  }

  fn all_skip_tile(tile_id: u8) -> EncodedTile {
    EncodedTile {
      tile_id,
      base_mv: Mv::ZERO,
      segment_count: 1,
      fragments: vec![EncodedFragment::full_eye(
        tile_id,
        vec![0x7f, 0x7f, 0x79, 0],
        vec![],
        vec![],
      )],
    }
  }

  #[test]
  fn key_raw_round_trips() {
    let frame = patterned_frame(9);
    let mut bytes = Vec::new();
    write_file_header(&mut bytes, 1);
    write_key_raw_frame(&mut bytes, 0, &frame);
    assert_eq!(decode_stream(&bytes).unwrap(), vec![frame]);
  }

  #[test]
  fn skipped_p_frame_round_trips() {
    let key = patterned_frame(1);
    let mut bytes = Vec::new();
    write_file_header(&mut bytes, 2);
    write_key_raw_frame(&mut bytes, 0, &key);
    write_p_frame(&mut bytes, 1, all_skip_tile(0), all_skip_tile(1));
    let frames = decode_stream(&bytes).unwrap();
    assert_eq!(frames[0], key);
    assert_eq!(frames[1], key);
  }

  #[test]
  fn reusable_decoder_state_resets_between_streams() {
    let key = patterned_frame(1);
    let mut bytes = Vec::new();
    write_file_header(&mut bytes, 2);
    write_key_raw_frame(&mut bytes, 0, &key);
    write_p_frame(&mut bytes, 1, all_skip_tile(0), all_skip_tile(1));

    let mut state = DecoderState::new();
    let mut frames = Vec::new();
    let decoded =
      decode_stream_for_each_with_state(&bytes, &mut state, |decoded| {
        frames.push(decoded.frame.clone());
      })
      .unwrap();
    assert_eq!(decoded, 2);
    assert_eq!(frames[1], key);

    let mut p_without_reference = Vec::new();
    write_file_header(&mut p_without_reference, 1);
    write_p_frame(
      &mut p_without_reference,
      0,
      all_skip_tile(0),
      all_skip_tile(1),
    );
    assert!(decode_stream_for_each_with_state(
      &p_without_reference,
      &mut state,
      |_| {},
    )
    .is_err());
  }

  #[test]
  fn skipped_p_frame_keeps_reference_for_following_p() {
    let key = patterned_frame(1);
    let mut bytes = Vec::new();
    write_file_header(&mut bytes, 3);
    write_key_raw_frame(&mut bytes, 0, &key);
    write_p_frame(&mut bytes, 1, all_skip_tile(0), all_skip_tile(1));

    let mut residual = Vec::new();
    put_u32(&mut residual, 0x0000_0001);
    residual.push(TAG_DC_ONLY_S8);
    residual.push(5 * 8);
    let left = EncodedTile {
      tile_id: 0,
      base_mv: Mv::ZERO,
      segment_count: 1,
      fragments: vec![EncodedFragment::full_eye(
        0,
        vec![0x80 | MODE_BASE_RES, 0x7f, 0x7f, 0x78, 0],
        residual,
        vec![],
      )],
    };
    write_p_frame(&mut bytes, 2, left, all_skip_tile(1));

    let frames = decode_stream(&bytes).unwrap();
    let mut expected = key.clone();
    add_dc_block(&mut expected.left.y, EYE_W, 0, 0, 5 * 8);

    assert_eq!(frames[1], key);
    assert_eq!(frames[2], expected);
  }

  #[test]
  fn all_skip_tile_with_residual_bytes_is_rejected() {
    let bytes =
      p_with_tiles(vec![0x7f, 0x7f, 0x79, 0], vec![0, 0, 0, 0], vec![]);
    assert!(decode_stream(&bytes).is_err());
  }

  #[test]
  fn skipped_p_frame_with_base_motion_round_trips() {
    let key = patterned_frame(2);
    let mut bytes = Vec::new();
    write_file_header(&mut bytes, 2);
    write_key_raw_frame(&mut bytes, 0, &key);
    let left = EncodedTile {
      tile_id: 0,
      base_mv: Mv { x: 1, y: 0 },
      segment_count: 1,
      fragments: vec![EncodedFragment::full_eye(
        0,
        vec![0x7f, 0x7f, 0x79, 0],
        vec![],
        vec![],
      )],
    };
    let right = EncodedTile {
      tile_id: 1,
      base_mv: Mv { x: -1, y: 0 },
      segment_count: 1,
      fragments: vec![EncodedFragment::full_eye(
        1,
        vec![0x7f, 0x7f, 0x79, 0],
        vec![],
        vec![],
      )],
    };
    write_p_frame(&mut bytes, 1, left, right);
    let frames = decode_stream(&bytes).unwrap();
    let mut expected = SbsFrame::new();
    prefill_eye(&mut expected.left, &key.left, Mv { x: 1, y: 0 });
    prefill_eye(&mut expected.right, &key.right, Mv { x: -1, y: 0 });
    assert_eq!(frames[1], expected);
  }

  #[test]
  fn sparse_base_copy_tile_round_trips() {
    let key = patterned_frame(4);
    let mut bytes = Vec::new();
    write_file_header(&mut bytes, 2);
    write_key_raw_frame(&mut bytes, 0, &key);

    let mut left_modes = vec![1];
    for _ in 1..MB_COUNT {
      left_modes.push(0x80 | MODE_COPY16);
      left_modes.extend_from_slice(&[0, 0]);
    }
    left_modes.push(0);

    let left = EncodedTile {
      tile_id: 0,
      base_mv: Mv { x: 1, y: 0 },
      segment_count: 1,
      fragments: vec![EncodedFragment::full_eye(
        0,
        left_modes,
        vec![],
        vec![],
      )],
    };
    let right = EncodedTile {
      tile_id: 1,
      base_mv: Mv::ZERO,
      segment_count: 1,
      fragments: vec![EncodedFragment::full_eye(
        1,
        vec![0x7f, 0x7f, 0x79, 0],
        vec![],
        vec![],
      )],
    };
    write_p_frame(&mut bytes, 1, left, right);

    let p_payload_start = FILE_HEADER_SIZE as usize
      + FRAME_HEADER_SIZE
      + SBS_FRAME_BYTES
      + FRAME_HEADER_SIZE;
    let tile_flags = u16::from_le_bytes([
      bytes[p_payload_start + 11],
      bytes[p_payload_start + 12],
    ]);
    assert_eq!(
      tile_flags & TILE_FLAG_LAZY_BASE_COPY,
      TILE_FLAG_LAZY_BASE_COPY
    );

    let frames = decode_stream(&bytes).unwrap();
    let mut expected = key.clone();
    copy_mb_from_reference(
      &mut expected.left,
      &key.left,
      0,
      Mv { x: 1, y: 0 },
    );
    assert_eq!(frames[1], expected);
  }

  #[test]
  fn decoded_metadata_marks_raw_and_p_frames() {
    let frames = decode_stream_with_metadata(&p_with_tiles(
      vec![0x7f, 0x7f, 0x79, 0],
      vec![],
      vec![],
    ))
    .unwrap();

    assert_eq!(frames[0].frame_no, 0);
    assert_eq!(frames[0].frame_type, FRAME_TYPE_KEY_RAW);
    assert_eq!(frames[1].frame_no, 1);
    assert_eq!(frames[1].frame_type, FRAME_TYPE_P);
  }

  #[test]
  fn copy16_motion_round_trips() {
    let bytes = p_with_tiles(
      vec![0x80 | MODE_COPY16, 1, 0, 0x7f, 0x7f, 0x78, 0],
      vec![],
      vec![],
    );
    let frames = decode_stream(&bytes).unwrap();
    for row in 0..16 {
      for col in 0..16 {
        assert_eq!(
          frames[1].left.y[row * EYE_W + col],
          frames[0].left.y[row * EYE_W + col + 1]
        );
      }
    }
  }

  #[test]
  fn dc_residual_round_trips() {
    let mut res = Vec::new();
    put_u32(&mut res, 0x0000_ffff);
    for _ in 0..16 {
      res.push(TAG_DC_ONLY_S8);
      res.push(5 * 8);
    }
    let bytes = p_with_tiles(
      vec![0x80 | MODE_BASE_RES, 0x7f, 0x7f, 0x78, 0],
      res,
      vec![],
    );
    let frames = decode_stream(&bytes).unwrap();
    for row in 0..16 {
      for col in 0..16 {
        let before = frames[0].left.y[row * EYE_W + col] as i32;
        assert_eq!(frames[1].left.y[row * EYE_W + col], clip_u8(before + 5));
      }
    }
  }

  #[test]
  fn dc_fast_path_matches_idct_rounding() {
    for coeff in -64..=64 {
      let mut dc_plane = vec![91u8; 8 * 8];
      let mut idct_plane = dc_plane.clone();
      add_dc_block(&mut dc_plane, 8, 2, 2, coeff);
      add_idct_block(
        &mut idct_plane,
        8,
        2,
        2,
        [coeff, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
      );
      assert_eq!(dc_plane, idct_plane);
    }
  }

  #[test]
  fn dc_clip_table_matches_saturating_clip() {
    for delta in DC_CLIP_DELTA_MIN..=DC_CLIP_DELTA_MAX {
      let table = &DC_CLIP_TABLE[(delta - DC_CLIP_DELTA_MIN) as usize];
      for sample in 0..=255 {
        assert_eq!(table[sample], clip_u8(sample as i32 + delta));
      }
    }
  }

  #[test]
  fn ac_mask_residual_round_trips() {
    let mut res = Vec::new();
    put_u32(&mut res, 0x0000_0001);
    res.push(TAG_AC_MASK_S8);
    res.extend_from_slice(&1u16.to_le_bytes());
    res.push(5 * 8);

    let bytes = p_with_tiles(
      vec![0x80 | MODE_BASE_RES, 0x7f, 0x7f, 0x78, 0],
      res,
      vec![],
    );
    let frames = decode_stream(&bytes).unwrap();
    for row in 0..4 {
      for col in 0..4 {
        let before = frames[0].left.y[row * EYE_W + col] as i32;
        assert_eq!(frames[1].left.y[row * EYE_W + col], clip_u8(before + 5));
      }
    }
  }

  #[test]
  fn raw_mb_round_trips() {
    let mut raw = Vec::new();
    raw.resize(RAW_MB_BYTES, 77);
    let bytes =
      p_with_tiles(vec![0x80 | MODE_RAW_MB, 0x7f, 0x7f, 0x78, 0], vec![], raw);
    let frames = decode_stream(&bytes).unwrap();
    for row in 0..16 {
      for col in 0..16 {
        assert_eq!(frames[1].left.y[row * EYE_W + col], 77);
      }
    }
  }

  #[test]
  fn raw_4x4_residual_round_trips() {
    let mut res = Vec::new();
    put_u32(&mut res, 0x0000_0001);
    res.push(TAG_RAW_4X4);
    for sample in 0..16 {
      res.push(200 + sample);
    }

    let bytes = p_with_tiles(
      vec![0x80 | MODE_BASE_RES, 0x7f, 0x7f, 0x78, 0],
      res,
      vec![],
    );
    let frames = decode_stream(&bytes).unwrap();
    for row in 0..4 {
      for col in 0..4 {
        assert_eq!(
          frames[1].left.y[row * EYE_W + col],
          200 + (row * 4 + col) as u8
        );
      }
    }
  }

  #[test]
  fn vbs_copy16x8_round_trips() {
    let bytes = p_with_tiles(
      vec![0x80 | MODE_COPY16X8, 1, 0, 255, 0, 0x7f, 0x7f, 0x78, 0],
      vec![],
      vec![],
    );
    let frames = decode_stream(&bytes).unwrap();
    for row in 0..8 {
      for col in 0..16 {
        assert_eq!(
          frames[1].left.y[row * EYE_W + col],
          frames[0].left.y[row * EYE_W + col + 1]
        );
      }
    }
    for row in 8..16 {
      for col in 0..16 {
        let src_col = if col == 0 { 0 } else { col - 1 };
        assert_eq!(
          frames[1].left.y[row * EYE_W + col],
          frames[0].left.y[row * EYE_W + src_col]
        );
      }
    }
  }

  #[test]
  fn segment_map_and_row_bands_validate() {
    let mut bytes = Vec::new();
    write_file_header(&mut bytes, 2);
    let key = patterned_frame(5);
    write_key_raw_frame(&mut bytes, 0, &key);

    let mut left_top = EncodedFragment {
      tile_id: 0,
      row_start: 0,
      row_count: 5,
      start_mb: 0,
      mb_count: 125,
      segment_map_stream: vec![124, 2],
      mode_stream: vec![0x7d, 0],
      residual_stream: vec![],
      raw_stream: vec![],
    };
    let left_bottom = EncodedFragment {
      tile_id: 0,
      row_start: 5,
      row_count: 10,
      start_mb: 125,
      mb_count: 250,
      segment_map_stream: vec![249, 3],
      mode_stream: vec![0x7f, 0x7b, 0],
      residual_stream: vec![],
      raw_stream: vec![],
    };
    // Exercise mixed segment runs inside a fragment.
    left_top.segment_map_stream = vec![9, 1, 114, 2];

    let left = EncodedTile {
      tile_id: 0,
      base_mv: Mv::ZERO,
      segment_count: 4,
      fragments: vec![left_top, left_bottom],
    };
    let right = EncodedTile {
      tile_id: 1,
      base_mv: Mv::ZERO,
      segment_count: 1,
      fragments: vec![EncodedFragment::full_eye(
        1,
        vec![0x7f, 0x7f, 0x79, 0],
        vec![],
        vec![],
      )],
    };
    write_p_frame(&mut bytes, 1, left, right);
    let frames = decode_stream(&bytes).unwrap();
    assert_eq!(frames[1], key);
  }

  #[test]
  fn malformed_stream_is_rejected() {
    let mut bytes = Vec::new();
    write_file_header(&mut bytes, 1);
    write_key_raw_frame(&mut bytes, 0, &patterned_frame(0));
    bytes.truncate(bytes.len() - 10);
    assert!(decode_stream(&bytes).is_err());

    let mut res = Vec::new();
    put_u32(&mut res, 0x0100_0000);
    let bytes = p_with_tiles(
      vec![0x80 | MODE_BASE_RES, 0x7f, 0x7f, 0x78, 0],
      res,
      vec![],
    );
    assert!(decode_stream(&bytes).is_err());
  }

  #[test]
  fn motion_clamps_at_eye_edge() {
    let bytes = p_with_tiles(
      vec![0x18, 0x80 | MODE_COPY16, 127, 0, 0x7f, 0x7f, 0x60, 0],
      vec![],
      vec![],
    );
    let frames = decode_stream(&bytes).unwrap();
    for row in 0..16 {
      for col in 0..16 {
        assert_eq!(
          frames[1].left.y[row * EYE_W + 384 + col],
          frames[0].left.y[row * EYE_W + (EYE_W - 1)]
        );
      }
    }
  }
}
