use minidecoder::{
  put_u32, write_file_header, write_key_raw_frame, write_p_frame,
  EncodedFragment, EncodedTile, Mv, SbsFrame, MB_COUNT, MODE_BASE_RES,
  MODE_COPY16, MODE_RAW_MB, RAW_MB_BYTES, TAG_AC_MASK_S8, TAG_DC_ONLY_S8,
  TAG_RAW_4X4,
};
use std::env;
use std::fs;

fn main() -> Result<(), Box<dyn std::error::Error>> {
  let options = parse_args()?;
  let bytes = make_stream(options.kind, options.frames)?;
  fs::write(&options.output, bytes)?;
  Ok(())
}

#[derive(Clone, Copy)]
enum StressKind {
  AllSkip,
  PrefillShift,
  Copy16,
  RawMb,
  Dc6000,
  Ac6000,
  Raw4x4,
}

struct Options {
  kind: StressKind,
  frames: usize,
  output: String,
}

fn parse_args() -> Result<Options, Box<dyn std::error::Error>> {
  let mut kind = None;
  let mut frames = 100usize;
  let mut output = None;

  let mut args = env::args().skip(1);
  while let Some(arg) = args.next() {
    match arg.as_str() {
      "--kind" => {
        kind =
          Some(parse_kind(&args.next().ok_or("--kind requires a name")?)?);
      }
      "--frames" => {
        frames =
          args.next().ok_or("--frames requires a count")?.parse::<usize>()?;
        if frames == 0 {
          return Err("--frames must be positive".into());
        }
      }
      "--output" => {
        output = Some(args.next().ok_or("--output requires a path")?)
      }
      "-h" | "--help" => {
        print_usage();
        std::process::exit(0);
      }
      _ => return Err(format!("unexpected argument {arg}").into()),
    }
  }

  let Some(kind) = kind else {
    print_usage();
    return Err("missing --kind".into());
  };
  let Some(output) = output else {
    print_usage();
    return Err("missing --output".into());
  };

  Ok(Options { kind, frames, output })
}

fn parse_kind(name: &str) -> Result<StressKind, Box<dyn std::error::Error>> {
  match name {
    "all-skip" => Ok(StressKind::AllSkip),
    "prefill-shift" => Ok(StressKind::PrefillShift),
    "copy16" => Ok(StressKind::Copy16),
    "raw-mb" => Ok(StressKind::RawMb),
    "dc6000" => Ok(StressKind::Dc6000),
    "ac6000" => Ok(StressKind::Ac6000),
    "raw4x4" => Ok(StressKind::Raw4x4),
    _ => Err(format!("unknown stress kind {name}").into()),
  }
}

fn print_usage() {
  eprintln!(
    "usage: o3yv-stress --kind all-skip|prefill-shift|copy16|raw-mb|dc6000|ac6000|raw4x4 --output OUT.o3yv [--frames N]"
  );
}

fn make_stream(
  kind: StressKind, frames: usize,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
  let mut out = Vec::new();
  write_file_header(&mut out, frames as u32);
  write_key_raw_frame(&mut out, 0, &patterned_frame());

  for frame_no in 1..frames {
    let left = stress_tile(0, kind);
    let right = stress_tile(1, kind);
    write_p_frame(&mut out, frame_no as u32, left, right);
  }
  Ok(out)
}

fn patterned_frame() -> SbsFrame {
  let mut frame = SbsFrame::new();
  fill_eye(&mut frame.left, 17);
  fill_eye(&mut frame.right, 83);
  frame
}

fn fill_eye(eye: &mut minidecoder::EyeFrame, seed: u8) {
  for (i, y) in eye.y.iter_mut().enumerate() {
    *y = seed.wrapping_add(i as u8).wrapping_add((i / 400) as u8);
  }
  for (i, cb) in eye.cb.iter_mut().enumerate() {
    *cb = 96u8.wrapping_add(seed).wrapping_add((i * 3) as u8);
  }
  for (i, cr) in eye.cr.iter_mut().enumerate() {
    *cr = 160u8.wrapping_sub(seed).wrapping_add((i * 5) as u8);
  }
}

