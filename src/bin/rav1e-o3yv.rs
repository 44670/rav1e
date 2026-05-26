use minidecoder::{
  copy_mb_from_reference, decode_stream, prefill_eye, read_raw_mb,
  write_file_header, write_key_raw_frame, write_p_frame, EncodedFragment,
  EncodedTile, EyeFrame, Mv, SbsFrame, CHROMA_W, EYE_H, EYE_W, MB_COUNT, MB_W,
  MODE_BASE_RES, MODE_COPY16, MODE_RAW_MB, SBS_FRAME_BYTES, TAG_DC_ONLY_S16,
  TAG_DC_ONLY_S8,
};
use std::env;
use std::fs;
use std::io::{self, Read, Write};

#[derive(Debug)]
struct Options {
  input: String,
  output: String,
  frames: Option<usize>,
  keyint: usize,
  loopback: bool,
}

#[derive(Default)]
struct FrameStats {
  skip_mb: usize,
  copy16_mb: usize,
  base_res_mb: usize,
  raw_mb: usize,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
  let options = parse_args()?;
  let input = read_input(&options.input)?;
  if input.len() < SBS_FRAME_BYTES {
    return Err(
      format!(
        "input has fewer than one 800x240 yuv420 frame: {} bytes",
        input.len()
      )
      .into(),
    );
  }

  let available_frames = input.len() / SBS_FRAME_BYTES;
  let frame_count =
    options.frames.unwrap_or(available_frames).min(available_frames);
  if frame_count == 0 {
    return Err("no frames selected".into());
  }

  let mut stream = Vec::new();
  write_file_header(&mut stream, frame_count as u32);

  let mut reference: Option<SbsFrame> = None;
  let mut total = FrameStats::default();

  for frame_no in 0..frame_count {
    let start = frame_no * SBS_FRAME_BYTES;
    let frame =
      SbsFrame::from_yuv420_sbs(&input[start..start + SBS_FRAME_BYTES])?;
    let is_key = frame_no == 0 || frame_no % options.keyint == 0;

    let recon = if is_key {
      write_key_raw_frame(&mut stream, frame_no as u32, &frame);
      frame
    } else {
      let reference_frame =
        reference.as_ref().ok_or("missing reference frame")?;
      let (left_tile, left_recon, left_stats) =
        encode_tile(0, &frame.left, &reference_frame.left);
      let (right_tile, right_recon, right_stats) =
        encode_tile(1, &frame.right, &reference_frame.right);
      write_p_frame(&mut stream, frame_no as u32, left_tile, right_tile);
      add_stats(&mut total, &left_stats);
      add_stats(&mut total, &right_stats);
      eprintln!(
        "frame {frame_no}: skip={} copy16={} base_res={} raw_mb={}",
        left_stats.skip_mb + right_stats.skip_mb,
        left_stats.copy16_mb + right_stats.copy16_mb,
        left_stats.base_res_mb + right_stats.base_res_mb,
        left_stats.raw_mb + right_stats.raw_mb
      );
      SbsFrame { left: left_recon, right: right_recon }
    };

    if options.loopback {
      let decoded = decode_stream(&stream)?;
      let decoded_frame =
        decoded.last().ok_or("loopback decoder produced no frames")?;
      if decoded_frame != &recon {
        return Err(
          format!("closed-loop mismatch at frame {frame_no}").into(),
        );
      }
    }

    reference = Some(recon);
  }

  write_output(&options.output, &stream)?;
  eprintln!(
    "wrote {} bytes, frames={}, skip={} copy16={} base_res={} raw_mb={}",
    stream.len(),
    frame_count,
    total.skip_mb,
    total.copy16_mb,
    total.base_res_mb,
    total.raw_mb
  );
  Ok(())
}

