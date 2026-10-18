//! node_zlib.cc `ZlibContext`: one owned zlib stream driven through the zlib C API, from
//! the linked Node zlib when the host provides it and from libz-rs otherwise.
use super::{Failure, Step};
use crate::modules::zlib_constants::Mode;
use std::{
    ffi::{c_int, c_uint, CStr},
    ptr,
};

#[cfg(quench_node_zlib)]
mod sys {
    use std::ffi::{c_char, c_int, c_uint};
    pub(super) use libz_rs_sys::z_stream;
    // libz-rs-sys only names the C `z_stream` layout here; the functions are Node's zlib.
    #[link(name = "quench_node_zlib", kind = "static")]
    extern "C" {
        pub(super) fn zlibVersion() -> *const c_char;
        pub(super) fn deflateInit2_(
            s: *mut z_stream,
            level: c_int,
            method: c_int,
            window: c_int,
            memory: c_int,
            strategy: c_int,
            version: *const c_char,
            size: c_int,
        ) -> c_int;
        pub(super) fn inflateInit2_(s: *mut z_stream, window: c_int, version: *const c_char, size: c_int) -> c_int;
        pub(super) fn deflate(s: *mut z_stream, flush: c_int) -> c_int;
        pub(super) fn inflate(s: *mut z_stream, flush: c_int) -> c_int;
        pub(super) fn deflateEnd(s: *mut z_stream) -> c_int;
        pub(super) fn inflateEnd(s: *mut z_stream) -> c_int;
        pub(super) fn deflateReset(s: *mut z_stream) -> c_int;
        pub(super) fn inflateReset(s: *mut z_stream) -> c_int;
        pub(super) fn deflateParams(s: *mut z_stream, level: c_int, strategy: c_int) -> c_int;
        pub(super) fn deflateSetDictionary(s: *mut z_stream, dict: *const u8, length: c_uint) -> c_int;
        pub(super) fn inflateSetDictionary(s: *mut z_stream, dict: *const u8, length: c_uint) -> c_int;
    }
}
#[cfg(not(quench_node_zlib))]
mod sys {
    pub(super) use libz_rs_sys::{
        deflate, deflateEnd, deflateInit2_, deflateParams, deflateReset, deflateSetDictionary, inflate,
        inflateEnd, inflateInit2_, inflateReset, inflateSetDictionary, z_stream, zlibVersion,
    };
}
use sys::*;

const Z_OK: c_int = 0;
const Z_STREAM_END: c_int = 1;
const Z_NEED_DICT: c_int = 2;
const Z_ERRNO: c_int = -1;
const Z_STREAM_ERROR: c_int = -2;
const Z_DATA_ERROR: c_int = -3;
const Z_MEM_ERROR: c_int = -4;
const Z_BUF_ERROR: c_int = -5;
const Z_VERSION_ERROR: c_int = -6;
const Z_FINISH: c_int = 4;
const Z_DEFLATED: c_int = 8;
/// windowBits offsets selecting the gzip wrapper and gzip/zlib detection.
const GZIP_WINDOW_OFFSET: i32 = 16;
const AUTO_WINDOW_OFFSET: i32 = 32;
const GZIP_HEADER: [u8; 2] = [0x1f, 0x8b];
/// Padding after a gzip member that never starts another member.
const GZIP_PADDING: u8 = 0;

/// zlib.h's `ZEXTERN const char *zError` names, as node_zlib.cc's `ZlibStrerror`.
fn code(status: c_int) -> &'static str {
    match status {
        Z_OK => "Z_OK",
        Z_STREAM_END => "Z_STREAM_END",
        Z_NEED_DICT => "Z_NEED_DICT",
        Z_ERRNO => "Z_ERRNO",
        Z_STREAM_ERROR => "Z_STREAM_ERROR",
        Z_DATA_ERROR => "Z_DATA_ERROR",
        Z_MEM_ERROR => "Z_MEM_ERROR",
        Z_BUF_ERROR => "Z_BUF_ERROR",
        Z_VERSION_ERROR => "Z_VERSION_ERROR",
        _ => "Z_UNKNOWN_ERROR",
    }
}

pub(crate) fn version() -> String {
    unsafe { CStr::from_ptr(zlibVersion()).to_string_lossy().into_owned() }
}

