use minidecoder::{
  copy_mb_from_reference, copy_vbs_from_reference, decode_stream, prefill_eye,
  read_raw_mb, write_file_header, write_key_raw_frame, write_p_frame,
  EncodedFragment, EncodedTile, EyeFrame, Mv, SbsFrame, VbsShape, CHROMA_H,
  CHROMA_W, EYE_H, EYE_W, MB_H, MB_W, MODE_BASE_RES, MODE_COPY16,
  MODE_COPY16X8, MODE_COPY16X8_RES, MODE_COPY16_RES, MODE_COPY8X16,
  MODE_COPY8X16_RES, MODE_COPY8X8, MODE_COPY8X8_RES, MODE_RAW_MB,
  SBS_FRAME_BYTES, TAG_DC_ONLY_S16, TAG_DC_ONLY_S8, TAG_RAW_4X4,
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
  row_bands: bool,
  max_p_frame_bytes: usize,
}

#[derive(Default)]
struct FrameStats {
  skip_mb: usize,
  copy16_mb: usize,
  copy16_res_mb: usize,
  vbs_mb: usize,
  vbs_res_mb: usize,
  base_res_mb: usize,
  raw_mb: usize,
  p_frame_bytes: usize,
}

impl FrameStats {
  fn add_choice(&mut self, choice: &MbChoice) {
    match choice.mode {
      MODE_BASE_RES => self.base_res_mb += 1,
      MODE_COPY16 => self.copy16_mb += 1,
      MODE_COPY16_RES => self.copy16_res_mb += 1,
      MODE_COPY16X8 | MODE_COPY8X16 | MODE_COPY8X8 => self.vbs_mb += 1,
      MODE_COPY16X8_RES | MODE_COPY8X16_RES | MODE_COPY8X8_RES => {
        self.vbs_res_mb += 1
      }
      MODE_RAW_MB => self.raw_mb += 1,
      _ => {}
    }
  }
}

#[derive(Clone)]
struct MbChoice {
  mode: u8,
  mvs: Vec<Mv>,
  residual: Vec<u8>,
  raw: Vec<u8>,
  cost: usize,
  segment_id: u8,
  vbs_shape: Option<VbsShape>,
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
        encode_tile(0, &frame.left, &reference_frame.left, options.row_bands);
      let (right_tile, right_recon, right_stats) = encode_tile(
        1,
        &frame.right,
        &reference_frame.right,
        options.row_bands,
      );
      let frame_start = stream.len();
      write_p_frame(&mut stream, frame_no as u32, left_tile, right_tile);
      let p_frame_bytes = stream.len() - frame_start;
      add_stats(&mut total, &left_stats);
      add_stats(&mut total, &right_stats);
      total.p_frame_bytes += p_frame_bytes;
      eprintln!(
        "frame {frame_no}: bytes={} skip={} copy16={} copy16_res={} vbs={} vbs_res={} base_res={} raw_mb={}",
        p_frame_bytes,
        left_stats.skip_mb + right_stats.skip_mb,
        left_stats.copy16_mb + right_stats.copy16_mb,
        left_stats.copy16_res_mb + right_stats.copy16_res_mb,
        left_stats.vbs_mb + right_stats.vbs_mb,
        left_stats.vbs_res_mb + right_stats.vbs_res_mb,
        left_stats.base_res_mb + right_stats.base_res_mb,
        left_stats.raw_mb + right_stats.raw_mb
      );
      if p_frame_bytes > options.max_p_frame_bytes {
        eprintln!(
          "warning: frame {frame_no} exceeds configured P-frame budget: {p_frame_bytes} > {}",
          options.max_p_frame_bytes
        );
      }
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
    "wrote {} bytes, frames={}, p_bytes={}, skip={} copy16={} copy16_res={} vbs={} vbs_res={} base_res={} raw_mb={}",
    stream.len(),
    frame_count,
    total.p_frame_bytes,
    total.skip_mb,
    total.copy16_mb,
    total.copy16_res_mb,
    total.vbs_mb,
    total.vbs_res_mb,
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
  let mut row_bands = true;
  let mut max_p_frame_bytes = 131_072usize;

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
      "--full-fragment" => row_bands = false,
      "--max-p-frame-bytes" => {
        max_p_frame_bytes =
          args.next().ok_or("--max-p-frame-bytes needs a value")?.parse()?
      }
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
    row_bands,
    max_p_frame_bytes,
  })
}

