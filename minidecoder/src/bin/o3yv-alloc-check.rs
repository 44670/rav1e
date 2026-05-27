use minidecoder::StreamDecoder;
use std::alloc::{GlobalAlloc, Layout, System};
use std::env;
use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};

struct CountingAlloc;

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

#[global_allocator]
static GLOBAL: CountingAlloc = CountingAlloc;

unsafe impl GlobalAlloc for CountingAlloc {
  unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
    let ptr = unsafe { System.alloc(layout) };
    if !ptr.is_null() {
      ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
    }
    ptr
  }

  unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
    unsafe { System.dealloc(ptr, layout) };
  }

  unsafe fn realloc(
    &self, ptr: *mut u8, layout: Layout, new_size: usize,
  ) -> *mut u8 {
    let new_ptr = unsafe { System.realloc(ptr, layout, new_size) };
    if !new_ptr.is_null() {
      ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
    }
    new_ptr
  }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
  let input = env::args()
    .nth(1)
    .ok_or_else(|| "usage: o3yv-alloc-check <input.o3yv>".to_string())?;
  let bytes = fs::read(input)?;
  let mut decoder = StreamDecoder::new(&bytes)?;

  ALLOCATIONS.store(0, Ordering::Relaxed);
  decoder.reset()?;
  let mut frames = 0usize;
  while decoder.next_frame()?.is_some() {
    frames += 1;
  }
  let allocations = ALLOCATIONS.load(Ordering::Relaxed);
  if allocations != 0 {
    return Err(
      format!(
        "decode allocated {allocations} time(s) after DecoderState init"
      )
      .into(),
    );
  }

  eprintln!("alloc_check frames={frames} decode_allocations=0");
  Ok(())
}
