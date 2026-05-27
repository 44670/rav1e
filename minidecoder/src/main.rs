use image::{Rgb, RgbImage};
use minidecoder::{
  decode_stream_for_each, decode_stream_with_metadata, SbsFrame, CHROMA_W,
  EYE_H, EYE_W, FRAME_TYPE_KEY_RAW, FRAME_TYPE_P, SBS_FRAME_BYTES, VISIBLE_H,
  VISIBLE_W,
};
use std::env;
use std::fs;
use std::io::{self, Write};
use std::time::{Duration, Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
  let options = parse_args()?;

  let bytes = fs::read(&options.input)?;
  #[cfg(not(feature = "stats"))]
  if options.stats {
    return Err(
      "--stats requires building minidecoder with --features stats".into(),
    );
  }

  #[cfg(feature = "stats")]
  if options.stats {
    print_stream_stats(&collect_stream_stats(&bytes)?)?;
    if options.bench_iters.is_none()
      && options.output.is_none()
      && options.png_dir.is_none()
    {
      return Ok(());
    }
  }

  if let Some(iterations) = options.bench_iters {
    if options.output.is_some() || options.png_dir.is_some() {
      return Err(
        "--bench cannot be combined with output.yuv or --png-dir".into(),
      );
    }
    run_bench(&bytes, iterations)?;
    return Ok(());
  }

  if options.output.is_none() && options.png_dir.is_none() {
    let frame_count = decode_stream_for_each(&bytes, |_| {})?;
    eprintln!("decoded {frame_count} frame(s)");
    return Ok(());
  }

  let frames = decode_stream_with_metadata(&bytes)?;
  eprintln!("decoded {} frame(s)", frames.len());

  if let Some(output) = options.output {
    let mut out = Vec::with_capacity(frames.len() * SBS_FRAME_BYTES);
    for decoded in &frames {
      out.extend_from_slice(&decoded.frame.to_yuv420_sbs());
    }
    if output == "-" {
      io::stdout().write_all(&out)?;
    } else {
      fs::write(output, out)?;
    }
  }

  if let Some(png_dir) = options.png_dir {
    fs::create_dir_all(&png_dir)?;
    for (index, decoded) in frames.iter().enumerate() {
      let kind = png_frame_kind(decoded.frame_type)?;
      let path = png_dir.join(format!("f_{index:05}_{kind}.png"));
      frame_to_rgb(&decoded.frame).save(&path)?;
      eprintln!("wrote {}", path.display());
    }
  }

  Ok(())
}

struct Options {
  input: String,
  output: Option<String>,
  png_dir: Option<std::path::PathBuf>,
  bench_iters: Option<usize>,
  stats: bool,
}

fn parse_args() -> Result<Options, Box<dyn std::error::Error>> {
  let mut input = None;
  let mut output = None;
  let mut png_dir = None;
  let mut bench_iters = None;
  let mut stats = false;

  let mut args = env::args().skip(1);
  while let Some(arg) = args.next() {
    match arg.as_str() {
      "--png-dir" => {
        png_dir =
          Some(args.next().ok_or("--png-dir requires a directory")?.into());
      }
      "--bench" => {
        let iterations = args
          .next()
          .ok_or("--bench requires an iteration count")?
          .parse::<usize>()?;
        if iterations == 0 {
          return Err("--bench iteration count must be positive".into());
        }
        bench_iters = Some(iterations);
      }
      "--stats" => {
        stats = true;
      }
      "-h" | "--help" => {
        print_usage();
        std::process::exit(0);
      }
      _ if input.is_none() => input = Some(arg),
      _ if output.is_none() => output = Some(arg),
      _ => return Err(format!("unexpected argument {arg}").into()),
    }
  }

  let Some(input) = input else {
    print_usage();
    return Err("missing input file".into());
  };

  Ok(Options { input, output, png_dir, bench_iters, stats })
}

