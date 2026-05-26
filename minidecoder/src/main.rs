use minidecoder::{decode_stream, SBS_FRAME_BYTES};
use std::env;
use std::fs;
use std::io::{self, Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
  let mut args = env::args().skip(1);
  let input =
    args.next().ok_or("usage: minidecoder <input.o3yv> [output.yuv]")?;
  let output = args.next();

  let bytes = fs::read(&input)?;
  let frames = decode_stream(&bytes)?;
  eprintln!("decoded {} frame(s)", frames.len());

  if let Some(output) = output {
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

  Ok(())
}