fn print_usage() {
  eprintln!(
    "usage: rav1e-o3yv --input <800x240-yuv420.yuv|-> --output <out.o3yv|-> [--frames N] [--keyint N] [--loopback] [--full-fragment] [--max-p-frame-bytes N]"
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
  tile_id: u8, src: &EyeFrame, reference: &EyeFrame, row_bands: bool,
) -> (EncodedTile, EyeFrame, FrameStats) {
  let base_mv = find_base_mv(src, reference);
  let mut recon = EyeFrame::new();
  prefill_eye(&mut recon, reference, base_mv);

  let mut stats = FrameStats::default();
  let mut fragments = Vec::new();
  let bands: &[(usize, usize)] =
    if row_bands { &[(0, 5), (5, 5), (10, 5)] } else { &[(0, MB_H)] };

  for &(row_start, row_count) in bands {
    let mut mode_stream = Vec::new();
    let mut residual_stream = Vec::new();
    let mut raw_stream = Vec::new();
    let mut segment_ids = Vec::with_capacity(row_count * MB_W);
    let mut skip_run = 0usize;

    for row in row_start..row_start + row_count {
      for col in 0..MB_W {
        let mb_index = row * MB_W + col;
        if mb_equal(src, &recon, mb_index) {
          skip_run += 1;
          segment_ids.push(0);
          stats.skip_mb += 1;
          if skip_run == 0x7f {
            mode_stream.push(0x7f);
            skip_run = 0;
          }
          continue;
        }

        flush_skip(&mut mode_stream, &mut skip_run);
        let choice = choose_mb(src, reference, &recon, mb_index);
        write_choice(
          &mut mode_stream,
          &mut residual_stream,
          &mut raw_stream,
          &choice,
        );
        apply_choice_to_recon(&mut recon, src, reference, mb_index, &choice);
        segment_ids.push(choice.segment_id);
        stats.add_choice(&choice);
      }
    }

    flush_skip(&mut mode_stream, &mut skip_run);
    mode_stream.push(0);

    let fragment = EncodedFragment {
      tile_id,
      row_start: row_start as u8,
      row_count: row_count as u8,
      start_mb: (row_start * MB_W) as u16,
      mb_count: (row_count * MB_W) as u16,
      segment_map_stream: build_segment_map(&segment_ids),
      mode_stream,
      residual_stream,
      raw_stream,
    };
    fragments.push(fragment);
  }

  let segment_count = if fragments
    .iter()
    .any(|f| f.segment_map_stream.chunks_exact(2).any(|entry| entry[1] != 0))
  {
    4
  } else {
    1
  };
  if segment_count == 1 {
    for fragment in &mut fragments {
      fragment.segment_map_stream.clear();
    }
  }

  let tile = EncodedTile { tile_id, base_mv, segment_count, fragments };
  (tile, recon, stats)
}

fn flush_skip(mode_stream: &mut Vec<u8>, skip_run: &mut usize) {
  while *skip_run > 0 {
    let run = (*skip_run).min(0x7f);
    mode_stream.push(run as u8);
    *skip_run -= run;
  }
}

fn choose_mb(
  src: &EyeFrame, reference: &EyeFrame, prefilled: &EyeFrame, mb_index: usize,
) -> MbChoice {
  let mut candidates = Vec::new();
  let mut raw = Vec::with_capacity(384);
  read_raw_mb(src, mb_index, &mut raw);
  candidates.push(MbChoice {
    mode: MODE_RAW_MB,
    mvs: Vec::new(),
    residual: Vec::new(),
    cost: 1 + raw.len() + 24,
    raw,
    segment_id: 3,
    vbs_shape: None,
  });

  if let Some(residual) = build_lossless_residual(src, prefilled, mb_index) {
    candidates.push(MbChoice {
      mode: MODE_BASE_RES,
      mvs: Vec::new(),
      cost: 1 + residual.len() + residual_decode_cost(&residual),
      residual,
      raw: Vec::new(),
      segment_id: 2,
      vbs_shape: None,
    });
  }

  if let Some(mv) = find_exact_copy(src, reference, mb_index) {
    candidates.push(MbChoice {
      mode: MODE_COPY16,
      mvs: vec![mv],
      residual: Vec::new(),
      raw: Vec::new(),
      cost: 1 + 2 + 2,
      segment_id: 1,
      vbs_shape: None,
    });
  }

  let mv = find_best_copy_mv(src, reference, mb_index);
  let mut copy_pred = prefilled.clone();
  copy_mb_from_reference(&mut copy_pred, reference, mb_index, mv);
  if let Some(residual) = build_lossless_residual(src, &copy_pred, mb_index) {
    candidates.push(MbChoice {
      mode: MODE_COPY16_RES,
      mvs: vec![mv],
      cost: 1 + 2 + residual.len() + residual_decode_cost(&residual),
      residual,
      raw: Vec::new(),
      segment_id: 2,
      vbs_shape: None,
    });
  }

  for shape in [VbsShape::Split16x8, VbsShape::Split8x16, VbsShape::Split8x8] {
    let mvs = find_best_vbs_mvs(src, reference, mb_index, shape);
    let mut pred = prefilled.clone();
    copy_vbs_from_reference(&mut pred, reference, mb_index, shape, &mvs);
    let exact = mb_equal(src, &pred, mb_index);
    let mv_bytes = mvs.len() * 2;
    let exact_mode = match shape {
      VbsShape::Split16x8 => MODE_COPY16X8,
      VbsShape::Split8x16 => MODE_COPY8X16,
      VbsShape::Split8x8 => MODE_COPY8X8,
    };
    let res_mode = match shape {
      VbsShape::Split16x8 => MODE_COPY16X8_RES,
      VbsShape::Split8x16 => MODE_COPY8X16_RES,
      VbsShape::Split8x8 => MODE_COPY8X8_RES,
    };
    if exact {
      candidates.push(MbChoice {
        mode: exact_mode,
        mvs,
        residual: Vec::new(),
        raw: Vec::new(),
        cost: 1 + mv_bytes + 3,
        segment_id: 1,
        vbs_shape: Some(shape),
      });
    } else if let Some(residual) =
      build_lossless_residual(src, &pred, mb_index)
    {
      candidates.push(MbChoice {
        mode: res_mode,
        mvs,
        cost: 1 + mv_bytes + residual.len() + residual_decode_cost(&residual),
        residual,
        raw: Vec::new(),
        segment_id: 2,
        vbs_shape: Some(shape),
      });
    }
  }

  candidates
    .into_iter()
    .min_by_key(|choice| choice.cost)
    .expect("RAW_MB candidate always exists")
}

fn write_choice(
  mode_stream: &mut Vec<u8>, residual_stream: &mut Vec<u8>,
  raw_stream: &mut Vec<u8>, choice: &MbChoice,
) {
  mode_stream.push(0x80 | choice.mode);
  for mv in &choice.mvs {
    mode_stream.push(mv.x as u8);
    mode_stream.push(mv.y as u8);
  }
  residual_stream.extend_from_slice(&choice.residual);
  raw_stream.extend_from_slice(&choice.raw);
}

fn apply_choice_to_recon(
  recon: &mut EyeFrame, src: &EyeFrame, reference: &EyeFrame, mb_index: usize,
  choice: &MbChoice,
) {
  match choice.mode {
    MODE_COPY16 => {
      copy_mb_from_reference(recon, reference, mb_index, choice.mvs[0])
    }
    MODE_COPY16X8 | MODE_COPY8X16 | MODE_COPY8X8 => {
      copy_vbs_from_reference(
        recon,
        reference,
        mb_index,
        choice.vbs_shape.expect("VBS choice has shape"),
        &choice.mvs,
      );
    }
    _ => copy_mb_from_eye(recon, src, mb_index),
  }
}

fn build_segment_map(segment_ids: &[u8]) -> Vec<u8> {
  let mut out = Vec::new();
  let mut i = 0;
  while i < segment_ids.len() {
    let id = segment_ids[i];
    let mut run = 1usize;
    while i + run < segment_ids.len()
      && segment_ids[i + run] == id
      && run < 256
    {
      run += 1;
    }
    out.push((run - 1) as u8);
    out.push(id);
    i += run;
  }
  out
}

fn find_base_mv(src: &EyeFrame, reference: &EyeFrame) -> Mv {
  let mut best = Mv::ZERO;
  let mut best_sad = u64::MAX;
  for y in -4..=4 {
    for x in -4..=4 {
      let mv = Mv { x, y };
      let sad = sampled_eye_sad(src, reference, mv);
      if sad < best_sad {
        best_sad = sad;
        best = mv;
      }
    }
  }
  best
}

fn sampled_eye_sad(src: &EyeFrame, reference: &EyeFrame, mv: Mv) -> u64 {
  let mut sad = 0u64;
  for y in (0..EYE_H).step_by(4) {
    for x in (0..EYE_W).step_by(4) {
      let sx = (x as i32 + mv.x as i32).clamp(0, EYE_W as i32 - 1) as usize;
      let sy = (y as i32 + mv.y as i32).clamp(0, EYE_H as i32 - 1) as usize;
      sad += (src.y[y * EYE_W + x] as i32
        - reference.y[sy * EYE_W + sx] as i32)
        .unsigned_abs() as u64;
    }
  }
  sad
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

fn find_best_copy_mv(
  src: &EyeFrame, reference: &EyeFrame, mb_index: usize,
) -> Mv {
  let mut best = Mv::ZERO;
  let mut best_sad = u64::MAX;
  for y in -8..=8 {
    for x in -8..=8 {
      let mv = Mv { x, y };
      let sad = partition_sad(src, reference, mb_index, 0, 0, 16, 16, mv)
        + chroma_mb_sad(src, reference, mb_index, mv);
      if sad < best_sad {
        best_sad = sad;
        best = mv;
      }
    }
  }
  best
}

fn find_best_vbs_mvs(
  src: &EyeFrame, reference: &EyeFrame, mb_index: usize, shape: VbsShape,
) -> Vec<Mv> {
  match shape {
    VbsShape::Split16x8 => vec![
      find_best_partition_mv(src, reference, mb_index, 0, 0, 16, 8),
      find_best_partition_mv(src, reference, mb_index, 0, 8, 16, 8),
    ],
    VbsShape::Split8x16 => vec![
      find_best_partition_mv(src, reference, mb_index, 0, 0, 8, 16),
      find_best_partition_mv(src, reference, mb_index, 8, 0, 8, 16),
    ],
    VbsShape::Split8x8 => vec![
      find_best_partition_mv(src, reference, mb_index, 0, 0, 8, 8),
      find_best_partition_mv(src, reference, mb_index, 8, 0, 8, 8),
      find_best_partition_mv(src, reference, mb_index, 0, 8, 8, 8),
      find_best_partition_mv(src, reference, mb_index, 8, 8, 8, 8),
    ],
  }
}

fn find_best_partition_mv(
  src: &EyeFrame, reference: &EyeFrame, mb_index: usize, part_x: usize,
  part_y: usize, part_w: usize, part_h: usize,
) -> Mv {
  let mut best = Mv::ZERO;
  let mut best_sad = u64::MAX;
  for y in -8..=8 {
    for x in -8..=8 {
      let mv = Mv { x, y };
      let sad = partition_sad(
        src, reference, mb_index, part_x, part_y, part_w, part_h, mv,
      );
      if sad < best_sad {
        best_sad = sad;
        best = mv;
      }
    }
  }
  best
}

fn partition_sad(
  src: &EyeFrame, reference: &EyeFrame, mb_index: usize, part_x: usize,
  part_y: usize, part_w: usize, part_h: usize, mv: Mv,
) -> u64 {
  let mb_x = mb_index % MB_W;
  let mb_y = mb_index / MB_W;
  let mut sad = 0u64;
  for row in 0..part_h {
    for col in 0..part_w {
      let x = mb_x * 16 + part_x + col;
      let y = mb_y * 16 + part_y + row;
      let sx = (x as i32 + mv.x as i32).clamp(0, EYE_W as i32 - 1) as usize;
      let sy = (y as i32 + mv.y as i32).clamp(0, EYE_H as i32 - 1) as usize;
      sad += (src.y[y * EYE_W + x] as i32
        - reference.y[sy * EYE_W + sx] as i32)
        .unsigned_abs() as u64;
    }
  }
  sad
}

fn chroma_mb_sad(
  src: &EyeFrame, reference: &EyeFrame, mb_index: usize, mv: Mv,
) -> u64 {
  let mb_x = mb_index % MB_W;
  let mb_y = mb_index / MB_W;
  let cmv_x = (mv.x as i32) >> 1;
  let cmv_y = (mv.y as i32) >> 1;
  let mut sad = 0u64;
  for row in 0..8 {
    for col in 0..8 {
      let x = mb_x * 8 + col;
      let y = mb_y * 8 + row;
      let sx = (x as i32 + cmv_x).clamp(0, CHROMA_W as i32 - 1) as usize;
      let sy = (y as i32 + cmv_y).clamp(0, CHROMA_H as i32 - 1) as usize;
      sad += (src.cb[y * CHROMA_W + x] as i32
        - reference.cb[sy * CHROMA_W + sx] as i32)
        .unsigned_abs() as u64;
      sad += (src.cr[y * CHROMA_W + x] as i32
        - reference.cr[sy * CHROMA_W + sx] as i32)
        .unsigned_abs() as u64;
    }
  }
  sad
}

fn build_lossless_residual(
  src: &EyeFrame, pred: &EyeFrame, mb_index: usize,
) -> Option<Vec<u8>> {
  let mut mask = 0u32;
  let mut payloads: Vec<Vec<u8>> = Vec::new();

  for block in 0..16 {
    let bx = (mb_index % MB_W) * 16 + (block % 4) * 4;
    let by = (mb_index / MB_W) * 16 + (block / 4) * 4;
    if let Some(payload) =
      lossless_block_payload(&src.y, &pred.y, EYE_W, bx, by)
    {
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
      lossless_block_payload(&src.cb, &pred.cb, CHROMA_W, bx, by)
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
      lossless_block_payload(&src.cr, &pred.cr, CHROMA_W, bx, by)
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

fn lossless_block_payload(
  src: &[u8], pred: &[u8], stride: usize, x: usize, y: usize,
) -> Option<Vec<u8>> {
  let first = src[y * stride + x] as i16 - pred[y * stride + x] as i16;
  let mut constant_delta = true;
  let mut equal = first == 0;
  for row in 0..4 {
    for col in 0..4 {
      let idx = (y + row) * stride + x + col;
      let delta = src[idx] as i16 - pred[idx] as i16;
      if delta != first {
        constant_delta = false;
      }
      if delta != 0 {
        equal = false;
      }
    }
  }
  if equal {
    return Some(Vec::new());
  }

  if !constant_delta {
    let mut out = Vec::with_capacity(17);
    out.push(TAG_RAW_4X4);
    for row in 0..4 {
      let src_off = (y + row) * stride + x;
      out.extend_from_slice(&src[src_off..src_off + 4]);
    }
    return Some(out);
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

fn residual_decode_cost(residual: &[u8]) -> usize {
  if residual.len() < 4 {
    return 0;
  }
  let mask =
    u32::from_le_bytes([residual[0], residual[1], residual[2], residual[3]]);
  (mask.count_ones() as usize) * 3
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
  total.copy16_res_mb += frame.copy16_res_mb;
  total.vbs_mb += frame.vbs_mb;
  total.vbs_res_mb += frame.vbs_res_mb;
  total.base_res_mb += frame.base_res_mb;
  total.raw_mb += frame.raw_mb;
}

#[cfg(test)]
mod tests {
  use super::*;
  use minidecoder::MB_COUNT;

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
      encode_tile(0, &second.left, &first.left, true);
    let (right_tile, right_recon, right_stats) =
      encode_tile(1, &second.right, &first.right, true);
    assert_eq!(left_stats.base_res_mb + right_stats.base_res_mb, MB_COUNT * 2);
    write_p_frame(&mut stream, 1, left_tile, right_tile);

    let decoded = decode_stream(&stream).unwrap();
    assert_eq!(decoded[0], first);
    assert_eq!(decoded[1], SbsFrame { left: left_recon, right: right_recon });
    assert_eq!(decoded[1], second);
  }
}
