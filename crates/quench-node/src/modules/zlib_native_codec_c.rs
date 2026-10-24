//! Exclusive Rust ownership of the actual native zlib stream and its byte buffers.
use crate::modules::zlib_shared_vm::Codec as Mode;
use std::{
    ffi::{c_char, c_int, c_uint, c_ulong, c_void, CStr},
    ptr,
};
const Z_OK: c_int = 0;
const Z_STREAM_END: c_int = 1;
const Z_NEED_DICT: c_int = 2;
const Z_STREAM_ERROR: c_int = -2;
const Z_DATA_ERROR: c_int = -3;
const Z_MEM_ERROR: c_int = -4;
const Z_BUF_ERROR: c_int = -5;
const Z_VERSION_ERROR: c_int = -6;
const Z_FINISH: c_int = 4;
const Z_DEFLATED: c_int = 8;
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
#[link(name = "quench_node_zlib", kind = "static")]
extern "C" {
    fn zlibVersion() -> *const c_char;
    fn deflateInit2_(
        s: *mut Stream,
        level: c_int,
        method: c_int,
        window: c_int,
        memory: c_int,
        strategy: c_int,
        version: *const c_char,
        size: c_int,
    ) -> c_int;
    fn inflateInit2_(s: *mut Stream, window: c_int, version: *const c_char, size: c_int) -> c_int;
    fn deflate(s: *mut Stream, flush: c_int) -> c_int;
    fn deflateParams(s: *mut Stream, level: c_int, strategy: c_int) -> c_int;
    fn inflate(s: *mut Stream, flush: c_int) -> c_int;
    fn deflateEnd(s: *mut Stream) -> c_int;
    fn inflateEnd(s: *mut Stream) -> c_int;
    fn inflateReset(s: *mut Stream) -> c_int;
    fn deflateSetDictionary(s: *mut Stream, dict: *const u8, length: c_uint) -> c_int;
    fn inflateSetDictionary(s: *mut Stream, dict: *const u8, length: c_uint) -> c_int;
}
use super::{Failure, Step};
pub(crate) struct Codec {
    stream: Box<Stream>,
    mode: Mode,
    encoding: bool,
    dictionary: Option<Vec<u8>>,
    gzip_members: bool,
    reject_trailing: bool,
    auto_header: Option<Vec<u8>>,
}
// The boxed C stream and codec allocations have one owner. Writes finish and
// clear all borrowed buffer pointers before this owner moves between threads.
unsafe impl Send for Codec {}
impl Drop for Codec {
    fn drop(&mut self) {
        if !self.stream.state.is_null() {
            unsafe {
                if self.encoding {
                    deflateEnd(&mut *self.stream);
                } else {
                    inflateEnd(&mut *self.stream);
                }
            }
        }
    }
}
pub(crate) fn version() -> String {
    unsafe { CStr::from_ptr(zlibVersion()).to_string_lossy().into_owned() }
}
fn code(n: i32) -> &'static str {
    match n {
        Z_OK => "Z_OK",
        Z_STREAM_END => "Z_STREAM_END",
        Z_NEED_DICT => "Z_NEED_DICT",
        Z_STREAM_ERROR => "Z_STREAM_ERROR",
        Z_DATA_ERROR => "Z_DATA_ERROR",
        Z_MEM_ERROR => "Z_MEM_ERROR",
        Z_BUF_ERROR => "Z_BUF_ERROR",
        Z_VERSION_ERROR => "Z_VERSION_ERROR",
        _ => "Z_ERRNO",
    }
}
impl Codec {
    pub(crate) fn params(&mut self, level: i32, strategy: i32) -> Result<(), Failure> {
        if !self.encoding {
            return Err(Failure {
                errno: Z_STREAM_ERROR,
                code: code(Z_STREAM_ERROR).into(),
                message: "stream error".into(),
            });
        }
        let status = unsafe { deflateParams(&mut *self.stream, level, strategy) };
        if status == Z_OK {
            Ok(())
        } else {
            Err(Failure {
                errno: status,
                code: code(status).into(),
                message: if self.stream.msg.is_null() {
                    "stream error".into()
                } else {
                    unsafe { CStr::from_ptr(self.stream.msg) }
                        .to_string_lossy()
                        .into_owned()
                },
            })
        }
    }