fn print_usage() {
  let stats = if cfg!(feature = "stats") { " [--stats]" } else { "" };
  eprintln!(
    "usage: minidecoder <input.o3yv> [output.yuv] [--png-dir DIR] [--bench N]{stats}"
  );
}

fn run_bench(
  bytes: &[u8], iterations: usize,
) -> Result<(), Box<dyn std::error::Error>> {
  let mut times = Vec::with_capacity(iterations);
  let mut frames_per_iter = None;

  for _ in 0..iterations {
    let start = Instant::now();
    let frames = decode_stream_for_each(bytes, |_| {})?;
    let elapsed = start.elapsed();
    if let Some(expected) = frames_per_iter {
      if frames != expected {
        return Err(
          "decoded frame count changed across bench iterations".into(),
        );
      }
    } else {
      frames_per_iter = Some(frames);
    }
    times.push(elapsed);
  }

  let frames = frames_per_iter.unwrap_or(0);
  if frames == 0 {
    return Err("cannot benchmark an empty stream".into());
  }

  times.sort_unstable();
  let mean = mean_duration(&times);
  let min = times[0];
  let median = percentile_duration(&times, 50);
  let p95 = percentile_duration(&times, 95);
  let p99 = percentile_duration(&times, 99);
  let max = times[times.len() - 1];

  eprintln!("bench_iterations={iterations}");
  eprintln!("frames_per_iteration={frames}");
  eprintln!(
    "ms_per_frame mean={:.3} min={:.3} median={:.3} p95={:.3} p99={:.3} max={:.3}",
    ms_per_frame(mean, frames),
    ms_per_frame(min, frames),
    ms_per_frame(median, frames),
    ms_per_frame(p95, frames),
    ms_per_frame(p99, frames),
    ms_per_frame(max, frames),
  );
  Ok(())
}

fn mean_duration(times: &[Duration]) -> Duration {
  let total_secs = times.iter().map(Duration::as_secs_f64).sum::<f64>();
  Duration::from_secs_f64(total_secs / times.len() as f64)
}

fn percentile_duration(times: &[Duration], percentile: usize) -> Duration {
  let len = times.len();
  let rank = (percentile * len).div_ceil(100).saturating_sub(1);
  times[rank.min(len - 1)]
}

fn ms_per_frame(duration: Duration, frames: usize) -> f64 {
  duration.as_secs_f64() * 1000.0 / frames as f64
}

#[cfg(feature = "stats")]
#[derive(Default)]
struct StreamStats {
  frames: usize,
  key_raw_frames: usize,
  p_frames: usize,
  frame_payload_bytes: usize,
  p_payload_bytes: usize,
  max_frame_payload_bytes: usize,
  max_p_payload_bytes: usize,
  tiles: usize,
  fragments: usize,
  segment_map_bytes: usize,
  mode_bytes: usize,
  residual_bytes: usize,
  raw_bytes: usize,
  prefill_zero_tiles: usize,
  prefill_shift_tiles: usize,
  skip_mb: usize,
  copy16_mb: usize,
  copy_vbs_mb: usize,
  raw_mb: usize,
  residual_mb: usize,
  dc_only_blocks: usize,
  full_idct_blocks: usize,
  raw_4x4_blocks: usize,
  p_work_units: usize,
  max_p_work_units: usize,
  max_p_segment_map_bytes: usize,
  max_p_mode_bytes: usize,
  max_p_residual_bytes: usize,
  max_p_raw_bytes: usize,
  max_p_skip_mb: usize,
  max_p_copy16_mb: usize,
  max_p_copy_vbs_mb: usize,
  max_p_raw_mb: usize,
  max_p_residual_mb: usize,
  max_p_dc_only_blocks: usize,
  max_p_full_idct_blocks: usize,
  max_p_raw_4x4_blocks: usize,
}