fn stress_tile(tile_id: u8, kind: StressKind) -> EncodedTile {
  let (base_mv, mode_stream, residual_stream, raw_stream) = match kind {
    StressKind::AllSkip => {
      (Mv::ZERO, skip_mode_stream(), Vec::new(), Vec::new())
    }
    StressKind::PrefillShift => {
      (Mv { x: 1, y: 0 }, skip_mode_stream(), Vec::new(), Vec::new())
    }
    StressKind::Copy16 => {
      (Mv::ZERO, copy16_mode_stream(), Vec::new(), Vec::new())
    }
    StressKind::RawMb => {
      let (mode_stream, raw_stream) = raw_mb_streams(tile_id);
      (Mv::ZERO, mode_stream, Vec::new(), raw_stream)
    }
    StressKind::Dc6000 => {
      let (mode_stream, residual_stream) = dc6000_streams();
      (Mv::ZERO, mode_stream, residual_stream, Vec::new())
    }
    StressKind::Ac6000 => {
      let (mode_stream, residual_stream) = ac6000_streams();
      (Mv::ZERO, mode_stream, residual_stream, Vec::new())
    }
    StressKind::Raw4x4 => {
      let (mode_stream, residual_stream) = raw4x4_streams(tile_id);
      (Mv::ZERO, mode_stream, residual_stream, Vec::new())
    }
  };

  EncodedTile {
    tile_id,
    base_mv,
    segment_count: 1,
    fragments: vec![EncodedFragment::full_eye(
      tile_id,
      mode_stream,
      residual_stream,
      raw_stream,
    )],
  }
}

fn skip_mode_stream() -> Vec<u8> {
  vec![0x7f, 0x7f, 0x79, 0]
}

fn copy16_mode_stream() -> Vec<u8> {
  let mut out = Vec::with_capacity(MB_COUNT * 3 + 1);
  for _ in 0..MB_COUNT {
    out.push(0x80 | MODE_COPY16);
    out.push(1);
    out.push(0);
  }
  out.push(0);
  out
}

fn raw_mb_streams(tile_id: u8) -> (Vec<u8>, Vec<u8>) {
  let mut mode = Vec::with_capacity(MB_COUNT + 1);
  let mut raw = Vec::with_capacity(MB_COUNT * RAW_MB_BYTES);
  for mb in 0..MB_COUNT {
    mode.push(0x80 | MODE_RAW_MB);
    for i in 0..RAW_MB_BYTES {
      raw.push(
        (tile_id.wrapping_mul(53))
          .wrapping_add(mb as u8)
          .wrapping_add(i as u8),
      );
    }
  }
  mode.push(0);
  (mode, raw)
}

fn dc6000_streams() -> (Vec<u8>, Vec<u8>) {
  let mut mode = Vec::with_capacity(MB_COUNT + 1);
  let mut residual = Vec::with_capacity(MB_COUNT * 20);
  for _ in 0..MB_COUNT {
    mode.push(0x80 | MODE_BASE_RES);
    put_u32(&mut residual, 0x0000_00ff);
    for _ in 0..8 {
      residual.push(TAG_DC_ONLY_S8);
      residual.push(8);
    }
  }
  mode.push(0);
  (mode, residual)
}

fn ac6000_streams() -> (Vec<u8>, Vec<u8>) {
  let mut mode = Vec::with_capacity(MB_COUNT + 1);
  let mut residual = Vec::with_capacity(MB_COUNT * 84);
  for mb in 0..MB_COUNT {
    mode.push(0x80 | MODE_BASE_RES);
    put_u32(&mut residual, 0x0000_00ff);
    for block in 0..8 {
      residual.push(TAG_AC_MASK_S8);
      residual.extend_from_slice(&0x007fu16.to_le_bytes());
      for coeff in 0..7 {
        residual.push(((mb + block + coeff) % 15 + 1) as u8);
      }
    }
  }
  mode.push(0);
  (mode, residual)
}

fn raw4x4_streams(tile_id: u8) -> (Vec<u8>, Vec<u8>) {
  let mut mode = Vec::with_capacity(MB_COUNT + 1);
  let mut residual = Vec::with_capacity(MB_COUNT * 140);
  for mb in 0..MB_COUNT {
    mode.push(0x80 | MODE_BASE_RES);
    put_u32(&mut residual, 0x0000_00ff);
    for block in 0..8 {
      residual.push(TAG_RAW_4X4);
      for i in 0..16 {
        residual.push(
          tile_id
            .wrapping_mul(41)
            .wrapping_add(mb as u8)
            .wrapping_add((block * 7 + i) as u8),
        );
      }
    }
  }
  mode.push(0);
  (mode, residual)
}
