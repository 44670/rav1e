use minidecoder::{
  copy_mb_from_reference, copy_vbs_from_reference, decode_stream, prefill_eye,
  read_raw_mb, write_file_header, write_key_raw_frame, write_p_frame,
  EncodedFragment, EncodedTile, EyeFrame, Mv, SbsFrame, VbsShape, CHROMA_H,
  CHROMA_W, EYE_FRAME_BYTES, EYE_H, EYE_W, MB_H, MB_W, MODE_BASE_RES,
  MODE_COPY16, MODE_COPY16X8, MODE_COPY16X8_RES, MODE_COPY16_RES,
  MODE_COPY8X16, MODE_COPY8X16_RES, MODE_COPY8X8, MODE_COPY8X8_RES,
  MODE_RAW_MB, RAW_MB_BYTES, SBS_FRAME_BYTES, TAG_AC_MASK_S8, TAG_DC_ONLY_S16,
  TAG_DC_ONLY_S8, TAG_RAW_4X4,
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
  raw_4x4_blocks: usize,
  dc_only_blocks: usize,
  ac_mask_blocks: usize,
  full_idct_blocks: usize,
  segment_map_stream_bytes: usize,
  mode_stream_bytes: usize,
  residual_stream_bytes: usize,
  raw_stream_bytes: usize,
  decode_work_units: usize,
  distortion: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ResidualStats {
  raw_4x4_blocks: usize,
  dc_only_blocks: usize,
  ac_mask_blocks: usize,
  full_idct_blocks: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct QualityMetrics {
  y_sse: u64,
  y_mae_milli: u32,
  y_bad_gt16_per_mille: u32,
  y_bad_gt32_per_mille: u32,
  max_mb_y_sse: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SceneChangeMetrics {
  avg_y_delta: u32,
  high_y_delta_percent: u32,
}

const FRAME_HEADER_BYTES: usize = 28;
const KEY_RAW_CODED_BYTES: usize = FRAME_HEADER_BYTES + SBS_FRAME_BYTES;
const RATE_WINDOW_FRAMES: usize = 24;
const RATE_WINDOW_BYTES: usize = 2_000_000;
const QUALITY_RECOVERY_WINDOW_BYTES: usize = RATE_WINDOW_BYTES;
const P_FRAME_SOFT_TARGET_MIN: usize = 60 * 1024;
const MAX_RAW_MB_PER_P_FRAME: usize = 96;
const MAX_AC_MASK_BLOCKS_PER_P_FRAME: usize = 3_650;
const MAX_FULL_IDCT_BLOCKS_PER_P_FRAME: usize = 3_650;
const MAX_P_DECODE_WORK_UNITS: usize = 2_200_000;
const BUDGET_Q_STEPS: [i16; 8] = [4, 8, 12, 16, 24, 32, 48, 64];
const QUALITY_RECOVERY_Q_STEP: i16 = 1;
const QUALITY_RECOVERY_RAW4X4_SSE_THRESHOLDS: [usize; 7] =
  [400, 200, 100, 50, 25, 12, 4];
const QUALITY_RECOVERY_RAW_MB_COST_BIAS: usize = 96;
// 45 dB over one 800x240 luma frame is about 394k SSE.
const QUALITY_TARGET_Y_SSE: u64 = 400_000;
const QUALITY_RECOVERY_RELAXED_STOP_Y_SSE: u64 = QUALITY_TARGET_Y_SSE * 2;
const QUALITY_RECOVERY_TIGHT_WINDOW_BYTES: usize = 1_700_000;
const QUALITY_BAD_Y_MAE_MILLI: u32 = 1_800;
const QUALITY_BAD_Y_GT16_PER_MILLE: u32 = 5;
const QUALITY_RAW_KEY_Y_SSE: u64 = QUALITY_TARGET_Y_SSE * 10;
const QUALITY_AC_MAX_EXTRA_COEFFS: usize = 6;
const QUALITY_AC_MAX_PAYLOAD_BYTES: usize = 10;
const QUALITY_AC_MAX_BLOCK_SSE: usize = 96;
const QUALITY_AC_MIN_SSE_GAIN: usize = 96;
const QUALITY_RAW_4X4_LOCAL_COST: usize = 21;
const QUALITY_AC_DECODE_COST_BIAS: usize = 9;
const QUALITY_AC_DISTORTION_COST_DIVISOR: usize = 16;
const QUALITY_RAW_MB_RAW4X4_BLOCK_THRESHOLD: usize = 18;
const QUALITY_RAW_MB_HARD_MARGIN: usize = 192;
const SCENE_CUT_AVG_Y_DELTA: u32 = 32;
const SCENE_CUT_HIGH_Y_DELTA: u8 = 48;
const SCENE_CUT_HIGH_Y_PERCENT: u32 = 25;
const O3YV_ZIGZAG: [usize; 16] =
  [0, 1, 4, 8, 5, 2, 3, 6, 9, 12, 13, 10, 7, 11, 14, 15];

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
    self.raw_4x4_blocks += choice.residual_stats.raw_4x4_blocks;
    self.dc_only_blocks += choice.residual_stats.dc_only_blocks;
    self.ac_mask_blocks += choice.residual_stats.ac_mask_blocks;
    self.full_idct_blocks += choice.residual_stats.full_idct_blocks;
    self.distortion += choice.distortion;
  }
}

#[derive(Clone)]
struct MbChoice {
  mode: u8,
  mvs: Vec<Mv>,
  residual: Vec<u8>,
  raw: Vec<u8>,
  recon: Vec<u8>,
  cost: usize,
  segment_id: u8,
  residual_stats: ResidualStats,
  distortion: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EncodeMode {
  Lossless,
  Budget { q_step: i16 },
  QualityRecovery { q_step: i16, raw4x4_sse_threshold: usize },
}

impl EncodeMode {
  fn name(self) -> String {
    match self {
      EncodeMode::Lossless => "lossless".into(),
      EncodeMode::Budget { q_step } => format!("budget-q{q_step}"),
      EncodeMode::QualityRecovery { q_step, raw4x4_sse_threshold } => {
        format!("quality-recovery-q{q_step}-raw4x4-sse{raw4x4_sse_threshold}")
      }
    }
  }

  fn q_step(self) -> Option<i16> {
    match self {
      EncodeMode::Lossless => None,
      EncodeMode::Budget { q_step }
      | EncodeMode::QualityRecovery { q_step, .. } => Some(q_step),
    }
  }

  fn quality_recovery(self) -> bool {
    matches!(self, EncodeMode::QualityRecovery { .. })
  }

  fn raw4x4_sse_threshold(self) -> Option<usize> {
    match self {
      EncodeMode::QualityRecovery { raw4x4_sse_threshold, .. } => {
        Some(raw4x4_sse_threshold)
      }
      _ => None,
    }
  }
}

struct PFrameCandidate {
  bytes: Vec<u8>,
  recon: SbsFrame,
  stats: FrameStats,
  mode_name: String,
  q_step: Option<i16>,
  quality: QualityMetrics,
  quality_recovery_used: bool,
  left_base_mv: Mv,
  right_base_mv: Mv,
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
  let mut previous_source: Option<SbsFrame> = None;
  let mut recent_frame_bytes = Vec::new();
  let mut total = FrameStats::default();
  let mut key_frames = 0usize;
  let mut last_raw_frame_no: Option<usize> = None;

  for frame_no in 0..frame_count {
    let start = frame_no * SBS_FRAME_BYTES;
    let frame =
      SbsFrame::from_yuv420_sbs(&input[start..start + SBS_FRAME_BYTES])?;
    let max_gap_key =
      raw_key_due_for_max_gap(frame_no, last_raw_frame_no, options.keyint);
    let scene_metrics = previous_source
      .as_ref()
      .map(|previous| source_scene_change_metrics(&frame, previous));
    let scene_cut = !max_gap_key
      && scene_metrics.is_some_and(scene_change_requires_keyframe);
    let scene_key_allowed =
      rolling_window_allows_keyframe(&recent_frame_bytes);
    let is_key = max_gap_key || (scene_cut && scene_key_allowed);

    let frame_start = stream.len();
    let recon = if is_key {
      write_key_raw_frame(&mut stream, frame_no as u32, &frame);
      key_frames += 1;
      last_raw_frame_no = Some(frame_no);
      match (frame_no, max_gap_key, scene_metrics) {
        (0, _, _) => eprintln!(
          "frame {frame_no}: mode=key-raw bytes={}",
          stream.len() - frame_start
        ),
        (_, true, _) => eprintln!(
          "frame {frame_no}: mode=key-raw reason=max-gap bytes={}",
          stream.len() - frame_start
        ),
        (_, false, Some(metrics)) => eprintln!(
          "frame {frame_no}: mode=key-raw reason=scene-cut bytes={} avg_y_delta={} high_y_delta={}%",
          stream.len() - frame_start,
          metrics.avg_y_delta,
          metrics.high_y_delta_percent
        ),
        _ => unreachable!("scene-cut keyframe requires metrics"),
      }
      let key_reason = if frame_no == 0 {
        "first"
      } else if max_gap_key {
        "max-gap"
      } else {
        "scene-cut"
      };
      log_key_frame_stats(
        frame_no,
        stream.len() - frame_start,
        &recent_frame_bytes,
        key_reason,
      );
      frame.clone()
    } else {
      if scene_cut {
        if let Some(metrics) = scene_metrics {
          eprintln!(
            "frame {frame_no}: scene-cut deferred by rolling budget avg_y_delta={} high_y_delta={}%",
            metrics.avg_y_delta, metrics.high_y_delta_percent
          );
        }
      }
      let reference_frame =
        reference.as_ref().ok_or("missing reference frame")?;
      let reserve_next_raw = next_frame_needs_raw_reserve(
        frame_no,
        frame_count,
        last_raw_frame_no,
        options.keyint,
        &input,
        &frame,
      )?;
      let candidate = encode_budgeted_p_frame(
        frame_no as u32,
        &frame,
        reference_frame,
        options.row_bands,
        options.max_p_frame_bytes,
        &recent_frame_bytes,
        reserve_next_raw,
      );
      add_stats(&mut total, &candidate.stats);
      total.p_frame_bytes += candidate.stats.p_frame_bytes;
      eprintln!(
        "frame {frame_no}: mode={} bytes={} skip={} copy16={} copy16_res={} vbs={} vbs_res={} base_res={} raw_mb={} raw4x4={} dc={} ac={} y_mae_milli={} y_bad16_pm={}",
        candidate.mode_name,
        candidate.stats.p_frame_bytes,
        candidate.stats.skip_mb,
        candidate.stats.copy16_mb,
        candidate.stats.copy16_res_mb,
        candidate.stats.vbs_mb,
        candidate.stats.vbs_res_mb,
        candidate.stats.base_res_mb,
        candidate.stats.raw_mb,
        candidate.stats.raw_4x4_blocks,
        candidate.stats.dc_only_blocks,
        candidate.stats.ac_mask_blocks,
        candidate.quality.y_mae_milli,
        candidate.quality.y_bad_gt16_per_mille
      );
      log_p_frame_stats(frame_no, &candidate, &recent_frame_bytes);
      stream.extend_from_slice(&candidate.bytes);
      candidate.recon
    };
    recent_frame_bytes.push(stream.len() - frame_start);

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
    previous_source = Some(frame);
  }

  write_output(&options.output, &stream)?;
  eprintln!(
    "wrote {} bytes, frames={}, key_frames={}, p_bytes={}, skip={} copy16={} copy16_res={} vbs={} vbs_res={} base_res={} raw_mb={} raw4x4={} dc={} ac={} full_idct={} decode_work={}",
    stream.len(),
    frame_count,
    key_frames,
    total.p_frame_bytes,
    total.skip_mb,
    total.copy16_mb,
    total.copy16_res_mb,
    total.vbs_mb,
    total.vbs_res_mb,
    total.base_res_mb,
    total.raw_mb,
    total.raw_4x4_blocks,
    total.dc_only_blocks,
    total.ac_mask_blocks,
    total.full_idct_blocks,
    total.decode_work_units
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

fn raw_key_due_for_max_gap(
  frame_no: usize, last_raw_frame_no: Option<usize>, max_gap: usize,
) -> bool {
  last_raw_frame_no
    .map(|last| frame_no.saturating_sub(last) >= max_gap)
    .unwrap_or(true)
}

fn log_key_frame_stats(
  frame_no: usize, frame_bytes: usize, recent_frame_bytes: &[usize],
  reason: &str,
) {
  let rolling_1s_bytes =
    rolling_window_bytes_after(recent_frame_bytes, frame_bytes);
  eprintln!(
    "{{\"kind\":\"o3yv_frame_stats\",\"frame_no\":{},\"frame_type\":\"raw\",\"frame_size_bytes\":{},\"rolling_1s_bytes\":{},\"reason\":\"{}\",\"q_step\":-1,\"skip_base_mb\":0,\"copy16_mb\":0,\"copy_vbs_mb\":0,\"base_res_mb\":0,\"raw_mb\":0,\"raw_4x4_blocks\":0,\"dc_only_blocks\":0,\"ac_mask_blocks\":0,\"full_idct_blocks\":0,\"segment_map_stream_bytes\":0,\"residual_stream_bytes\":0,\"raw_stream_bytes\":{},\"mode_stream_bytes\":0,\"decode_work_units\":{},\"y_sse\":0,\"y_mae_milli\":0,\"y_bad_gt16_per_mille\":0,\"y_bad_gt32_per_mille\":0,\"max_mb_y_sse\":0,\"quality_recovery_used\":false}}",
    frame_no,
    frame_bytes,
    rolling_1s_bytes,
    reason,
    SBS_FRAME_BYTES,
    SBS_FRAME_BYTES
  );
}

fn log_p_frame_stats(
  frame_no: usize, candidate: &PFrameCandidate, recent_frame_bytes: &[usize],
) {
  let rolling_1s_bytes = rolling_window_bytes_after(
    recent_frame_bytes,
    candidate.stats.p_frame_bytes,
  );
  eprintln!(
    "{{\"kind\":\"o3yv_frame_stats\",\"frame_no\":{},\"frame_type\":\"p\",\"frame_size_bytes\":{},\"rolling_1s_bytes\":{},\"q_step\":{},\"skip_base_mb\":{},\"copy16_mb\":{},\"copy_vbs_mb\":{},\"base_res_mb\":{},\"raw_mb\":{},\"raw_4x4_blocks\":{},\"dc_only_blocks\":{},\"ac_mask_blocks\":{},\"full_idct_blocks\":{},\"segment_map_stream_bytes\":{},\"residual_stream_bytes\":{},\"raw_stream_bytes\":{},\"mode_stream_bytes\":{},\"decode_work_units\":{},\"left_base_mv_x\":{},\"left_base_mv_y\":{},\"right_base_mv_x\":{},\"right_base_mv_y\":{},\"y_sse\":{},\"y_mae_milli\":{},\"y_bad_gt16_per_mille\":{},\"y_bad_gt32_per_mille\":{},\"max_mb_y_sse\":{},\"quality_recovery_used\":{}}}",
    frame_no,
    candidate.stats.p_frame_bytes,
    rolling_1s_bytes,
    candidate.q_step.unwrap_or(-1),
    candidate.stats.skip_mb,
    candidate.stats.copy16_mb,
    candidate.stats.vbs_mb + candidate.stats.vbs_res_mb,
    candidate.stats.base_res_mb,
    candidate.stats.raw_mb,
    candidate.stats.raw_4x4_blocks,
    candidate.stats.dc_only_blocks,
    candidate.stats.ac_mask_blocks,
    candidate.stats.full_idct_blocks,
    candidate.stats.segment_map_stream_bytes,
    candidate.stats.residual_stream_bytes,
    candidate.stats.raw_stream_bytes,
    candidate.stats.mode_stream_bytes,
    candidate.stats.decode_work_units,
    candidate.left_base_mv.x,
    candidate.left_base_mv.y,
    candidate.right_base_mv.x,
    candidate.right_base_mv.y,
    candidate.quality.y_sse,
    candidate.quality.y_mae_milli,
    candidate.quality.y_bad_gt16_per_mille,
    candidate.quality.y_bad_gt32_per_mille,
    candidate.quality.max_mb_y_sse,
    candidate.quality_recovery_used
  );
}

fn encode_budgeted_p_frame(
  frame_no: u32, frame: &SbsFrame, reference: &SbsFrame, row_bands: bool,
  max_p_frame_bytes: usize, recent_frame_bytes: &[usize],
  reserve_next_raw: bool,
) -> PFrameCandidate {
  let lossless = encode_p_frame_candidate(
    frame_no,
    frame,
    reference,
    row_bands,
    EncodeMode::Lossless,
  );
  if p_frame_satisfies_budget(&lossless.stats, max_p_frame_bytes) {
    return lossless;
  }

  let mut best = lossless;
  for q_step in BUDGET_Q_STEPS {
    let candidate = encode_p_frame_candidate(
      frame_no,
      frame,
      reference,
      row_bands,
      EncodeMode::Budget { q_step },
    );
    if candidate.stats.p_frame_bytes < best.stats.p_frame_bytes {
      best = candidate;
    }
    if p_frame_satisfies_budget(&best.stats, max_p_frame_bytes) {
      break;
    }
  }

  if should_try_quality_recovery(&best, recent_frame_bytes, reserve_next_raw) {
    let recovery_stop_y_sse =
      quality_recovery_stop_y_sse(&best, recent_frame_bytes, reserve_next_raw);
    for raw4x4_sse_threshold in QUALITY_RECOVERY_RAW4X4_SSE_THRESHOLDS {
      let recovery = encode_p_frame_candidate(
        frame_no,
        frame,
        reference,
        row_bands,
        EncodeMode::QualityRecovery {
          q_step: QUALITY_RECOVERY_Q_STEP,
          raw4x4_sse_threshold,
        },
      );
      if p_frame_satisfies_budget(&recovery.stats, max_p_frame_bytes)
        && recovery.quality.y_sse < best.quality.y_sse
        && rolling_window_allows_quality_recovery_frame_bytes(
          recent_frame_bytes,
          recovery.stats.p_frame_bytes,
          reserve_next_raw,
        )
      {
        best = recovery;
        if best.quality.y_sse <= recovery_stop_y_sse {
          break;
        }
      }
    }
  }

  if !p_frame_satisfies_budget(&best.stats, max_p_frame_bytes) {
    eprintln!(
      "warning: frame {frame_no} still exceeds budget after quantization: bytes={} raw_mb={} decode_work={} (limits: bytes<={max_p_frame_bytes}, raw_mb<={MAX_RAW_MB_PER_P_FRAME}, decode_work<={MAX_P_DECODE_WORK_UNITS})",
      best.stats.p_frame_bytes,
      best.stats.raw_mb,
      best.stats.decode_work_units
    );
  }
  best
}

fn encode_p_frame_candidate(
  frame_no: u32, frame: &SbsFrame, reference: &SbsFrame, row_bands: bool,
  mode: EncodeMode,
) -> PFrameCandidate {
  let (left_tile, left_recon, left_stats) =
    encode_tile(0, &frame.left, &reference.left, row_bands, mode);
  let (right_tile, right_recon, right_stats) =
    encode_tile(1, &frame.right, &reference.right, row_bands, mode);
  let left_base_mv = left_tile.base_mv;
  let right_base_mv = right_tile.base_mv;
  let mut bytes = Vec::new();
  write_p_frame(&mut bytes, frame_no, left_tile, right_tile);

  let mut stats = FrameStats::default();
  add_stats(&mut stats, &left_stats);
  add_stats(&mut stats, &right_stats);
  stats.p_frame_bytes = bytes.len();
  stats.decode_work_units = estimate_p_decode_work_units(&stats);
  let recon = SbsFrame { left: left_recon, right: right_recon };
  let quality = frame_quality_metrics(frame, &recon);

  PFrameCandidate {
    bytes,
    recon,
    stats,
    mode_name: mode.name(),
    q_step: mode.q_step(),
    quality,
    quality_recovery_used: mode.quality_recovery(),
    left_base_mv,
    right_base_mv,
  }
}

fn p_frame_satisfies_budget(
  stats: &FrameStats, max_p_frame_bytes: usize,
) -> bool {
  stats.p_frame_bytes <= max_p_frame_bytes
    && stats.raw_mb <= MAX_RAW_MB_PER_P_FRAME
    && stats.ac_mask_blocks <= MAX_AC_MASK_BLOCKS_PER_P_FRAME
    && stats.full_idct_blocks <= MAX_FULL_IDCT_BLOCKS_PER_P_FRAME
    && stats.decode_work_units <= MAX_P_DECODE_WORK_UNITS
}

fn estimate_p_decode_work_units(stats: &FrameStats) -> usize {
  let stream_parse_units = stats.segment_map_stream_bytes
    + stats.mode_stream_bytes * 2
    + stats.residual_stream_bytes * 2
    + stats.raw_stream_bytes;
  let prefill_units = 2 * EYE_FRAME_BYTES;
  let prediction_mb =
    stats.copy16_mb + stats.copy16_res_mb + stats.vbs_mb + stats.vbs_res_mb;
  let prediction_units = prediction_mb * RAW_MB_BYTES;
  let raw_units = stats.raw_mb * RAW_MB_BYTES + stats.raw_4x4_blocks * 16;
  let residual_mb = stats.base_res_mb + stats.copy16_res_mb + stats.vbs_res_mb;
  let residual_units =
    residual_mb * 8 + stats.dc_only_blocks * 64 + stats.full_idct_blocks * 512;
  stream_parse_units
    + prefill_units
    + prediction_units
    + raw_units
    + residual_units
}

fn rolling_window_allows_keyframe(recent_frame_bytes: &[usize]) -> bool {
  rolling_window_allows_frame_bytes(recent_frame_bytes, KEY_RAW_CODED_BYTES)
}

fn rolling_window_allows_frame_bytes(
  recent_frame_bytes: &[usize], next_frame_bytes: usize,
) -> bool {
  rolling_window_bytes_after(recent_frame_bytes, next_frame_bytes)
    <= RATE_WINDOW_BYTES
}

fn rolling_window_allows_quality_recovery_frame_bytes(
  recent_frame_bytes: &[usize], next_frame_bytes: usize,
  reserve_next_raw: bool,
) -> bool {
  if reserve_next_raw {
    return rolling_window_allows_next_raw_after_frame(
      recent_frame_bytes,
      next_frame_bytes,
    );
  }
  rolling_window_bytes_after(recent_frame_bytes, next_frame_bytes)
    <= QUALITY_RECOVERY_WINDOW_BYTES
}

fn rolling_window_allows_next_raw_after_frame(
  recent_frame_bytes: &[usize], current_frame_bytes: usize,
) -> bool {
  let recent_bytes: usize = recent_frame_bytes
    .iter()
    .rev()
    .take(RATE_WINDOW_FRAMES.saturating_sub(2))
    .sum();
  recent_bytes + current_frame_bytes + KEY_RAW_CODED_BYTES <= RATE_WINDOW_BYTES
}

fn rolling_window_bytes_after(
  recent_frame_bytes: &[usize], next_frame_bytes: usize,
) -> usize {
  let recent_bytes: usize = recent_frame_bytes
    .iter()
    .rev()
    .take(RATE_WINDOW_FRAMES.saturating_sub(1))
    .sum();
  recent_bytes + next_frame_bytes
}

fn should_try_quality_recovery(
  candidate: &PFrameCandidate, recent_frame_bytes: &[usize],
  reserve_next_raw: bool,
) -> bool {
  let has_room = rolling_window_allows_quality_recovery_frame_bytes(
    recent_frame_bytes,
    candidate.stats.p_frame_bytes,
    reserve_next_raw,
  );
  if !has_room {
    return false;
  }

  candidate.quality.y_sse > QUALITY_TARGET_Y_SSE
    || (candidate.stats.p_frame_bytes < P_FRAME_SOFT_TARGET_MIN
      && rolling_window_allows_frame_bytes(
        recent_frame_bytes,
        P_FRAME_SOFT_TARGET_MIN,
      )
      && candidate.quality.y_mae_milli >= QUALITY_BAD_Y_MAE_MILLI
      && candidate.quality.y_bad_gt16_per_mille
        >= QUALITY_BAD_Y_GT16_PER_MILLE)
    || candidate.quality.y_sse >= QUALITY_RAW_KEY_Y_SSE
}

fn quality_recovery_stop_y_sse(
  candidate: &PFrameCandidate, recent_frame_bytes: &[usize],
  reserve_next_raw: bool,
) -> u64 {
  if reserve_next_raw
    || rolling_window_bytes_after(
      recent_frame_bytes,
      candidate.stats.p_frame_bytes,
    ) >= QUALITY_RECOVERY_TIGHT_WINDOW_BYTES
  {
    QUALITY_RECOVERY_RELAXED_STOP_Y_SSE
  } else {
    QUALITY_TARGET_Y_SSE
  }
}

fn next_frame_needs_raw_reserve(
  frame_no: usize, frame_count: usize, last_raw_frame_no: Option<usize>,
  keyint: usize, input: &[u8], current: &SbsFrame,
) -> Result<bool, Box<dyn std::error::Error>> {
  let next_frame_no = frame_no + 1;
  if next_frame_no >= frame_count {
    return Ok(false);
  }
  if raw_key_due_for_max_gap(next_frame_no, last_raw_frame_no, keyint) {
    return Ok(true);
  }

  let start = next_frame_no * SBS_FRAME_BYTES;
  let next =
    SbsFrame::from_yuv420_sbs(&input[start..start + SBS_FRAME_BYTES])?;
  Ok(scene_change_requires_keyframe(source_scene_change_metrics(
    &next, current,
  )))
}

fn scene_change_requires_keyframe(metrics: SceneChangeMetrics) -> bool {
  metrics.avg_y_delta >= SCENE_CUT_AVG_Y_DELTA
    && metrics.high_y_delta_percent >= SCENE_CUT_HIGH_Y_PERCENT
}

fn source_scene_change_metrics(
  current: &SbsFrame, previous: &SbsFrame,
) -> SceneChangeMetrics {
  let mut sad = 0u64;
  let mut high_delta_samples = 0u64;
  let mut samples = 0u64;

  accumulate_scene_eye_metrics(
    &current.left,
    &previous.left,
    find_base_mv(&current.left, &previous.left),
    &mut sad,
    &mut high_delta_samples,
    &mut samples,
  );
  accumulate_scene_eye_metrics(
    &current.right,
    &previous.right,
    find_base_mv(&current.right, &previous.right),
    &mut sad,
    &mut high_delta_samples,
    &mut samples,
  );

  SceneChangeMetrics {
    avg_y_delta: (sad / samples) as u32,
    high_y_delta_percent: ((high_delta_samples * 100) / samples) as u32,
  }
}

fn accumulate_scene_eye_metrics(
  current: &EyeFrame, previous: &EyeFrame, mv: Mv, sad: &mut u64,
  high_delta_samples: &mut u64, samples: &mut u64,
) {
  for y in (0..EYE_H).step_by(4) {
    for x in (0..EYE_W).step_by(4) {
      let sx = (x as i32 + mv.x as i32).clamp(0, EYE_W as i32 - 1) as usize;
      let sy = (y as i32 + mv.y as i32).clamp(0, EYE_H as i32 - 1) as usize;
      let delta = (current.y[y * EYE_W + x] as i32
        - previous.y[sy * EYE_W + sx] as i32)
        .unsigned_abs() as u64;
      *sad += delta;
      if delta >= SCENE_CUT_HIGH_Y_DELTA as u64 {
        *high_delta_samples += 1;
      }
      *samples += 1;
    }
  }
}

fn frame_quality_metrics(src: &SbsFrame, recon: &SbsFrame) -> QualityMetrics {
  let mut metrics = QualityMetrics::default();
  accumulate_quality_eye_metrics(&src.left, &recon.left, &mut metrics);
  accumulate_quality_eye_metrics(&src.right, &recon.right, &mut metrics);
  let samples = (EYE_W * EYE_H * 2) as u64;

  let (abs_sum, bad16, bad32, max_mb) =
    quality_abs_metrics(&src.left, &recon.left, &src.right, &recon.right);
  metrics.y_mae_milli = ((abs_sum * 1000) / samples) as u32;
  metrics.y_bad_gt16_per_mille = ((bad16 * 1000) / samples) as u32;
  metrics.y_bad_gt32_per_mille = ((bad32 * 1000) / samples) as u32;
  metrics.max_mb_y_sse = max_mb;
  metrics
}

fn accumulate_quality_eye_metrics(
  src: &EyeFrame, recon: &EyeFrame, metrics: &mut QualityMetrics,
) {
  for (a, b) in src.y.iter().zip(&recon.y) {
    let delta = (*a as i32 - *b as i32).unsigned_abs() as u64;
    metrics.y_sse += delta * delta;
  }
}

fn quality_abs_metrics(
  left_src: &EyeFrame, left_recon: &EyeFrame, right_src: &EyeFrame,
  right_recon: &EyeFrame,
) -> (u64, u64, u64, u64) {
  let mut abs_sum = 0u64;
  let mut bad16 = 0u64;
  let mut bad32 = 0u64;
  let mut max_mb = 0u64;
  for (src, recon) in [(left_src, left_recon), (right_src, right_recon)] {
    for (a, b) in src.y.iter().zip(&recon.y) {
      let delta = (*a as i32 - *b as i32).unsigned_abs() as u64;
      abs_sum += delta;
      if delta > 16 {
        bad16 += 1;
      }
      if delta > 32 {
        bad32 += 1;
      }
    }
    for mb_index in 0..(MB_W * MB_H) {
      max_mb = max_mb.max(mb_y_sse(src, recon, mb_index));
    }
  }
  (abs_sum, bad16, bad32, max_mb)
}

fn mb_y_sse(src: &EyeFrame, recon: &EyeFrame, mb_index: usize) -> u64 {
  let mb_x = mb_index % MB_W;
  let mb_y = mb_index / MB_W;
  let mut sse = 0u64;
  for row in 0..16 {
    for col in 0..16 {
      let idx = (mb_y * 16 + row) * EYE_W + mb_x * 16 + col;
      let delta =
        (src.y[idx] as i32 - recon.y[idx] as i32).unsigned_abs() as u64;
      sse += delta * delta;
    }
  }
  sse
}

fn encode_tile(
  tile_id: u8, src: &EyeFrame, reference: &EyeFrame, row_bands: bool,
  mode: EncodeMode,
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
        let choice = choose_mb(src, reference, &recon, mb_index, mode);
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
    stats.mode_stream_bytes += mode_stream.len();
    stats.residual_stream_bytes += residual_stream.len();
    stats.raw_stream_bytes += raw_stream.len();

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
  stats.segment_map_stream_bytes =
    fragments.iter().map(|fragment| fragment.segment_map_stream.len()).sum();

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
  mode: EncodeMode,
) -> MbChoice {
  let mut candidates = Vec::new();
  let mut raw = Vec::with_capacity(384);
  read_raw_mb(src, mb_index, &mut raw);
  if mode == EncodeMode::Lossless || mode.quality_recovery() {
    candidates.push(MbChoice {
      mode: MODE_RAW_MB,
      mvs: Vec::new(),
      residual: Vec::new(),
      recon: raw.clone(),
      cost: 1
        + raw.len()
        + if mode.quality_recovery() {
          QUALITY_RECOVERY_RAW_MB_COST_BIAS
        } else {
          0
        },
      raw,
      segment_id: 3,
      residual_stats: ResidualStats::default(),
      distortion: 0,
    });
  }

  add_residual_candidates(
    &mut candidates,
    MODE_BASE_RES,
    Vec::new(),
    src,
    prefilled,
    mb_index,
    mode,
  );

  if let Some(mv) = find_exact_copy(src, reference, mb_index) {
    let mut pred = prefilled.clone();
    copy_mb_from_reference(&mut pred, reference, mb_index, mv);
    candidates.push(MbChoice {
      mode: MODE_COPY16,
      mvs: vec![mv],
      residual: Vec::new(),
      raw: Vec::new(),
      recon: raw_mb_bytes(&pred, mb_index),
      cost: 1 + 2 + 2,
      segment_id: 1,
      residual_stats: ResidualStats::default(),
      distortion: 0,
    });
  }

  let mv = find_best_copy_mv(src, reference, mb_index);
  let mut copy_pred = prefilled.clone();
  copy_mb_from_reference(&mut copy_pred, reference, mb_index, mv);
  add_residual_candidates(
    &mut candidates,
    MODE_COPY16_RES,
    vec![mv],
    src,
    &copy_pred,
    mb_index,
    mode,
  );

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
        recon: raw_mb_bytes(&pred, mb_index),
        cost: 1 + mv_bytes + 3,
        segment_id: 1,
        residual_stats: ResidualStats::default(),
        distortion: 0,
      });
    } else {
      add_residual_candidates(
        &mut candidates,
        res_mode,
        mvs,
        src,
        &pred,
        mb_index,
        mode,
      );
    }
  }

  if mode.quality_recovery() {
    if let Some(choice) = choose_quality_recovery_raw_mb(&candidates) {
      return choice;
    }
  }

  candidates
    .into_iter()
    .min_by_key(|choice| choice.cost)
    .expect("at least one residual or raw candidate exists")
}