#[cfg(feature = "stats")]
#[derive(Clone, Copy, Default)]
struct FrameWorkload {
  segment_map_bytes: usize,
  mode_bytes: usize,
  residual_bytes: usize,
  raw_bytes: usize,
  skip_mb: usize,
  copy16_mb: usize,
  copy_vbs_mb: usize,
  raw_mb: usize,
  residual_mb: usize,
  dc_only_blocks: usize,
  full_idct_blocks: usize,
  raw_4x4_blocks: usize,
}

#[cfg(feature = "stats")]
fn print_stream_stats(
  stats: &StreamStats,
) -> Result<(), Box<dyn std::error::Error>> {
  eprintln!(
    "stats frames={} key_raw={} p={}",
    stats.frames, stats.key_raw_frames, stats.p_frames
  );
  eprintln!(
    "stats frame_payload_bytes={} p_payload_bytes={} max_frame_payload_bytes={} max_p_payload_bytes={}",
    stats.frame_payload_bytes,
    stats.p_payload_bytes,
    stats.max_frame_payload_bytes,
    stats.max_p_payload_bytes
  );
  eprintln!(
    "stats tiles={} fragments={} prefill_zero_tiles={} prefill_shift_tiles={}",
    stats.tiles,
    stats.fragments,
    stats.prefill_zero_tiles,
    stats.prefill_shift_tiles
  );
  eprintln!(
    "stats stream_bytes segment_map={} mode={} residual={} raw={}",
    stats.segment_map_bytes,
    stats.mode_bytes,
    stats.residual_bytes,
    stats.raw_bytes
  );
  eprintln!(
    "stats mb skip={} copy16={} copy_vbs={} raw={} residual={}",
    stats.skip_mb,
    stats.copy16_mb,
    stats.copy_vbs_mb,
    stats.raw_mb,
    stats.residual_mb
  );
  eprintln!(
    "stats blocks dc_only={} full_idct={} raw_4x4={}",
    stats.dc_only_blocks, stats.full_idct_blocks, stats.raw_4x4_blocks
  );
  eprintln!(
    "stats max_p_stream_bytes segment_map={} mode={} residual={} raw={}",
    stats.max_p_segment_map_bytes,
    stats.max_p_mode_bytes,
    stats.max_p_residual_bytes,
    stats.max_p_raw_bytes
  );
  eprintln!(
    "stats max_p_mb skip={} copy16={} copy_vbs={} raw={} residual={}",
    stats.max_p_skip_mb,
    stats.max_p_copy16_mb,
    stats.max_p_copy_vbs_mb,
    stats.max_p_raw_mb,
    stats.max_p_residual_mb
  );
  eprintln!(
    "stats max_p_blocks dc_only={} full_idct={} raw_4x4={}",
    stats.max_p_dc_only_blocks,
    stats.max_p_full_idct_blocks,
    stats.max_p_raw_4x4_blocks
  );
  if stats.p_frames > 0 {
    eprintln!(
      "stats estimated_work_units p_total={} p_avg={} p_max={} old3ds_15ms_cycles_268mhz={}",
      stats.p_work_units,
      stats.p_work_units / stats.p_frames,
      stats.max_p_work_units,
      268_000_000usize * 15 / 1000
    );
  }
  Ok(())
}

#[cfg(feature = "stats")]
impl StreamStats {
  fn workload_snapshot(&self) -> FrameWorkload {
    FrameWorkload {
      segment_map_bytes: self.segment_map_bytes,
      mode_bytes: self.mode_bytes,
      residual_bytes: self.residual_bytes,
      raw_bytes: self.raw_bytes,
      skip_mb: self.skip_mb,
      copy16_mb: self.copy16_mb,
      copy_vbs_mb: self.copy_vbs_mb,
      raw_mb: self.raw_mb,
      residual_mb: self.residual_mb,
      dc_only_blocks: self.dc_only_blocks,
      full_idct_blocks: self.full_idct_blocks,
      raw_4x4_blocks: self.raw_4x4_blocks,
    }
  }