fn parse_args() -> Result<Options, Box<dyn std::error::Error>> {
  let mut input = None;
  let mut output = None;
  let mut frames = None;
  let mut keyint = 48usize;
  let mut loopback = false;

  let mut args = env::args().skip(1);
  while let Some(arg) = args.next() {
    match arg.as_str() {
      "--input" | "-i" => input = args.next(),
      "--output" | "-o" => output = args.next(),
      "--frames" => {
        frames = Some(args.next().ok_or("--frames needs a value")?.parse()?)
      }
      "--keyint" => {
        keyint = args.next().ok_or("--keyint needs a value")?.parse()?
      }
      "--loopback" => loopback = true,
      "--help" | "-h" => {
        print_usage();
        std::process::exit(0);
      }
      _ => return Err(format!("unknown argument {arg}").into()),
    }
  }

  if keyint == 0 {
    return Err("--keyint must be greater than zero".into());
  }

  Ok(Options {
    input: input.ok_or("--input is required")?,
    output: output.ok_or("--output is required")?,
    frames,
    keyint,
    loopback,
  })
}

fn print_usage() {
  eprintln!(
    "usage: rav1e-o3yv --input <800x240-yuv420.yuv|-> --output <out.o3yv|-> [--frames N] [--keyint N] [--loopback]"
  );
}

fn read_input(path: &str) -> io::Result<Vec<u8>> {
  if path == "-" {
    let mut bytes = Vec::new();
    io::stdin().read_to_end(&mut bytes)?;
    Ok(bytes)
  } else {
    fs::read(path)
  }
}

fn write_output(path: &str, bytes: &[u8]) -> io::Result<()> {
  if path == "-" {
    io::stdout().write_all(bytes)
  } else {
    fs::write(path, bytes)
  }
}

fn encode_tile(
  tile_id: u8, src: &EyeFrame, reference: &EyeFrame,
) -> (EncodedTile, EyeFrame, FrameStats) {
  let mut recon = EyeFrame::new();
  prefill_eye(&mut recon, reference, Mv::ZERO);

  let mut mode_stream = Vec::new();
  let mut residual_stream = Vec::new();
  let mut raw_stream = Vec::new();
  let mut stats = FrameStats::default();
  let mut skip_run = 0usize;

  for mb_index in 0..MB_COUNT {
    if mb_equal(src, &recon, mb_index) {
      skip_run += 1;
      stats.skip_mb += 1;
      if skip_run == 0x7f {
        mode_stream.push(0x7f);
        skip_run = 0;
      }
      continue;
    }

    flush_skip(&mut mode_stream, &mut skip_run);

    if let Some(mv) = find_exact_copy(src, reference, mb_index) {
      mode_stream.push(0x80 | MODE_COPY16);
      mode_stream.push(mv.x as u8);
      mode_stream.push(mv.y as u8);
      copy_mb_from_reference(&mut recon, reference, mb_index, mv);
      stats.copy16_mb += 1;
    } else if let Some(residual) = build_dc_residual(src, &recon, mb_index) {
      mode_stream.push(0x80 | MODE_BASE_RES);
      residual_stream.extend_from_slice(&residual);
      copy_mb_from_eye(&mut recon, src, mb_index);
      stats.base_res_mb += 1;
    } else {
      mode_stream.push(0x80 | MODE_RAW_MB);
      read_raw_mb(src, mb_index, &mut raw_stream);
      copy_mb_from_eye(&mut recon, src, mb_index);
      stats.raw_mb += 1;
    }
  }

  flush_skip(&mut mode_stream, &mut skip_run);
  mode_stream.push(0);

  let fragment = EncodedFragment::full_eye(
    tile_id,
    mode_stream,
    residual_stream,
    raw_stream,
  );
  let tile =
    EncodedTile { tile_id, base_mv: Mv::ZERO, fragments: vec![fragment] };
  (tile, recon, stats)
}

fn flush_skip(mode_stream: &mut Vec<u8>, skip_run: &mut usize) {
  while *skip_run > 0 {
    let run = (*skip_run).min(0x7f);
    mode_stream.push(run as u8);
    *skip_run -= run;
  }
}

