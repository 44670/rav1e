use image::{Rgb, RgbImage};
use minidecoder::{
  decode_stream, SbsFrame, CHROMA_W, EYE_H, EYE_W, SBS_FRAME_BYTES, VISIBLE_H,
  VISIBLE_W,
};
use std::env;
use std::fs;
use std::io::{self, Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
  let options = parse_args()?;

  let bytes = fs::read(&options.input)?;
  let frames = decode_stream(&bytes)?;
  eprintln!("decoded {} frame(s)", frames.len());

  if let Some(output) = options.output {
    let mut out = Vec::with_capacity(frames.len() * SBS_FRAME_BYTES);
    for frame in &frames {
      out.extend_from_slice(&frame.to_yuv420_sbs());
    }
    if output == "-" {
      io::stdout().write_all(&out)?;
    } else {
      fs::write(output, out)?;
    }
  }

  if let Some(png_dir) = options.png_dir {
    fs::create_dir_all(&png_dir)?;
    for (index, frame) in frames.iter().enumerate() {
      let path = png_dir.join(format!("frame_{index:06}.png"));
      frame_to_rgb(frame).save(&path)?;
      eprintln!("wrote {}", path.display());
    }
  }

  Ok(())
}

struct Options {
  input: String,
  output: Option<String>,
  png_dir: Option<std::path::PathBuf>,
}

fn parse_args() -> Result<Options, Box<dyn std::error::Error>> {
  let mut input = None;
  let mut output = None;
  let mut png_dir = None;

  let mut args = env::args().skip(1);
  while let Some(arg) = args.next() {
    match arg.as_str() {
      "--png-dir" => {
        png_dir =
          Some(args.next().ok_or("--png-dir requires a directory")?.into());
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

  Ok(Options { input, output, png_dir })
}

fn print_usage() {
  eprintln!("usage: minidecoder <input.o3yv> [output.yuv] [--png-dir DIR]");
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