  fn record_p_workload(&mut self, before: FrameWorkload, tile_count: usize) {
    let after = self.workload_snapshot();
    let frame = FrameWorkload {
      segment_map_bytes: after.segment_map_bytes - before.segment_map_bytes,
      mode_bytes: after.mode_bytes - before.mode_bytes,
      residual_bytes: after.residual_bytes - before.residual_bytes,
      raw_bytes: after.raw_bytes - before.raw_bytes,
      skip_mb: after.skip_mb - before.skip_mb,
      copy16_mb: after.copy16_mb - before.copy16_mb,
      copy_vbs_mb: after.copy_vbs_mb - before.copy_vbs_mb,
      raw_mb: after.raw_mb - before.raw_mb,
      residual_mb: after.residual_mb - before.residual_mb,
      dc_only_blocks: after.dc_only_blocks - before.dc_only_blocks,
      full_idct_blocks: after.full_idct_blocks - before.full_idct_blocks,
      raw_4x4_blocks: after.raw_4x4_blocks - before.raw_4x4_blocks,
    };
    let work_units = estimate_p_work_units(&frame, tile_count);
    self.p_work_units += work_units;
    self.max_p_work_units = self.max_p_work_units.max(work_units);
    self.max_p_segment_map_bytes =
      self.max_p_segment_map_bytes.max(frame.segment_map_bytes);
    self.max_p_mode_bytes = self.max_p_mode_bytes.max(frame.mode_bytes);
    self.max_p_residual_bytes =
      self.max_p_residual_bytes.max(frame.residual_bytes);
    self.max_p_raw_bytes = self.max_p_raw_bytes.max(frame.raw_bytes);
    self.max_p_skip_mb = self.max_p_skip_mb.max(frame.skip_mb);
    self.max_p_copy16_mb = self.max_p_copy16_mb.max(frame.copy16_mb);
    self.max_p_copy_vbs_mb = self.max_p_copy_vbs_mb.max(frame.copy_vbs_mb);
    self.max_p_raw_mb = self.max_p_raw_mb.max(frame.raw_mb);
    self.max_p_residual_mb = self.max_p_residual_mb.max(frame.residual_mb);
    self.max_p_dc_only_blocks =
      self.max_p_dc_only_blocks.max(frame.dc_only_blocks);
    self.max_p_full_idct_blocks =
      self.max_p_full_idct_blocks.max(frame.full_idct_blocks);
    self.max_p_raw_4x4_blocks =
      self.max_p_raw_4x4_blocks.max(frame.raw_4x4_blocks);
  }
}

#[cfg(feature = "stats")]
fn estimate_p_work_units(frame: &FrameWorkload, tile_count: usize) -> usize {
  let stream_parse_units = frame.segment_map_bytes
    + frame.mode_bytes * 2
    + frame.residual_bytes * 2
    + frame.raw_bytes;
  let prefill_units = tile_count * minidecoder::EYE_FRAME_BYTES;
  let prediction_units =
    (frame.copy16_mb + frame.copy_vbs_mb) * minidecoder::RAW_MB_BYTES;
  let raw_units =
    frame.raw_mb * minidecoder::RAW_MB_BYTES + frame.raw_4x4_blocks * 16;
  let residual_units = frame.residual_mb * 8
    + frame.dc_only_blocks * 64
    + frame.full_idct_blocks * 512;
  stream_parse_units
    + prefill_units
    + prediction_units
    + raw_units
    + residual_units
}