pub(crate) struct Codec {
    stream: Box<z_stream>,
    /// The current mode: `Unzip` becomes `Inflate` or `Gunzip` once the header is read.
    mode: Mode,
    encoding: bool,
    dictionary: Vec<u8>,
    reject_garbage_after_end: bool,
    gzip_id_bytes_read: usize,
    err: c_int,
    flush: c_int,
    /// Whether a gzip member is open after emitting bytes a reset cannot take back.
    stream_complete: bool,
    output_emitted: bool,
}
// The boxed stream and its allocations have one owner. Writes finish and clear all
// borrowed buffer pointers before this owner moves between threads.
unsafe impl Send for Codec {}
impl Drop for Codec {
    fn drop(&mut self) {
        unsafe {
            if self.encoding {
                deflateEnd(&mut *self.stream);
            } else {
                inflateEnd(&mut *self.stream);
            }
        }
    }
}

impl Codec {
    pub(in crate::modules) fn new(
        mode: Mode,
        bits: i32,
        level: i32,
        memory: i32,
        strategy: i32,
        dictionary: Option<Vec<u8>>,
        reject_garbage_after_end: bool,
    ) -> Result<Self, Failure> {
        let encoding = matches!(mode, Mode::Deflate | Mode::DeflateRaw | Mode::Gzip);
        let window = match mode {
            Mode::Gzip | Mode::Gunzip => bits + GZIP_WINDOW_OFFSET,
            Mode::Unzip => bits + AUTO_WINDOW_OFFSET,
            Mode::DeflateRaw | Mode::InflateRaw => -bits,
            _ => bits,
        };
        // z_stream's null allocator pair selects the library allocator. The box is
        // stable because zlib keeps a pointer back to this exact stream.
        let mut codec = Self {
            stream: Box::new(unsafe { std::mem::zeroed::<z_stream>() }),
            mode,
            encoding,
            dictionary: dictionary.unwrap_or_default(),
            reject_garbage_after_end,
            gzip_id_bytes_read: 0,
            err: Z_OK,
            flush: 0,
            stream_complete: true,
            output_emitted: false,
        };
        let size = std::mem::size_of::<z_stream>() as c_int;
        codec.err = unsafe {
            if encoding {
                deflateInit2_(&mut *codec.stream, level, Z_DEFLATED, window, memory, strategy, zlibVersion(), size)
            } else {
                inflateInit2_(&mut *codec.stream, window, zlibVersion(), size)
            }
        };
        if codec.err != Z_OK {
            return Err(codec.error("Init error"));
        }
        codec.set_dictionary()?;
        Ok(codec)
    }

    /// `ErrorForMessage`: zlib's own message wins over the caller's.
    fn error(&self, message: &str) -> Failure {
        let message = if self.stream.msg.is_null() {
            message.into()
        } else {
            unsafe { CStr::from_ptr(self.stream.msg) }.to_string_lossy().into_owned()
        };
        Failure {
            errno: self.err,
            code: code(self.err).into(),
            message,
        }
    }

    fn set_dictionary(&mut self) -> Result<(), Failure> {
        if self.dictionary.is_empty() {
            return Ok(());
        }
        let (data, length) = (self.dictionary.as_ptr(), self.dictionary.len() as c_uint);
        self.err = match self.mode {
            Mode::Deflate | Mode::DeflateRaw => unsafe { deflateSetDictionary(&mut *self.stream, data, length) },
            // Other inflate modes load the dictionary when inflate asks for it.
            Mode::InflateRaw => unsafe { inflateSetDictionary(&mut *self.stream, data, length) },
            _ => Z_OK,
        };
        if self.err != Z_OK {
            return Err(self.error("Failed to set dictionary"));
        }
        Ok(())
    }

    /// `ResetStream`.
    pub(crate) fn reset(&mut self) -> Result<(), Failure> {
        if matches!(self.mode, Mode::Gzip) && !self.stream_complete && self.output_emitted {
            return Err(Failure {
                errno: Z_STREAM_ERROR,
                code: "ERR_ZLIB_INCOMPLETE_FRAME".into(),
                message: "Cannot reset a gzip stream with an incomplete member; end the stream or start a new gzip compressor".into(),
            });
        }
        self.err = match self.mode {
            Mode::Deflate | Mode::DeflateRaw | Mode::Gzip => unsafe { deflateReset(&mut *self.stream) },
            Mode::Inflate | Mode::InflateRaw | Mode::Gunzip => unsafe { inflateReset(&mut *self.stream) },
            _ => Z_OK,
        };
        if self.err != Z_OK {
            return Err(self.error("Failed to reset stream"));
        }
        self.stream_complete = true;
        self.output_emitted = false;
        self.set_dictionary()
    }