fn find_exact_copy(
  src: &EyeFrame, reference: &EyeFrame, mb_index: usize,
) -> Option<Mv> {
  for radius in 0..=8 {
    for dy in -(radius as i32)..=(radius as i32) {
      for dx in -(radius as i32)..=(radius as i32) {
        if dx.abs().max(dy.abs()) != radius as i32 {
          continue;
        }
        if dx == 0 && dy == 0 {
          continue;
        }
        let mv = Mv { x: dx as i8, y: dy as i8 };
        if copy_matches(src, reference, mb_index, mv) {
          return Some(mv);
        }
      }
    }
  }
  None
}

fn copy_matches(
  src: &EyeFrame, reference: &EyeFrame, mb_index: usize, mv: Mv,
) -> bool {
  let mb_x = mb_index % MB_W;
  let mb_y = mb_index / MB_W;
  for row in 0..16 {
    for col in 0..16 {
      let x = mb_x * 16 + col;
      let y = mb_y * 16 + row;
      let sx = (x as i32 + mv.x as i32).clamp(0, EYE_W as i32 - 1) as usize;
      let sy = (y as i32 + mv.y as i32).clamp(0, EYE_H as i32 - 1) as usize;
      if src.y[y * EYE_W + x] != reference.y[sy * EYE_W + sx] {
        return false;
      }
    }
  }

  let cmv_x = (mv.x as i32) >> 1;
  let cmv_y = (mv.y as i32) >> 1;
  for row in 0..8 {
    for col in 0..8 {
      let x = mb_x * 8 + col;
      let y = mb_y * 8 + row;
      let sx = (x as i32 + cmv_x).clamp(0, CHROMA_W as i32 - 1) as usize;
      let sy = (y as i32 + cmv_y).clamp(0, (EYE_H / 2) as i32 - 1) as usize;
      if src.cb[y * CHROMA_W + x] != reference.cb[sy * CHROMA_W + sx] {
        return false;
      }
      if src.cr[y * CHROMA_W + x] != reference.cr[sy * CHROMA_W + sx] {
        return false;
      }
    }
  }
  true
}

fn build_dc_residual(
  src: &EyeFrame, pred: &EyeFrame, mb_index: usize,
) -> Option<Vec<u8>> {
  let mut mask = 0u32;
  let mut payloads: Vec<Vec<u8>> = Vec::new();

  for block in 0..16 {
    let bx = (mb_index % MB_W) * 16 + (block % 4) * 4;
    let by = (mb_index / MB_W) * 16 + (block / 4) * 4;
    if let Some(payload) = dc_block_payload(&src.y, &pred.y, EYE_W, bx, by) {
      if !payload.is_empty() {
        mask |= 1 << block;
        payloads.push(payload);
      }
    } else {
      return None;
    }
  }
  for block in 0..4 {
    let bx = (mb_index % MB_W) * 8 + (block % 2) * 4;
    let by = (mb_index / MB_W) * 8 + (block / 2) * 4;
    if let Some(payload) =
      dc_block_payload(&src.cb, &pred.cb, CHROMA_W, bx, by)
    {
      if !payload.is_empty() {
        mask |= 1 << (16 + block);
        payloads.push(payload);
      }
    } else {
      return None;
    }
  }
  for block in 0..4 {
    let bx = (mb_index % MB_W) * 8 + (block % 2) * 4;
    let by = (mb_index / MB_W) * 8 + (block / 2) * 4;
    if let Some(payload) =
      dc_block_payload(&src.cr, &pred.cr, CHROMA_W, bx, by)
    {
      if !payload.is_empty() {
        mask |= 1 << (20 + block);
        payloads.push(payload);
      }
    } else {
      return None;
    }
  }

  if mask == 0 {
    return None;
  }

  let mut out = Vec::new();
  out.extend_from_slice(&mask.to_le_bytes());
  for payload in payloads {
    out.extend_from_slice(&payload);
  }
  Some(out)
}

