#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

#[cfg(not(feature = "std"))]
use core::alloc::{GlobalAlloc, Layout};
use core::ffi::c_void;
use core::mem::{align_of, size_of};
use core::{ptr, slice};
use minidecoder::{Error, StreamDecoder, EYE_FRAME_BYTES};

const O3YV_DONE: i32 = 0;
const O3YV_FRAME: i32 = 1;
const O3YV_ERR_NULL: i32 = -1;
const O3YV_ERR_STORAGE: i32 = -2;
const O3YV_ERR_EOF: i32 = -3;
const O3YV_ERR_INVALID: i32 = -4;
const O3YV_ERR_UNSUPPORTED: i32 = -5;

#[cfg(not(feature = "std"))]
unsafe extern "C" {
  fn malloc(size: usize) -> *mut c_void;
  fn realloc(ptr: *mut c_void, size: usize) -> *mut c_void;
  fn free(ptr: *mut c_void);
}

#[cfg(not(feature = "std"))]
struct CAllocator;

#[cfg(not(feature = "std"))]
unsafe impl GlobalAlloc for CAllocator {
  unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
    if layout.align() > align_of::<usize>() {
      return ptr::null_mut();
    }
    unsafe { malloc(layout.size().max(1)).cast::<u8>() }
  }

  unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
    unsafe { free(ptr.cast::<c_void>()) };
  }

  unsafe fn realloc(
    &self,
    ptr: *mut u8,
    layout: Layout,
    new_size: usize,
  ) -> *mut u8 {
    if layout.align() > align_of::<usize>() {
      return ptr::null_mut();
    }
    unsafe { realloc(ptr.cast::<c_void>(), new_size.max(1)).cast::<u8>() }
  }
}

#[cfg(not(feature = "std"))]
#[global_allocator]
static ALLOCATOR: CAllocator = CAllocator;

#[cfg(not(feature = "std"))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo<'_>) -> ! {
  loop {
    core::hint::spin_loop();
  }
}

#[repr(C)]
pub struct O3yvFrameInfo {
  frame_no: u32,
  frame_type: u8,
  reserved: [u8; 3],
}

#[no_mangle]
pub extern "C" fn o3yv_decoder_size() -> usize {
  size_of::<StreamDecoder<'static>>()
}

#[no_mangle]
pub extern "C" fn o3yv_decoder_align() -> usize {
  align_of::<StreamDecoder<'static>>()
}

#[no_mangle]
pub extern "C" fn o3yv_eye_frame_bytes() -> usize {
  EYE_FRAME_BYTES
}

#[no_mangle]
pub unsafe extern "C" fn o3yv_decoder_init(
  decoder: *mut c_void,
  decoder_len: usize,
  stream: *const u8,
  stream_len: usize,
) -> i32 {
  if decoder.is_null() || stream.is_null() {
    return O3YV_ERR_NULL;
  }
  if decoder_len < o3yv_decoder_size()
    || (decoder as usize) % o3yv_decoder_align() != 0
  {
    return O3YV_ERR_STORAGE;
  }

  let stream = unsafe { slice::from_raw_parts(stream, stream_len) };
  match StreamDecoder::new(stream) {
    Ok(decoder_value) => {
      let decoder_value: StreamDecoder<'static> =
        unsafe { core::mem::transmute(decoder_value) };
      unsafe {
        ptr::write(decoder.cast::<StreamDecoder<'static>>(), decoder_value);
      }
      O3YV_DONE
    }
    Err(err) => map_error(err),
  }
}

#[no_mangle]
pub unsafe extern "C" fn o3yv_decoder_reset(decoder: *mut c_void) -> i32 {
  let Some(decoder) = decoder_mut(decoder) else {
    return O3YV_ERR_NULL;
  };
  match decoder.reset() {
    Ok(()) => O3YV_DONE,
    Err(err) => map_error(err),
  }
}

#[no_mangle]
pub unsafe extern "C" fn o3yv_decoder_next_frame_yuv420p(
  decoder: *mut c_void,
  left_yuv420p: *mut u8,
  left_len: usize,
  right_yuv420p: *mut u8,
  right_len: usize,
  info: *mut O3yvFrameInfo,
) -> i32 {
  let Some(decoder) = decoder_mut(decoder) else {
    return O3YV_ERR_NULL;
  };
  if left_yuv420p.is_null() || right_yuv420p.is_null() || info.is_null() {
    return O3YV_ERR_NULL;
  }
  if left_len != EYE_FRAME_BYTES || right_len != EYE_FRAME_BYTES {
    return O3YV_ERR_STORAGE;
  }

  let left = unsafe { slice::from_raw_parts_mut(left_yuv420p, left_len) };
  let right = unsafe { slice::from_raw_parts_mut(right_yuv420p, right_len) };
  match decoder.next_frame() {
    Ok(Some(decoded)) => {
      if let Err(err) = decoded.frame.left.write_yuv420p_into(left) {
        return map_error(err);
      }
      if let Err(err) = decoded.frame.right.write_yuv420p_into(right) {
        return map_error(err);
      }
      unsafe {
        ptr::write(
          info,
          O3yvFrameInfo {
            frame_no: decoded.frame_no,
            frame_type: decoded.frame_type,
            reserved: [0; 3],
          },
        );
      }
      O3YV_FRAME
    }
    Ok(None) => O3YV_DONE,
    Err(err) => map_error(err),
  }
}

#[no_mangle]
pub unsafe extern "C" fn o3yv_decoder_drop(decoder: *mut c_void) {
  if decoder.is_null() {
    return;
  }
  unsafe {
    ptr::drop_in_place(decoder.cast::<StreamDecoder<'static>>());
  }
}

fn decoder_mut<'a>(
  decoder: *mut c_void,
) -> Option<&'a mut StreamDecoder<'static>> {
  if decoder.is_null() {
    None
  } else {
    Some(unsafe { &mut *decoder.cast::<StreamDecoder<'static>>() })
  }
}

fn map_error(err: Error) -> i32 {
  match err {
    Error::Eof => O3YV_ERR_EOF,
    Error::Invalid(_) => O3YV_ERR_INVALID,
    Error::Unsupported(_) => O3YV_ERR_UNSUPPORTED,
  }
}
