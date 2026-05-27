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
}

fn parse_args() -> Result<Options, Box<dyn std::error::Error>> {
  let mut input = None;
  let mut output = None;
  let mut png_dir = None;
  let mut bench_iters = None;

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

  Ok(Options { input, output, png_dir, bench_iters })
}

fn print_usage() {
  eprintln!(
    "usage: minidecoder <input.o3yv> [output.yuv] [--png-dir DIR] [--bench N]"
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
