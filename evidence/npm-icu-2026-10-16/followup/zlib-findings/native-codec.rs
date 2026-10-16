//! Draft Rust ownership for the pinned Node zlib C codec; not production wiring.
use std::{
    ffi::{CStr, c_char, c_int, c_uint, c_ulong, c_void},
    io, ptr,
};
const DEFLATED: c_int = 8;
const DEFAULT_MEMORY_LEVEL: c_int = 8;
const DEFAULT_STRATEGY: c_int = 0;
const WINDOW_BITS: c_int = 15;
const GZIP_WRAPPER: c_int = 16;
const OUTPUT_CHUNK_BYTES: usize = 16 * 1024;
const Z_OK: c_int = 0;
const Z_STREAM_END: c_int = 1;
const Z_BUF_ERROR: c_int = -5;
#[repr(C)]
struct Stream {
    next_in: *mut u8,
    avail_in: c_uint,
    total_in: c_ulong,
    next_out: *mut u8,
    avail_out: c_uint,
    total_out: c_ulong,
    msg: *mut c_char,
    state: *mut c_void,
    zalloc: Option<unsafe extern "C" fn(*mut c_void, c_uint, c_uint) -> *mut c_void>,
    zfree: Option<unsafe extern "C" fn(*mut c_void, *mut c_void)>,
    opaque: *mut c_void,
    data_type: c_int,
    adler: c_ulong,
    reserved: c_ulong,
}
#[link(name = "nodezlib", kind = "static")]
unsafe extern "C" {
    fn z_zlibVersion() -> *const c_char;
    fn z_deflateInit2_(
        stream: *mut Stream,
        level: c_int,
        method: c_int,
        window: c_int,
        memory: c_int,
        strategy: c_int,
        version: *const c_char,
        size: c_int,
    ) -> c_int;
    fn z_deflate(stream: *mut Stream, flush: c_int) -> c_int;
    fn z_deflateEnd(stream: *mut Stream) -> c_int;
}
#[derive(Clone, Copy)]
pub enum Header {
    Zlib,
    Raw,
    Gzip,
}
#[derive(Clone, Copy)]
#[repr(i32)]
pub enum Flush {
    None = 0,
    Partial = 1,
    Sync = 2,
    Full = 3,
    Finish = 4,
    Block = 5,
}
#[derive(Debug)]
pub struct Error {
    pub status: i32,
    pub message: String,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for Error {}
/// A stable address is required: zlib's state keeps its owning z_stream pointer.
pub struct Compressor {
    stream: Box<Stream>,
}
impl Drop for Compressor {
    fn drop(&mut self) {
        // SAFETY: only a successful initialization constructs this owner.
        unsafe {
            z_deflateEnd(&mut *self.stream);
        }
    }
}
pub struct Output {
    pub bytes: Vec<u8>,
    pub consumed: usize,
    pub ended: bool,
}
impl Compressor {
    pub fn new(header: Header, level: i32) -> Result<Self, Error> {
        // SAFETY: zero/null is the documented z_stream allocator/input initial state.
        let mut stream = Box::new(unsafe { std::mem::zeroed::<Stream>() });
        let window = match header {
            Header::Zlib => WINDOW_BITS,
            Header::Raw => -WINDOW_BITS,
            Header::Gzip => WINDOW_BITS + GZIP_WRAPPER,
        };
        // SAFETY: the boxed stream stays at this address for its entire lifetime;
        // the version string is static and the explicit size checks the C layout.
        let status = unsafe {
            z_deflateInit2_(
                &mut *stream,
                level,
                DEFLATED,
                window,
                DEFAULT_MEMORY_LEVEL,
                DEFAULT_STRATEGY,
                z_zlibVersion(),
                std::mem::size_of::<Stream>() as c_int,
            )
        };
        if status != Z_OK {
            return Err(error(&stream, status));
        }
        Ok(Self { stream })
    }
    pub fn process(&mut self, input: &[u8], flush: Flush) -> Result<Output, Error> {
        let mut bytes = Vec::new();
        let mut consumed = 0;
        let mut status;
        loop {
            let remaining = input.len() - consumed;
            let input_length = remaining.min(c_uint::MAX as usize);
            let actual_flush = if remaining > input_length {
                Flush::None
            } else {
                flush
            };
            let mut output = [0u8; OUTPUT_CHUNK_BYTES];
            let stream = &mut *self.stream;
            // SAFETY: pointers are bounded by the exact supplied input/output lengths;
            // the call cannot retain borrowed input beyond this synchronous operation.
            stream.next_in = unsafe { input.as_ptr().add(consumed) }.cast_mut();
            stream.avail_in = input_length as c_uint;
            stream.next_out = output.as_mut_ptr();
            stream.avail_out = output.len() as c_uint;
            status = unsafe { z_deflate(stream, actual_flush as c_int) };
            let failure = (!matches!(status, Z_OK | Z_STREAM_END | Z_BUF_ERROR))
                .then(|| error(stream, status));
            let consumed_here = input_length - stream.avail_in as usize;
            let written = output.len() - stream.avail_out as usize;
            // Drop all borrowed pointers before returning, including error paths.
            stream.next_in = ptr::null_mut();
            stream.next_out = ptr::null_mut();
            if let Some(failure) = failure {
                return Err(failure);
            }
            consumed += consumed_here;
            bytes.extend_from_slice(&output[..written]);
            if written < output.len() && (consumed == input.len() || status == Z_STREAM_END) {
                break;
            }
            if written == 0 && consumed_here == 0 {
                return Err(error(stream, Z_BUF_ERROR));
            }
        }
        Ok(Output {
            bytes,
            consumed,
            ended: status == Z_STREAM_END,
        })
    }
}
fn error(stream: &Stream, status: c_int) -> Error {
    let message = if stream.msg.is_null() {
        format!("zlib status {status}")
    } else {
        // SAFETY: zlib owns its static NUL-terminated diagnostic while the stream lives.
        unsafe { CStr::from_ptr(stream.msg) }
            .to_string_lossy()
            .into_owned()
    };
    Error { status, message }
}
pub fn compress(header: Header, input: &[u8], level: i32) -> io::Result<Vec<u8>> {
    let mut codec = Compressor::new(header, level).map_err(io::Error::other)?;
    let output = codec
        .process(input, Flush::Finish)
        .map_err(io::Error::other)?;
    if !output.ended || output.consumed != input.len() {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "incomplete compressed output",
        ));
    }
    Ok(output.bytes)
}