#[cfg(feature = "stats")]
fn collect_stream_stats(
  bytes: &[u8],
) -> Result<StreamStats, Box<dyn std::error::Error>> {
  const FILE_MAGIC: u32 = u32::from_le_bytes(*b"O3YV");
  const FRAME_MAGIC: u32 = u32::from_le_bytes(*b"FRM1");
  const FILE_HEADER_SIZE: usize = 60;
  const FRAME_HEADER_SIZE: usize = 28;

  let mut stats = StreamStats::default();
  let mut r = StatsReader::new(bytes);
  if r.u32()? != FILE_MAGIC {
    return Err("bad file magic".into());
  }
  let _major = r.u16()?;
  let _minor = r.u16()?;
  let header_size = r.u16()? as usize;
  if header_size < FILE_HEADER_SIZE {
    return Err("file header too small".into());
  }
  r.skip(FILE_HEADER_SIZE - 10)?;
  if header_size > FILE_HEADER_SIZE {
    r.skip(header_size - FILE_HEADER_SIZE)?;
  }

  while r.remaining() > 0 {
    let frame_start = r.pos;
    if r.u32()? != FRAME_MAGIC {
      return Err("bad frame start code".into());
    }
    let frame_size = r.u32()? as usize;
    let _frame_no = r.u32()?;
    let _pts_ticks = r.u32()?;
    let frame_type = r.u8()?;
    let tile_count = r.u8()?;
    let flags = r.u8()?;
    let reserved = r.u8()?;
    let _crc = r.u32()?;
    let _cost = r.u32()?;
    if flags != 0 || reserved != 0 {
      return Err("reserved frame bits set".into());
    }

    let payload_start = frame_start + FRAME_HEADER_SIZE;
    let payload_end =
      payload_start.checked_add(frame_size).ok_or("frame size overflow")?;
    if payload_end > bytes.len() {
      return Err("frame payload exceeds stream".into());
    }

    stats.frames += 1;
    stats.frame_payload_bytes += frame_size;
    stats.max_frame_payload_bytes =
      stats.max_frame_payload_bytes.max(frame_size);

    match frame_type {
      FRAME_TYPE_KEY_RAW => {
        if tile_count != 2 || frame_size != SBS_FRAME_BYTES {
          return Err("invalid KEY_RAW frame".into());
        }
        stats.key_raw_frames += 1;
      }
      FRAME_TYPE_P => {
        if tile_count != 2 {
          return Err("invalid P-frame tile count".into());
        }
        stats.p_frames += 1;
        stats.p_payload_bytes += frame_size;
        stats.max_p_payload_bytes = stats.max_p_payload_bytes.max(frame_size);
        let before = stats.workload_snapshot();
        let mut pr = StatsReader::new(&bytes[payload_start..payload_end]);
        for _ in 0..2 {
          collect_tile_stats(&mut pr, &mut stats)?;
        }
        if pr.remaining() != 0 {
          return Err("unconsumed P-frame payload".into());
        }
        stats.record_p_workload(before, tile_count as usize);
      }
      _ => return Err(format!("unsupported frame type {frame_type}").into()),
    }

    r.pos = payload_end;
  }

  Ok(stats)
}

#[cfg(feature = "stats")]
fn collect_tile_stats(
  r: &mut StatsReader<'_>, stats: &mut StreamStats,
) -> Result<(), Box<dyn std::error::Error>> {
  let tile_id = r.u8()?;
  let mb_x = r.u8()?;
  let mb_y = r.u8()?;
  let mb_w = r.u8()?;
  let mb_h = r.u8()?;
  let mv_x = r.i8()?;
  let mv_y = r.i8()?;
  let q_y = r.u8()?;
  let q_uv = r.u8()?;
  let segment_count = r.u8()?;
  let fragment_count = r.u8()?;
  let tile_flags = r.u16()?;
  let payload_size = r.u32()? as usize;

  if tile_id > 1
    || mb_x != 0
    || mb_y != 0
    || mb_w as usize != minidecoder::MB_W
    || mb_h as usize != minidecoder::MB_H
    || q_y > 127
    || q_uv > 127
    || !(1..=4).contains(&segment_count)
    || fragment_count == 0
    || tile_flags != 0
  {
    return Err("invalid tile header".into());
  }
  if payload_size > r.remaining() {
    return Err("tile payload exceeds frame".into());
  }

  stats.tiles += 1;
  if mv_x == 0 && mv_y == 0 {
    stats.prefill_zero_tiles += 1;
  } else {
    stats.prefill_shift_tiles += 1;
  }

  let payload = r.take(payload_size)?;
  let mut tr = StatsReader::new(payload);
  for _ in 0..fragment_count {
    collect_fragment_stats(&mut tr, tile_id, segment_count, stats)?;
  }
  if tr.remaining() != 0 {
    return Err("unconsumed tile payload".into());
  }
  Ok(())
}