fn dc_block_payload(
  src: &[u8], pred: &[u8], stride: usize, x: usize, y: usize,
) -> Option<Vec<u8>> {
  let first = src[y * stride + x] as i16 - pred[y * stride + x] as i16;
  for row in 0..4 {
    for col in 0..4 {
      let idx = (y + row) * stride + x + col;
      if src[idx] as i16 - pred[idx] as i16 != first {
        return None;
      }
    }
  }
  if first == 0 {
    return Some(Vec::new());
  }

  let qdc = first * 8;
  let mut out = Vec::new();
  if i8::try_from(qdc).is_ok() {
    out.push(TAG_DC_ONLY_S8);
    out.push(qdc as i8 as u8);
  } else {
    out.push(TAG_DC_ONLY_S16);
    out.extend_from_slice(&qdc.to_le_bytes());
  }
  Some(out)
}

fn mb_equal(a: &EyeFrame, b: &EyeFrame, mb_index: usize) -> bool {
  let mb_x = mb_index % MB_W;
  let mb_y = mb_index / MB_W;
  for row in 0..16 {
    let off = (mb_y * 16 + row) * EYE_W + mb_x * 16;
    if a.y[off..off + 16] != b.y[off..off + 16] {
      return false;
    }
  }
  for row in 0..8 {
    let off = (mb_y * 8 + row) * CHROMA_W + mb_x * 8;
    if a.cb[off..off + 8] != b.cb[off..off + 8]
      || a.cr[off..off + 8] != b.cr[off..off + 8]
    {
      return false;
    }
  }
  true
}

fn copy_mb_from_eye(dst: &mut EyeFrame, src: &EyeFrame, mb_index: usize) {
  let mb_x = mb_index % MB_W;
  let mb_y = mb_index / MB_W;
  for row in 0..16 {
    let off = (mb_y * 16 + row) * EYE_W + mb_x * 16;
    dst.y[off..off + 16].copy_from_slice(&src.y[off..off + 16]);
  }
  for row in 0..8 {
    let off = (mb_y * 8 + row) * CHROMA_W + mb_x * 8;
    dst.cb[off..off + 8].copy_from_slice(&src.cb[off..off + 8]);
    dst.cr[off..off + 8].copy_from_slice(&src.cr[off..off + 8]);
  }
}

fn add_stats(total: &mut FrameStats, frame: &FrameStats) {
  total.skip_mb += frame.skip_mb;
  total.copy16_mb += frame.copy16_mb;
  total.base_res_mb += frame.base_res_mb;
  total.raw_mb += frame.raw_mb;
}

#[cfg(test)]
mod tests {
  use super::*;

  fn solid_frame(y: u8, cb: u8, cr: u8) -> SbsFrame {
    let mut frame = SbsFrame::new();
    frame.left.y.fill(y);
    frame.right.y.fill(y);
    frame.left.cb.fill(cb);
    frame.right.cb.fill(cb);
    frame.left.cr.fill(cr);
    frame.right.cr.fill(cr);
    frame
  }

  #[test]
  fn encoder_path_is_closed_loop() {
    let first = solid_frame(40, 90, 150);
    let second = solid_frame(45, 90, 150);

    let mut stream = Vec::new();
    write_file_header(&mut stream, 2);
    write_key_raw_frame(&mut stream, 0, &first);

    let (left_tile, left_recon, left_stats) =
      encode_tile(0, &second.left, &first.left);
    let (right_tile, right_recon, right_stats) =
      encode_tile(1, &second.right, &first.right);
    assert_eq!(left_stats.base_res_mb + right_stats.base_res_mb, MB_COUNT * 2);
    write_p_frame(&mut stream, 1, left_tile, right_tile);

    let decoded = decode_stream(&stream).unwrap();
    assert_eq!(decoded[0], first);
    assert_eq!(decoded[1], SbsFrame { left: left_recon, right: right_recon });
    assert_eq!(decoded[1], second);
  }
}