fn choose_quality_recovery_raw_mb(
  candidates: &[MbChoice],
) -> Option<MbChoice> {
  let raw = candidates.iter().find(|choice| choice.mode == MODE_RAW_MB)?;
  let best = candidates.iter().min_by_key(|choice| choice.cost)?;
  if best.mode == MODE_RAW_MB {
    return Some(raw.clone());
  }
  if best.residual_stats.raw_4x4_blocks < QUALITY_RAW_MB_RAW4X4_BLOCK_THRESHOLD
  {
    return None;
  }
  if raw.cost <= best.cost + QUALITY_RAW_MB_HARD_MARGIN {
    Some(raw.clone())
  } else {
    None
  }
}

fn add_residual_candidates(
  candidates: &mut Vec<MbChoice>, mode_id: u8, mvs: Vec<Mv>, src: &EyeFrame,
  pred: &EyeFrame, mb_index: usize, mode: EncodeMode,
) {
  match mode {
    EncodeMode::Lossless => {
      if let Some(residual) = build_lossless_residual(src, pred, mb_index) {
        let residual_stats = residual_stats(&residual);
        candidates.push(MbChoice {
          mode: mode_id,
          mvs,
          cost: 1
            + mv_payload_len(mode_id)
            + residual.len()
            + residual_decode_cost(&residual),
          residual,
          raw: Vec::new(),
          recon: raw_mb_bytes(src, mb_index),
          segment_id: 2,
          residual_stats,
          distortion: 0,
        });
      }
    }
    EncodeMode::Budget { q_step }
    | EncodeMode::QualityRecovery { q_step, .. } => {
      for (segment_id, segment_q) in aq_steps(q_step) {
        let residual = build_lossy_residual(
          src,
          pred,
          mb_index,
          segment_q,
          mode.raw4x4_sse_threshold(),
        );
        candidates.push(MbChoice {
          mode: mode_id,
          mvs: mvs.clone(),
          cost: 1
            + mv_payload_len(mode_id)
            + residual.bytes.len()
            + residual_decode_cost(&residual.bytes)
            + residual.distortion / 96,
          residual_stats: residual.stats,
          residual: residual.bytes,
          raw: Vec::new(),
          recon: residual.recon,
          segment_id,
          distortion: residual.distortion,
        });
      }
    }
  }
}