#[cfg(feature = "stats")]
fn collect_fragment_stats(
  r: &mut StatsReader<'_>, tile_id: u8, segment_count: u8,
  stats: &mut StreamStats,
) -> Result<(), Box<dyn std::error::Error>> {
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
    return Err("invalid fragment header".into());
  }
  if start_mb >= minidecoder::MB_COUNT
    || mb_count == 0
    || start_mb + mb_count > minidecoder::MB_COUNT
    || row_start as usize >= minidecoder::MB_H
    || row_count == 0
    || row_start as usize + row_count as usize > minidecoder::MB_H
    || start_mb != row_start as usize * minidecoder::MB_W
    || mb_count != row_count as usize * minidecoder::MB_W
  {
    return Err("invalid fragment range".into());
  }

  let segment_map = r.take(segment_map_size)?;
  collect_segment_map_stats(segment_map, segment_count, mb_count)?;
  let mode_stream = r.take(mode_size)?;
  let residual_stream = r.take(residual_size)?;
  let raw_stream = r.take(raw_size)?;

  stats.fragments += 1;
  stats.segment_map_bytes += segment_map_size;
  stats.mode_bytes += mode_size;
  stats.residual_bytes += residual_size;
  stats.raw_bytes += raw_size;

  let mut mr = StatsReader::new(mode_stream);
  let mut rr = StatsReader::new(residual_stream);
  let mut raw = StatsReader::new(raw_stream);
  let end_mb = start_mb + mb_count;
  let mut mb_index = start_mb;

  while mb_index < end_mb {
    let op = mr.u8()?;
    if op == 0 {
      break;
    } else if op <= 0x7f {
      let run = op as usize;
      if mb_index + run > end_mb {
        return Err("skip run exceeds fragment".into());
      }
      stats.skip_mb += run;
      mb_index += run;
    } else if (op & 0xf0) == 0x80 {
      collect_mb_stats(op & 0x0f, &mut mr, &mut rr, &mut raw, stats)?;
      mb_index += 1;
    } else if (op & 0xf0) == 0x90 {
      let mode = op & 0x0f;
      let run = mr.u8()? as usize + 1;
      if mb_index + run > end_mb {
        return Err("mode run exceeds fragment".into());
      }
      for _ in 0..run {
        collect_mb_stats(mode, &mut mr, &mut rr, &mut raw, stats)?;
        mb_index += 1;
      }
    } else {
      return Err("reserved mode opcode".into());
    }
  }

  if mb_index != end_mb {
    return Err("fragment ended early".into());
  }
  if mr.remaining() == 1 && mr.peek_u8()? == 0 {
    mr.skip(1)?;
  }
  if mr.remaining() != 0 || rr.remaining() != 0 || raw.remaining() != 0 {
    return Err("fragment streams were not fully consumed".into());
  }
  Ok(())
}

#[cfg(feature = "stats")]
fn collect_segment_map_stats(
  bytes: &[u8], segment_count: u8, mb_count: usize,
) -> Result<(), Box<dyn std::error::Error>> {
  if segment_count == 1 {
    if !bytes.is_empty() {
      return Err("segment map present when segment_count is 1".into());
    }
    return Ok(());
  }
  let mut r = StatsReader::new(bytes);
  let mut described = 0usize;
  while described < mb_count {
    let run = r.u8()? as usize + 1;
    let segment_id = r.u8()?;
    if segment_id >= segment_count {
      return Err("segment id out of range".into());
    }
    described = described.checked_add(run).ok_or("segment map overflow")?;
    if described > mb_count {
      return Err("segment map exceeds fragment".into());
    }
  }
  if r.remaining() != 0 {
    return Err("segment map was not fully consumed".into());
  }
  Ok(())
}