    pub(in crate::modules) fn new(
        mode: Mode,
        bits: i32,
        level: i32,
        memory: i32,
        strategy: i32,
        dictionary: Option<Vec<u8>>,
        reject_trailing: bool,
    ) -> Result<Self, Failure> {
        let encoding = matches!(mode, Mode::Deflate | Mode::DeflateRaw | Mode::Gzip);
        let window = match mode {
            Mode::Gzip | Mode::Gunzip => bits + 16,
            Mode::Unzip => bits + 32,
            Mode::DeflateRaw | Mode::InflateRaw => -bits,
            _ => bits,
        };
        // z_stream's documented null allocator pair selects the library allocator.
        // The box is stable because zlib keeps a pointer back to this exact stream.
        let stream = Box::new(unsafe { std::mem::zeroed::<Stream>() });
        let mut result = Self {
            stream,
            mode,
            encoding,
            dictionary,
            gzip_members: matches!(mode, Mode::Gunzip) && !reject_trailing,
            reject_trailing,
            auto_header: matches!(mode, Mode::Unzip).then(Vec::new),
        };
        let status = unsafe {
            let p = &mut *result.stream;
            if encoding {
                deflateInit2_(
                    p,
                    level,
                    Z_DEFLATED,
                    window,
                    memory,
                    strategy,
                    zlibVersion(),
                    std::mem::size_of::<Stream>() as c_int,
                )
            } else {
                inflateInit2_(
                    p,
                    window,
                    zlibVersion(),
                    std::mem::size_of::<Stream>() as c_int,
                )
            }
        };
        if status != Z_OK {
            return Err(result.failure(status, "Failed to initialize compression stream"));
        }
        if let Some(dict) = result.dictionary.as_deref().filter(|v| !v.is_empty()) {
            let status = unsafe {
                if matches!(mode, Mode::Deflate | Mode::DeflateRaw) {
                    deflateSetDictionary(&mut *result.stream, dict.as_ptr(), dict.len() as c_uint)
                } else if matches!(mode, Mode::InflateRaw) {
                    inflateSetDictionary(&mut *result.stream, dict.as_ptr(), dict.len() as c_uint)
                } else {
                    Z_OK
                }
            };
            if status != Z_OK {
                return Err(result.failure(status, "Failed to set dictionary"));
            }
        }
        Ok(result)
    }
    fn failure(&self, status: i32, fallback: &str) -> Failure {
        let message = if self.stream.msg.is_null() {
            fallback.into()
        } else {
            unsafe {
                CStr::from_ptr(self.stream.msg)
                    .to_string_lossy()
                    .into_owned()
            }
        };
        Failure {
            errno: status,
            code: code(status).into(),
            message,
        }
    }
    pub(crate) fn write(
        &mut self,
        flush: i32,
        input: &[u8],
        output: &mut [u8],
    ) -> Result<Step, Failure> {
        if input.len() > c_uint::MAX as usize || output.len() > c_uint::MAX as usize {
            return Err(self.failure(Z_STREAM_ERROR, "Compression buffer exceeds native range"));
        }
        if let Some(header) = self.auto_header.as_mut() {
            for value in input.iter().take(2 - header.len()) {
                header.push(*value);
            }
            if header.len() == 2 {
                self.gzip_members = header == &[0x1f, 0x8b] && !self.reject_trailing;
                self.auto_header = None;
            }
        }
        self.stream.next_in = input.as_ptr().cast_mut();
        self.stream.avail_in = input.len() as c_uint;
        self.stream.next_out = output.as_mut_ptr();
        self.stream.avail_out = output.len() as c_uint;
        let outcome = (|| {
            let mut status = unsafe {
                if self.encoding {
                    deflate(&mut *self.stream, flush)
                } else {
                    inflate(&mut *self.stream, flush)
                }
            };
            if status == Z_NEED_DICT {
                if let Some(dict) = self.dictionary.as_deref().filter(|v| !v.is_empty()) {
                    status = unsafe {
                        inflateSetDictionary(&mut *self.stream, dict.as_ptr(), dict.len() as c_uint)
                    };
                    if status == Z_OK {
                        status = unsafe { inflate(&mut *self.stream, flush) }
                    } else if status == Z_DATA_ERROR {
                        status = Z_NEED_DICT;
                    }
                }
            }
            while status == Z_STREAM_END
                && self.gzip_members
                && self.stream.avail_in > 0
                && self.stream.avail_out > 0
            {
                if unsafe { *self.stream.next_in } == 0 {
                    self.stream.avail_in = 0;
                    break;
                }
                status = unsafe { inflateReset(&mut *self.stream) };
                if status != Z_OK {
                    break;
                }
                status = unsafe { inflate(&mut *self.stream, flush) };
            }
            if status == Z_STREAM_END
                && self.reject_trailing
                && matches!(self.mode, Mode::Gunzip | Mode::Unzip)
                && self.stream.avail_in > 0
            {
                return Err(Failure {
                    errno: -1,
                    code: "ERR_TRAILING_JUNK_AFTER_STREAM_END".into(),
                    message: "Trailing junk after stream end".into(),
                });
            }
            match status {
                Z_STREAM_END => {}
                Z_OK | Z_BUF_ERROR => {
                    if self.stream.avail_out != 0 && flush == Z_FINISH {
                        return Err(self.failure(status, "unexpected end of file"));
                    }
                }
                Z_NEED_DICT => {
                    return Err(self.failure(
                        status,
                        if self.dictionary.as_ref().is_some_and(|v| !v.is_empty()) {
                            "Bad dictionary"
                        } else {
                            "Missing dictionary"
                        },
                    ));
                }
                _ => return Err(self.failure(status, "Zlib error")),
            }
            Ok(Step {
                consumed: input.len() - self.stream.avail_in as usize,
                produced: output.len() - self.stream.avail_out as usize,
            })
        })();
        // No borrowed Rust buffer is kept in native state across callbacks or Drop.
        self.stream.next_in = ptr::null_mut();
        self.stream.next_out = ptr::null_mut();
        self.stream.avail_in = 0;
        self.stream.avail_out = 0;
        outcome
    }
}