fn mv_payload_len(mode_id: u8) -> usize {
  match mode_id {
    MODE_COPY16_RES => 2,
    MODE_COPY16X8_RES => 4,
    MODE_COPY8X16_RES => 4,
    MODE_COPY8X8_RES => 8,
    _ => 0,
  }
}

fn aq_steps(base_q: i16) -> [(u8, i16); 4] {
  [
    (2, (base_q / 2).max(1)),
    (0, base_q.max(1)),
    (1, (base_q * 2).max(1)),
    (3, (base_q * 4).max(1)),
  ]
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
  recon: &mut EyeFrame, _src: &EyeFrame, _reference: &EyeFrame,
  mb_index: usize, choice: &MbChoice,
) {
  write_raw_mb_to_eye(recon, mb_index, &choice.recon);
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

struct LossyResidual {
  bytes: Vec<u8>,
  recon: Vec<u8>,
  distortion: usize,
  stats: ResidualStats,
}

struct AcBlockCandidate {
  bytes: Vec<u8>,
  values: [u8; 16],
  distortion: usize,
}

fn build_lossy_residual(
  src: &EyeFrame, pred: &EyeFrame, mb_index: usize, q_step: i16,
  raw4x4_sse_threshold: Option<usize>,
) -> LossyResidual {
  let mut recon_eye = pred.clone();
  let mut mask = 0u32;
  let mut payloads: Vec<Vec<u8>> = Vec::new();
  let mut distortion = 0usize;
  let mut stats = ResidualStats::default();

  for block in 0..16 {
    let bx = (mb_index % MB_W) * 16 + (block % 4) * 4;
    let by = (mb_index / MB_W) * 16 + (block / 4) * 4;
    if let Some(payload) = lossy_block_payload(
      &src.y,
      &pred.y,
      &mut recon_eye.y,
      EYE_W,
      bx,
      by,
      q_step,
      raw4x4_sse_threshold,
      &mut distortion,
      &mut stats,
    ) {
      mask |= 1 << block;
      payloads.push(payload);
    }
  }
  for block in 0..4 {
    let bx = (mb_index % MB_W) * 8 + (block % 2) * 4;
    let by = (mb_index / MB_W) * 8 + (block / 2) * 4;
    if let Some(payload) = lossy_block_payload(
      &src.cb,
      &pred.cb,
      &mut recon_eye.cb,
      CHROMA_W,
      bx,
      by,
      q_step,
      raw4x4_sse_threshold,
      &mut distortion,
      &mut stats,
    ) {
      mask |= 1 << (16 + block);
      payloads.push(payload);
    }
  }
  for block in 0..4 {
    let bx = (mb_index % MB_W) * 8 + (block % 2) * 4;
    let by = (mb_index / MB_W) * 8 + (block / 2) * 4;
    if let Some(payload) = lossy_block_payload(
      &src.cr,
      &pred.cr,
      &mut recon_eye.cr,
      CHROMA_W,
      bx,
      by,
      q_step,
      raw4x4_sse_threshold,
      &mut distortion,
      &mut stats,
    ) {
      mask |= 1 << (20 + block);
      payloads.push(payload);
    }
  }

  let mut bytes = Vec::new();
  bytes.extend_from_slice(&mask.to_le_bytes());
  for payload in payloads {
    bytes.extend_from_slice(&payload);
  }

  LossyResidual {
    bytes,
    recon: raw_mb_bytes(&recon_eye, mb_index),
    distortion,
    stats,
  }
}

fn lossy_block_payload(
  src: &[u8], pred: &[u8], recon: &mut [u8], stride: usize, x: usize,
  y: usize, q_step: i16, raw4x4_sse_threshold: Option<usize>,
  distortion: &mut usize, stats: &mut ResidualStats,
) -> Option<Vec<u8>> {
  let mut sum = 0i32;
  for row in 0..4 {
    for col in 0..4 {
      let idx = (y + row) * stride + x + col;
      sum += src[idx] as i32 - pred[idx] as i32;
    }
  }

  let avg = round_div_i32(sum, 16) as i16;
  let delta = quantize_delta(avg, q_step);
  let mut dc_values = [0u8; 16];
  let mut dc_distortion = 0usize;
  for row in 0..4 {
    for col in 0..4 {
      let idx = (y + row) * stride + x + col;
      let value = (pred[idx] as i16 + delta).clamp(0, 255) as u8;
      dc_values[row * 4 + col] = value;
      let err = src[idx] as i32 - value as i32;
      dc_distortion += (err * err) as usize;
    }
  }

  if raw4x4_sse_threshold.is_some_and(|threshold| dc_distortion >= threshold) {
    if let Some(ac) =
      quality_ac_mask_candidate(src, pred, stride, x, y, delta, dc_distortion)
    {
      if quality_ac_beats_raw_4x4(&ac) {
        for row in 0..4 {
          for col in 0..4 {
            let idx = (y + row) * stride + x + col;
            recon[idx] = ac.values[row * 4 + col];
          }
        }
        *distortion += ac.distortion;
        stats.ac_mask_blocks += 1;
        stats.full_idct_blocks += 1;
        return Some(ac.bytes);
      }
    }

    let mut out = Vec::with_capacity(17);
    out.push(TAG_RAW_4X4);
    for row in 0..4 {
      let src_off = (y + row) * stride + x;
      out.extend_from_slice(&src[src_off..src_off + 4]);
      let dst_off = (y + row) * stride + x;
      recon[dst_off..dst_off + 4].copy_from_slice(&src[src_off..src_off + 4]);
    }
    stats.raw_4x4_blocks += 1;
    return Some(out);
  }

  for row in 0..4 {
    for col in 0..4 {
      let idx = (y + row) * stride + x + col;
      recon[idx] = dc_values[row * 4 + col];
    }
  }
  *distortion += dc_distortion;

  if delta == 0 {
    return None;
  }

  stats.dc_only_blocks += 1;
  Some(dc_payload(delta))
}

fn quality_ac_beats_raw_4x4(ac: &AcBlockCandidate) -> bool {
  ac.bytes.len()
    + QUALITY_AC_DECODE_COST_BIAS
    + ac.distortion / QUALITY_AC_DISTORTION_COST_DIVISOR
    < QUALITY_RAW_4X4_LOCAL_COST
}

fn quality_ac_mask_candidate(
  src: &[u8], pred: &[u8], stride: usize, x: usize, y: usize, dc_delta: i16,
  dc_distortion: usize,
) -> Option<AcBlockCandidate> {
  let dc_coeff = dc_delta.checked_mul(8)?;
  if !i8::try_from(dc_coeff).is_ok() {
    return None;
  }

  let mut src_values = [0i32; 16];
  let mut pred_values = [0u8; 16];
  for row in 0..4 {
    for col in 0..4 {
      let src_idx = (y + row) * stride + x + col;
      let block_idx = row * 4 + col;
      src_values[block_idx] = src[src_idx] as i32;
      pred_values[block_idx] = pred[src_idx];
    }
  }

  let mut coeffs = [0i32; 16];
  coeffs[0] = dc_coeff as i32;
  let dc_recon = idct_reconstruct_block(&pred_values, coeffs);
  let mut residual = [0i32; 16];
  for i in 0..16 {
    residual[i] = src_values[i] - dc_recon[i] as i32;
  }

  let mut ac_choices = Vec::new();
  for zz in 1..16 {
    let natural = O3YV_ZIGZAG[zz];
    let mut basis_coeffs = [0i32; 16];
    basis_coeffs[natural] = 8;
    let basis = idct_block_deltas(basis_coeffs);
    let mut dot = 0i32;
    let mut norm = 0i32;
    for i in 0..16 {
      dot += residual[i] * basis[i];
      norm += basis[i] * basis[i];
    }
    if norm == 0 {
      continue;
    }
    let coeff = round_div_i32(dot * 8, norm).clamp(-127, 127);
    if coeff == 0 {
      continue;
    }
    let scaled = round_div_i32(coeff * dot, 8);
    let gain = (scaled * 2 - round_div_i32(coeff * coeff * norm, 64)).max(0);
    if gain > 0 {
      ac_choices.push((gain, zz, coeff));
    }
  }

  ac_choices.sort_by(|a, b| b.0.cmp(&a.0));
  let mut nz_mask = 1u16;
  let mut extra_coeffs = 0usize;
  for &(_, zz, coeff) in ac_choices.iter().take(QUALITY_AC_MAX_EXTRA_COEFFS) {
    let natural = O3YV_ZIGZAG[zz];
    coeffs[natural] = coeff;
    nz_mask |= 1 << zz;
    extra_coeffs += 1;
  }
  if extra_coeffs == 0 {
    return None;
  }

  let values = idct_reconstruct_block(&pred_values, coeffs);
  let mut distortion = 0usize;
  for i in 0..16 {
    let err = src_values[i] - values[i] as i32;
    distortion += (err * err) as usize;
  }
  if distortion > QUALITY_AC_MAX_BLOCK_SSE
    || dc_distortion.saturating_sub(distortion) < QUALITY_AC_MIN_SSE_GAIN
  {
    return None;
  }

  let payload_bytes = 1 + 2 + nz_mask.count_ones() as usize;
  if payload_bytes > QUALITY_AC_MAX_PAYLOAD_BYTES {
    return None;
  }

  let mut bytes = Vec::with_capacity(payload_bytes);
  bytes.push(TAG_AC_MASK_S8);
  bytes.extend_from_slice(&nz_mask.to_le_bytes());
  for zz in 0..16 {
    if (nz_mask & (1 << zz)) != 0 {
      let coeff = coeffs[O3YV_ZIGZAG[zz]];
      bytes.push(coeff as i8 as u8);
    }
  }
  Some(AcBlockCandidate { bytes, values, distortion })
}

fn idct_reconstruct_block(pred: &[u8; 16], coeffs: [i32; 16]) -> [u8; 16] {
  let deltas = idct_block_deltas(coeffs);
  let mut out = [0u8; 16];
  for i in 0..16 {
    out[i] = clip_u8(pred[i] as i32 + deltas[i]);
  }
  out
}

fn idct_block_deltas(coeffs: [i32; 16]) -> [i32; 16] {
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

  let mut out = [0i32; 16];
  for row in 0..4 {
    let base = row * 4;
    let a1 = tmp[base] + tmp[base + 2];
    let b1 = tmp[base] - tmp[base + 2];
    let c1 = ((tmp[base + 1] * 35468) >> 16) - tmp[base + 3];
    let d1 = tmp[base + 1] + ((tmp[base + 3] * 35468) >> 16);
    let vals = [a1 + d1, b1 + c1, b1 - c1, a1 - d1];
    for (col, value) in vals.iter().enumerate() {
      out[base + col] = (value + 4) >> 3;
    }
  }
  out
}

fn quantize_delta(delta: i16, q_step: i16) -> i16 {
  let q = q_step.max(1) as i32;
  let delta = delta as i32;
  let quantized = if delta >= 0 {
    ((delta + q / 2) / q) * q
  } else {
    -(((-delta + q / 2) / q) * q)
  };
  quantized.clamp(-255, 255) as i16
}

fn round_div_i32(value: i32, divisor: i32) -> i32 {
  if value >= 0 {
    (value + divisor / 2) / divisor
  } else {
    -((-value + divisor / 2) / divisor)
  }
}

fn clip_u8(value: i32) -> u8 {
  value.clamp(0, 255) as u8
}

fn dc_payload(delta: i16) -> Vec<u8> {
  let qdc = delta * 8;
  let mut out = Vec::new();
  if i8::try_from(qdc).is_ok() {
    out.push(TAG_DC_ONLY_S8);
    out.push(qdc as i8 as u8);
  } else {
    out.push(TAG_DC_ONLY_S16);
    out.extend_from_slice(&qdc.to_le_bytes());
  }
  out
}

fn residual_stats(residual: &[u8]) -> ResidualStats {
  let mut stats = ResidualStats::default();
  if residual.len() < 4 {
    return stats;
  }

  let mask =
    u32::from_le_bytes([residual[0], residual[1], residual[2], residual[3]]);
  let mut pos = 4usize;
  for block in 0..24 {
    if (mask & (1 << block)) == 0 {
      continue;
    }
    let Some(&tag) = residual.get(pos) else {
      break;
    };
    pos += 1;
    match tag & 0xc0 {
      0x00 => {
        stats.dc_only_blocks += 1;
        pos += 1;
      }
      0x40 => {
        stats.dc_only_blocks += 1;
        pos += 2;
      }
      0x80 => {
        stats.ac_mask_blocks += 1;
        stats.full_idct_blocks += 1;
        if pos + 2 > residual.len() {
          break;
        }
        let nz_mask = u16::from_le_bytes([residual[pos], residual[pos + 1]]);
        pos += 2 + nz_mask.count_ones() as usize;
      }
      0xc0 if (tag & 0x20) == 0 => {
        stats.ac_mask_blocks += 1;
        stats.full_idct_blocks += 1;
        if pos + 2 > residual.len() {
          break;
        }
        let nz_mask = u16::from_le_bytes([residual[pos], residual[pos + 1]]);
        pos += 2 + nz_mask.count_ones() as usize * 2;
      }
      0xc0 => {
        stats.raw_4x4_blocks += 1;
        pos += 16;
      }
      _ => {}
    }
    if pos > residual.len() {
      break;
    }
  }
  stats
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

fn raw_mb_bytes(eye: &EyeFrame, mb_index: usize) -> Vec<u8> {
  let mut out = Vec::with_capacity(384);
  read_raw_mb(eye, mb_index, &mut out);
  out
}

fn write_raw_mb_to_eye(dst: &mut EyeFrame, mb_index: usize, bytes: &[u8]) {
  debug_assert_eq!(bytes.len(), 384);
  let mb_x = mb_index % MB_W;
  let mb_y = mb_index / MB_W;
  let mut offset = 0;
  for row in 0..16 {
    let dst_off = (mb_y * 16 + row) * EYE_W + mb_x * 16;
    dst.y[dst_off..dst_off + 16].copy_from_slice(&bytes[offset..offset + 16]);
    offset += 16;
  }
  for row in 0..8 {
    let dst_off = (mb_y * 8 + row) * CHROMA_W + mb_x * 8;
    dst.cb[dst_off..dst_off + 8].copy_from_slice(&bytes[offset..offset + 8]);
    offset += 8;
  }
  for row in 0..8 {
    let dst_off = (mb_y * 8 + row) * CHROMA_W + mb_x * 8;
    dst.cr[dst_off..dst_off + 8].copy_from_slice(&bytes[offset..offset + 8]);
    offset += 8;
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
  total.raw_4x4_blocks += frame.raw_4x4_blocks;
  total.dc_only_blocks += frame.dc_only_blocks;
  total.ac_mask_blocks += frame.ac_mask_blocks;
  total.full_idct_blocks += frame.full_idct_blocks;
  total.segment_map_stream_bytes += frame.segment_map_stream_bytes;
  total.mode_stream_bytes += frame.mode_stream_bytes;
  total.residual_stream_bytes += frame.residual_stream_bytes;
  total.raw_stream_bytes += frame.raw_stream_bytes;
  total.decode_work_units += frame.decode_work_units;
  total.distortion += frame.distortion;
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

  fn noisy_frame() -> SbsFrame {
    let mut frame = SbsFrame::new();
    for y in 0..EYE_H {
      for x in 0..EYE_W {
        frame.left.y[y * EYE_W + x] =
          ((x * 37 + y * 19 + (x ^ y) * 3) & 255) as u8;
        frame.right.y[y * EYE_W + x] =
          ((x * 29 + y * 43 + (x ^ (y * 3)) * 5) & 255) as u8;
      }
    }
    for y in 0..CHROMA_H {
      for x in 0..CHROMA_W {
        frame.left.cb[y * CHROMA_W + x] = ((x * 11 + y * 7) & 255) as u8;
        frame.left.cr[y * CHROMA_W + x] = ((x * 5 + y * 17) & 255) as u8;
        frame.right.cb[y * CHROMA_W + x] = ((x * 13 + y * 3) & 255) as u8;
        frame.right.cr[y * CHROMA_W + x] = ((x * 7 + y * 23) & 255) as u8;
      }
    }
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
      encode_tile(0, &second.left, &first.left, true, EncodeMode::Lossless);
    let (right_tile, right_recon, right_stats) =
      encode_tile(1, &second.right, &first.right, true, EncodeMode::Lossless);
    assert_eq!(left_stats.base_res_mb + right_stats.base_res_mb, MB_COUNT * 2);
    write_p_frame(&mut stream, 1, left_tile, right_tile);

    let decoded = decode_stream(&stream).unwrap();
    assert_eq!(decoded[0], first);
    assert_eq!(decoded[1], SbsFrame { left: left_recon, right: right_recon });
    assert_eq!(decoded[1], second);
  }

  #[test]
  fn encoder_budget_mode_closes_loop_under_cap() {
    let first = solid_frame(40, 90, 150);
    let second = noisy_frame();
    let candidate =
      encode_budgeted_p_frame(1, &second, &first, true, 64 * 1024, &[], false);
    assert!(candidate.stats.p_frame_bytes <= 64 * 1024);
    assert_eq!(candidate.stats.raw_mb, 0);

    let mut stream = Vec::new();
    write_file_header(&mut stream, 2);
    write_key_raw_frame(&mut stream, 0, &first);
    stream.extend_from_slice(&candidate.bytes);

    let decoded = decode_stream(&stream).unwrap();
    assert_eq!(decoded[1], candidate.recon);
  }

  #[test]
  fn quality_recovery_ac_mask_matches_decoder() {
    let pred_block = [80u8; 16];
    let mut coeffs = [0i32; 16];
    coeffs[O3YV_ZIGZAG[1]] = 64;
    coeffs[O3YV_ZIGZAG[2]] = -48;
    let src_block = idct_reconstruct_block(&pred_block, coeffs);

    let pred = vec![80u8; EYE_W * EYE_H];
    let mut src = pred.clone();
    for row in 0..4 {
      src[row * EYE_W..row * EYE_W + 4]
        .copy_from_slice(&src_block[row * 4..row * 4 + 4]);
    }
    let dc_distortion = src_block
      .iter()
      .map(|&value| {
        let err = value as i32 - 80;
        (err * err) as usize
      })
      .sum();

    let ac =
      quality_ac_mask_candidate(&src, &pred, EYE_W, 0, 0, 0, dc_distortion)
        .expect("test pattern should produce an AC mask candidate");
    assert_eq!(ac.bytes[0], TAG_AC_MASK_S8);

    let first = solid_frame(80, 128, 128);
    let mut residual = 1u32.to_le_bytes().to_vec();
    residual.extend_from_slice(&ac.bytes);
    let left = EncodedTile {
      tile_id: 0,
      base_mv: Mv::ZERO,
      segment_count: 1,
      fragments: vec![EncodedFragment::full_eye(
        0,
        vec![0x80 | MODE_BASE_RES, 0x7f, 0x7f, 0x78, 0],
        residual,
        Vec::new(),
      )],
    };
    let right = EncodedTile {
      tile_id: 1,
      base_mv: Mv::ZERO,
      segment_count: 1,
      fragments: vec![EncodedFragment::full_eye(
        1,
        vec![0x7f, 0x7f, 0x79, 0],
        Vec::new(),
        Vec::new(),
      )],
    };

    let mut stream = Vec::new();
    write_file_header(&mut stream, 2);
    write_key_raw_frame(&mut stream, 0, &first);
    write_p_frame(&mut stream, 1, left, right);

    let decoded = decode_stream(&stream).unwrap();
    for row in 0..4 {
      for col in 0..4 {
        assert_eq!(
          decoded[1].left.y[row * EYE_W + col],
          ac.values[row * 4 + col]
        );
      }
    }
    assert_eq!(decoded[1].right, first.right);
  }

  #[test]
  fn quality_recovery_prefers_raw4x4_over_costly_ac() {
    let pred_block = [80u8; 16];
    let mut coeffs = [0i32; 16];
    coeffs[O3YV_ZIGZAG[1]] = 64;
    coeffs[O3YV_ZIGZAG[2]] = -48;
    let src_block = idct_reconstruct_block(&pred_block, coeffs);

    let pred = vec![80u8; EYE_W * EYE_H];
    let mut src = pred.clone();
    for row in 0..4 {
      src[row * EYE_W..row * EYE_W + 4]
        .copy_from_slice(&src_block[row * 4..row * 4 + 4]);
    }

    let mut recon = pred.clone();
    let mut distortion = 0usize;
    let mut stats = ResidualStats::default();
    let payload = lossy_block_payload(
      &src,
      &pred,
      &mut recon,
      EYE_W,
      0,
      0,
      1,
      Some(1),
      &mut distortion,
      &mut stats,
    )
    .expect("test pattern should be coded");

    assert_eq!(payload[0], TAG_RAW_4X4);
    assert_eq!(stats.raw_4x4_blocks, 1);
    assert_eq!(stats.ac_mask_blocks, 0);
    assert_eq!(distortion, 0);
    for row in 0..4 {
      assert_eq!(
        &recon[row * EYE_W..row * EYE_W + 4],
        &src_block[row * 4..row * 4 + 4]
      );
    }
  }

  #[test]
  fn scene_cut_detector_ignores_small_changes() {
    let first = solid_frame(40, 90, 150);
    let second = solid_frame(45, 90, 150);
    let metrics = source_scene_change_metrics(&second, &first);

    assert!(!scene_change_requires_keyframe(metrics));
  }

  #[test]
  fn scene_cut_detector_flags_hard_cuts() {
    let first = solid_frame(20, 90, 150);
    let second = solid_frame(190, 90, 150);
    let metrics = source_scene_change_metrics(&second, &first);

    assert!(scene_change_requires_keyframe(metrics));
  }

  #[test]
  fn raw_key_max_gap_clock_resets_after_scene_key() {
    assert!(raw_key_due_for_max_gap(0, None, 48));
    assert!(!raw_key_due_for_max_gap(47, Some(0), 48));
    assert!(raw_key_due_for_max_gap(48, Some(0), 48));
    assert!(!raw_key_due_for_max_gap(96, Some(94), 48));
    assert!(raw_key_due_for_max_gap(142, Some(94), 48));
  }
}