#[cfg(feature = "stats")]
fn collect_mb_stats(
  mode: u8, mode_stream: &mut StatsReader<'_>, residual: &mut StatsReader<'_>,
  raw: &mut StatsReader<'_>, stats: &mut StreamStats,
) -> Result<(), Box<dyn std::error::Error>> {
  match mode {
    minidecoder::MODE_BASE_RES => {
      stats.residual_mb += 1;
      collect_residual_stats(residual, stats)
    }
    minidecoder::MODE_COPY16 => {
      stats.copy16_mb += 1;
      mode_stream.skip(2)?;
      Ok(())
    }
    minidecoder::MODE_COPY16_RES => {
      stats.copy16_mb += 1;
      stats.residual_mb += 1;
      mode_stream.skip(2)?;
      collect_residual_stats(residual, stats)
    }
    minidecoder::MODE_COPY16X8 | minidecoder::MODE_COPY8X16 => {
      stats.copy_vbs_mb += 1;
      mode_stream.skip(4)?;
      Ok(())
    }
    minidecoder::MODE_COPY16X8_RES | minidecoder::MODE_COPY8X16_RES => {
      stats.copy_vbs_mb += 1;
      stats.residual_mb += 1;
      mode_stream.skip(4)?;
      collect_residual_stats(residual, stats)
    }
    minidecoder::MODE_COPY8X8 => {
      stats.copy_vbs_mb += 1;
      mode_stream.skip(8)?;
      Ok(())
    }
    minidecoder::MODE_COPY8X8_RES => {
      stats.copy_vbs_mb += 1;
      stats.residual_mb += 1;
      mode_stream.skip(8)?;
      collect_residual_stats(residual, stats)
    }
    minidecoder::MODE_RAW_MB => {
      stats.raw_mb += 1;
      raw.skip(minidecoder::RAW_MB_BYTES)?;
      Ok(())
    }
    _ => Err(format!("unsupported MB mode {mode}").into()),
  }
}

#[cfg(feature = "stats")]
fn collect_residual_stats(
  residual: &mut StatsReader<'_>, stats: &mut StreamStats,
) -> Result<(), Box<dyn std::error::Error>> {
  let mask = residual.u32()?;
  if (mask & 0xff00_0000) != 0 {
    return Err("coded block mask uses reserved bits".into());
  }
  let mut coded = mask;
  while coded != 0 {
    let _block = coded.trailing_zeros();
    coded &= coded - 1;
    collect_block_stats(residual, stats)?;
  }
  Ok(())
}

#[cfg(feature = "stats")]
fn collect_block_stats(
  residual: &mut StatsReader<'_>, stats: &mut StreamStats,
) -> Result<(), Box<dyn std::error::Error>> {
  let tag = residual.u8()?;
  match tag & 0xc0 {
    0x00 => {
      if tag != minidecoder::TAG_DC_ONLY_S8 {
        return Err("reserved DC_ONLY_S8 tag bits set".into());
      }
      stats.dc_only_blocks += 1;
      residual.skip(1)?;
    }
    0x40 => {
      if tag != minidecoder::TAG_DC_ONLY_S16 {
        return Err("reserved DC_ONLY_S16 tag bits set".into());
      }
      stats.dc_only_blocks += 1;
      residual.skip(2)?;
    }
    0x80 => {
      if tag != minidecoder::TAG_AC_MASK_S8 {
        return Err("reserved AC_MASK_S8 tag bits set".into());
      }
      stats.full_idct_blocks += 1;
      let nz = residual.u16()?;
      residual.skip(nz.count_ones() as usize)?;
    }
    0xc0 => {
      if (tag & 0x20) == 0 {
        if tag != minidecoder::TAG_AC_MASK_S16 {
          return Err("reserved AC_MASK_S16 tag bits set".into());
        }
        stats.full_idct_blocks += 1;
        let nz = residual.u16()?;
        residual.skip(nz.count_ones() as usize * 2)?;
      } else {
        if tag != minidecoder::TAG_RAW_4X4 {
          return Err("reserved RAW_4X4 tag bits set".into());
        }
        stats.raw_4x4_blocks += 1;
        residual.skip(16)?;
      }
    }
    _ => unreachable!(),
  }
  Ok(())
}

