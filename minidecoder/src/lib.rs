use std::fmt;

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

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

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
  let mut r = Reader::new(bytes);
  parse_file_header(&mut r)?;

  let mut frames = Vec::new();
  let mut reference: Option<SbsFrame> = None;

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

    let frame = match frame_type {
      FRAME_TYPE_KEY_RAW => {
        if tile_count != 2 {
          return Err(Error::Invalid("KEY_RAW tile_count must be 2".into()));
        }
        if frame_size != SBS_FRAME_BYTES {
          return Err(Error::Invalid(format!(
            "KEY_RAW payload must be {SBS_FRAME_BYTES} bytes, got {frame_size}"
          )));
        }
        let left = read_eye_raw(
          &bytes[payload_start..payload_start + EYE_FRAME_BYTES],
        )?;
        let right =
          read_eye_raw(&bytes[payload_start + EYE_FRAME_BYTES..payload_end])?;
        SbsFrame { left, right }
      }
      FRAME_TYPE_P => {
        if tile_count != 2 {
          return Err(Error::Invalid("P-frame tile_count must be 2".into()));
        }
        let reference = reference.as_ref().ok_or_else(|| {
          Error::Invalid(
            "P-frame cannot appear before a reference frame".into(),
          )
        })?;
        let mut current = SbsFrame::new();
        let mut pr = Reader::new(&bytes[payload_start..payload_end]);
        for _ in 0..2 {
          decode_tile(&mut pr, reference, &mut current)?;
        }
        if pr.remaining() != 0 {
          return Err(Error::Invalid("unconsumed P-frame payload".into()));
        }
        current
      }
      _ => return Err(Error::Unsupported(format!("frame type {frame_type}"))),
    };

    r.pos = payload_end;
    reference = Some(frame.clone());
    frames.push(frame);
  }

  Ok(frames)
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
  put_u16(out, 0);
  put_u32(out, payload.len() as u32);
  out.extend_from_slice(&payload);
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
    || tile_flags != 0
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
  prefill_eye(cur_eye, ref_eye, base_mv);

  let mut tr = Reader::new(payload);
  for _ in 0..fragment_count {
    decode_fragment(&mut tr, tile_id, segment_count, ref_eye, cur_eye)?;
  }
  if tr.remaining() != 0 {
    return Err(Error::Invalid("unconsumed tile payload".into()));
  }
  Ok(())
}