    /// `SetParams`: only the deflate streams change parameters mid-stream.
    pub(crate) fn params(&mut self, level: i32, strategy: i32) -> Result<(), Failure> {
        self.err = match self.mode {
            Mode::Deflate | Mode::DeflateRaw => {
                // deflateParams first runs deflate(Z_BLOCK), which rejects a null output
                // buffer. Node's stream still names its last output buffer; after the
                // flush lib/zlib.js waits for, nothing is pending, so an empty one serves.
                let mut empty = 0u8;
                self.stream.next_out = &mut empty;
                self.stream.avail_out = 0;
                let status = unsafe { deflateParams(&mut *self.stream, level, strategy) };
                self.stream.next_out = ptr::null_mut();
                status
            }
            _ => Z_OK,
        };
        if self.err != Z_OK && self.err != Z_BUF_ERROR {
            return Err(self.error("Failed to set parameters"));
        }
        Ok(())
    }

    /// `DoThreadPoolWork` then `GetErrorInfo`.
    pub(crate) fn write(&mut self, flush: i32, input: &[u8], output: &mut [u8]) -> Result<Step, Failure> {
        if input.len() > c_uint::MAX as usize || output.len() > c_uint::MAX as usize {
            self.err = Z_STREAM_ERROR;
            return Err(self.error("Compression buffer exceeds native range"));
        }
        self.flush = flush;
        self.stream.next_in = input.as_ptr().cast_mut();
        self.stream.avail_in = input.len() as c_uint;
        self.stream.next_out = output.as_mut_ptr();
        self.stream.avail_out = output.len() as c_uint;
        self.process(input);
        let outcome = self.error_info().map(|()| Step {
            consumed: input.len() - self.stream.avail_in as usize,
            produced: output.len() - self.stream.avail_out as usize,
        });
        // No borrowed Rust buffer stays in native state across callbacks or Drop.
        self.stream.next_in = ptr::null_mut();
        self.stream.next_out = ptr::null_mut();
        self.stream.avail_in = 0;
        self.stream.avail_out = 0;
        outcome
    }

    fn consumed(&self, input: &[u8]) -> usize {
        input.len() - self.stream.avail_in as usize
    }

    fn process(&mut self, input: &[u8]) {
        if self.encoding {
            let before = self.stream.avail_out;
            self.err = unsafe { deflate(&mut *self.stream, self.flush) };
            self.output_emitted |= before > self.stream.avail_out;
            if self.err == Z_STREAM_END {
                self.stream_complete = true;
                self.output_emitted = false;
            } else if self.err == Z_OK || self.err == Z_BUF_ERROR {
                self.stream_complete = false;
            }
            return;
        }
        if matches!(self.mode, Mode::Unzip) {
            // Detect the gzip magic across writes; anything else is a zlib stream.
            let mut next = self.consumed(input);
            while self.gzip_id_bytes_read < GZIP_HEADER.len() && next < input.len() {
                if input[next] != GZIP_HEADER[self.gzip_id_bytes_read] {
                    self.mode = Mode::Inflate;
                    break;
                }
                self.gzip_id_bytes_read += 1;
                next += 1;
            }
            if self.gzip_id_bytes_read == GZIP_HEADER.len() {
                self.mode = Mode::Gunzip;
            }
        }
        self.err = unsafe { inflate(&mut *self.stream, self.flush) };
        if !matches!(self.mode, Mode::InflateRaw) && self.err == Z_NEED_DICT && !self.dictionary.is_empty() {
            self.err = unsafe {
                inflateSetDictionary(&mut *self.stream, self.dictionary.as_ptr(), self.dictionary.len() as c_uint)
            };
            if self.err == Z_OK {
                self.err = unsafe { inflate(&mut *self.stream, self.flush) };
            } else if self.err == Z_DATA_ERROR {
                // Tell a bad dictionary from bad input.
                self.err = Z_NEED_DICT;
            }
        }
        // Remaining bytes may start another member; trailing zero padding never does.
        while self.stream.avail_in > 0
            && matches!(self.mode, Mode::Gunzip)
            && self.err == Z_STREAM_END
            && !self.reject_garbage_after_end
            && input[self.consumed(input)] != GZIP_PADDING
        {
            let _ = self.reset();
            self.err = unsafe { inflate(&mut *self.stream, self.flush) };
        }
    }

    fn error_info(&self) -> Result<(), Failure> {
        match self.err {
            Z_OK | Z_BUF_ERROR if self.stream.avail_out != 0 && self.flush == Z_FINISH => {
                Err(self.error("unexpected end of file"))
            }
            Z_OK | Z_BUF_ERROR | Z_STREAM_END => Ok(()),
            Z_NEED_DICT if self.dictionary.is_empty() => Err(self.error("Missing dictionary")),
            Z_NEED_DICT => Err(self.error("Bad dictionary")),
            _ => Err(self.error("Zlib error")),
        }
    }
}