#[cfg(feature = "stats")]
struct StatsReader<'a> {
  bytes: &'a [u8],
  pos: usize,
}

#[cfg(feature = "stats")]
impl<'a> StatsReader<'a> {
  fn new(bytes: &'a [u8]) -> Self {
    Self { bytes, pos: 0 }
  }

  fn remaining(&self) -> usize {
    self.bytes.len() - self.pos
  }

  fn take(
    &mut self, n: usize,
  ) -> Result<&'a [u8], Box<dyn std::error::Error>> {
    if n > self.remaining() {
      return Err("unexpected end of stream".into());
    }
    let start = self.pos;
    self.pos += n;
    Ok(&self.bytes[start..start + n])
  }

  fn skip(&mut self, n: usize) -> Result<(), Box<dyn std::error::Error>> {
    self.take(n).map(|_| ())
  }

  fn peek_u8(&self) -> Result<u8, Box<dyn std::error::Error>> {
    self
      .bytes
      .get(self.pos)
      .copied()
      .ok_or_else(|| "unexpected end of stream".into())
  }

  fn u8(&mut self) -> Result<u8, Box<dyn std::error::Error>> {
    let value = self.peek_u8()?;
    self.pos += 1;
    Ok(value)
  }

  fn i8(&mut self) -> Result<i8, Box<dyn std::error::Error>> {
    Ok(self.u8()? as i8)
  }

  fn u16(&mut self) -> Result<u16, Box<dyn std::error::Error>> {
    let bytes = self.take(2)?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
  }

  fn u32(&mut self) -> Result<u32, Box<dyn std::error::Error>> {
    let bytes = self.take(4)?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
  }
}

fn png_frame_kind(
  frame_type: u8,
) -> Result<&'static str, Box<dyn std::error::Error>> {
  match frame_type {
    FRAME_TYPE_KEY_RAW => Ok("raw"),
    FRAME_TYPE_P => Ok("p"),
    _ => Err(format!("unsupported frame type {frame_type}").into()),
  }
}

fn frame_to_rgb(frame: &SbsFrame) -> RgbImage {
  let mut image = RgbImage::new(VISIBLE_W as u32, VISIBLE_H as u32);
  draw_eye(&mut image, 0, &frame.left);
  draw_eye(&mut image, EYE_W, &frame.right);
  image
}

fn draw_eye(
  image: &mut RgbImage, x_offset: usize, eye: &minidecoder::EyeFrame,
) {
  for y in 0..EYE_H {
    for x in 0..EYE_W {
      let y_sample = eye.y[y * EYE_W + x];
      let cb = eye.cb[(y / 2) * CHROMA_W + (x / 2)];
      let cr = eye.cr[(y / 2) * CHROMA_W + (x / 2)];
      image.put_pixel(
        (x_offset + x) as u32,
        y as u32,
        yuv_to_rgb(y_sample, cb, cr),
      );
    }
  }
}

fn yuv_to_rgb(y: u8, cb: u8, cr: u8) -> Rgb<u8> {
  let y = (y as i32 - 16).max(0);
  let cb = cb as i32 - 128;
  let cr = cr as i32 - 128;

  // BT.709 limited-range integer approximation. O3YV currently writes this
  // color model in the file header for host validation images.
  let r = (19077 * y + 29372 * cr + 8192) >> 14;
  let g = (19077 * y - 3494 * cb - 8739 * cr + 8192) >> 14;
  let b = (19077 * y + 34610 * cb + 8192) >> 14;

  Rgb([clip_u8(r), clip_u8(g), clip_u8(b)])
}

fn clip_u8(value: i32) -> u8 {
  value.clamp(0, 255) as u8
}