fn decode_fragment(
  r: &mut Reader<'_>, tile_id: u8, segment_count: u8, reference: &EyeFrame,
  current: &mut EyeFrame,
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
      mb_index += run;
    } else if (op & 0xf0) == 0x80 {
      let mode = op & 0x0f;
      decode_one_mb(
        mode, mb_index, &mut mr, &mut rr, &mut raw, reference, current,
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
          mode, mb_index, &mut mr, &mut rr, &mut raw, reference, current,
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

fn decode_one_mb(
  mode: u8, mb_index: usize, mode_stream: &mut Reader<'_>,
  residual: &mut Reader<'_>, raw: &mut Reader<'_>, reference: &EyeFrame,
  current: &mut EyeFrame,
) -> Result<()> {
  match mode {
    MODE_BASE_RES => apply_mb_residual(current, mb_index, residual),
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

fn read_mv(r: &mut Reader<'_>) -> Result<Mv> {
  Ok(Mv { x: r.i8()?, y: r.i8()? })
}

fn apply_mb_residual(
  current: &mut EyeFrame, mb_index: usize, residual: &mut Reader<'_>,
) -> Result<()> {
  let mask = residual.u32()?;
  if (mask & 0xff00_0000) != 0 {
    return Err(Error::Invalid("coded block mask uses reserved bits".into()));
  }

  for block in 0..24 {
    if (mask & (1 << block)) == 0 {
      continue;
    }
    if block < 16 {
      let bx = (mb_index % MB_W) * 16 + (block % 4) * 4;
      let by = (mb_index / MB_W) * 16 + (block / 4) * 4;
      apply_block(&mut current.y, EYE_W, bx, by, residual)?;
    } else if block < 20 {
      let cblock = block - 16;
      let bx = (mb_index % MB_W) * 8 + (cblock % 2) * 4;
      let by = (mb_index / MB_W) * 8 + (cblock / 2) * 4;
      apply_block(&mut current.cb, CHROMA_W, bx, by, residual)?;
    } else {
      let cblock = block - 20;
      let bx = (mb_index % MB_W) * 8 + (cblock % 2) * 4;
      let by = (mb_index / MB_W) * 8 + (cblock / 2) * 4;
      apply_block(&mut current.cr, CHROMA_W, bx, by, residual)?;
    }
  }
  Ok(())
}

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
      add_idct_block(
        plane,
        stride,
        x,
        y,
        [coeff, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
      );
    }
    0x40 => {
      if tag != TAG_DC_ONLY_S16 {
        return Err(Error::Invalid(
          "reserved DC_ONLY_S16 tag bits set".into(),
        ));
      }
      let coeff = residual.i16()? as i32;
      add_idct_block(
        plane,
        stride,
        x,
        y,
        [coeff, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
      );
    }
    0x80 => {
      if tag != TAG_AC_MASK_S8 {
        return Err(Error::Invalid("reserved AC_MASK_S8 tag bits set".into()));
      }
      let mut coeffs = [0i32; 16];
      let nz_mask = residual.u16()?;
      for zz in 0..16 {
        if (nz_mask & (1 << zz)) != 0 {
          coeffs[ZIGZAG[zz]] = residual.i8()? as i32;
        }
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
        for zz in 0..16 {
          if (nz_mask & (1 << zz)) != 0 {
            coeffs[ZIGZAG[zz]] = residual.i16()? as i32;
          }
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

fn add_idct_block(
  plane: &mut [u8], stride: usize, x: usize, y: usize, coeffs: [i32; 16],
) {
  let mut tmp = [0i32; 16];
  for col in 0..4 {
    let a1 = coeffs[col] + coeffs[8 + col];
    let b1 = coeffs[col] - coeffs[8 + col];
    let c1 = ((coeffs[4 + col] * 35468) >> 16) - coeffs[12 + col];
    let d1 = coeffs[4 + col] + ((coeffs[12 + col] * 35468) >> 16);
    tmp[col] = a1 + d1;
    tmp[4 + col] = b1 + c1;
    tmp[8 + col] = b1 - c1;
    tmp[12 + col] = a1 - d1;
  }

  for row in 0..4 {
    let base = row * 4;
    let a1 = tmp[base] + tmp[base + 2];
    let b1 = tmp[base] - tmp[base + 2];
    let c1 = ((tmp[base + 1] * 35468) >> 16) - tmp[base + 3];
    let d1 = tmp[base + 1] + ((tmp[base + 3] * 35468) >> 16);
    let vals = [a1 + d1, b1 + c1, b1 - c1, a1 - d1];
    for (col, value) in vals.iter().enumerate() {
      let idx = (y + row) * stride + x + col;
      plane[idx] = clip_u8(plane[idx] as i32 + ((value + 4) >> 3));
    }
  }
}

pub fn prefill_eye(dst: &mut EyeFrame, src: &EyeFrame, mv: Mv) {
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
  for y in 0..h {
    for x in 0..w {
      let sx = clamp_i32(x as i32 + mv_x, 0, w as i32 - 1) as usize;
      let sy = clamp_i32(y as i32 + mv_y, 0, h as i32 - 1) as usize;
      dst[y * w + x] = src[sy * w + sx];
    }
  }
}

fn copy_rect(
  dst: &mut [u8], src: &[u8], w: usize, h: usize, dst_x: usize, dst_y: usize,
  bw: usize, bh: usize, mv_x: i32, mv_y: i32,
) {
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

fn write_raw_mb(current: &mut EyeFrame, mb_index: usize, bytes: &[u8]) {
  let mb_x = mb_index % MB_W;
  let mb_y = mb_index / MB_W;
  let mut off = 0;
  for row in 0..16 {
    let dst = (mb_y * 16 + row) * EYE_W + mb_x * 16;
    current.y[dst..dst + 16].copy_from_slice(&bytes[off..off + 16]);
    off += 16;
  }
  for row in 0..8 {
    let dst = (mb_y * 8 + row) * CHROMA_W + mb_x * 8;
    current.cb[dst..dst + 8].copy_from_slice(&bytes[off..off + 8]);
    off += 8;
  }
  for row in 0..8 {
    let dst = (mb_y * 8 + row) * CHROMA_W + mb_x * 8;
    current.cr[dst..dst + 8].copy_from_slice(&bytes[off..off + 8]);
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

fn read_eye_raw(bytes: &[u8]) -> Result<EyeFrame> {
  if bytes.len() != EYE_FRAME_BYTES {
    return Err(Error::Invalid("bad eye raw size".into()));
  }
  let y_len = EYE_W * EYE_H;
  let c_len = CHROMA_W * CHROMA_H;
  Ok(EyeFrame {
    y: bytes[..y_len].to_vec(),
    cb: bytes[y_len..y_len + c_len].to_vec(),
    cr: bytes[y_len + c_len..].to_vec(),
  })
}

fn clip_u8(v: i32) -> u8 {
  v.clamp(0, 255) as u8
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
  fn new(bytes: &'a [u8]) -> Self {
    Self { bytes, pos: 0 }
  }

  fn remaining(&self) -> usize {
    self.bytes.len() - self.pos
  }

  fn take(&mut self, n: usize) -> Result<&'a [u8]> {
    if n > self.remaining() {
      return Err(Error::Eof);
    }
    let start = self.pos;
    self.pos += n;
    Ok(&self.bytes[start..start + n])
  }

  fn skip(&mut self, n: usize) -> Result<()> {
    self.take(n).map(|_| ())
  }

  fn u8(&mut self) -> Result<u8> {
    Ok(*self.take(1)?.first().unwrap())
  }

  fn i8(&mut self) -> Result<i8> {
    Ok(self.u8()? as i8)
  }

  fn u16(&mut self) -> Result<u16> {
    let bytes = self.take(2)?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
  }

  fn i16(&mut self) -> Result<i16> {
    let bytes = self.take(2)?;
    Ok(i16::from_le_bytes([bytes[0], bytes[1]]))
  }

  fn u32(&mut self) -> Result<u32> {
    let bytes = self.take(4)?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
  }

  fn u64(&mut self) -> Result<u64> {
    let bytes = self.take(8)?;
    Ok(u64::from_le_bytes([
      bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6],
      bytes[7],
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
    let left = EncodedTile {
      tile_id: 0,
      base_mv: Mv::ZERO,
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
    assert_eq!(frames[0], key);
    assert_eq!(frames[1], key);
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
