//! Rust ownership of the matching Brotli encoder/decoder and dictionary lifetimes.
use super::{Failure, Step};
use crate::modules::zlib_shared_vm::Codec as Mode;
use std::{
    ffi::{c_char, c_int, c_void, CStr},
    ptr::{self, NonNull},
};
type Alloc = Option<unsafe extern "C" fn(*mut c_void, usize) -> *mut c_void>;
type Free = Option<unsafe extern "C" fn(*mut c_void, *mut c_void)>;
const FINISH: i32 = 2;
const DECODE_ERROR: c_int = 0;
const DECODE_NEEDS_MORE_INPUT: c_int = 2;
const PARAM_UNSET: u32 = u32::MAX;
const RAW_DICTIONARY: c_int = 0;
const MAX_QUALITY: c_int = 11;
#[link(name = "quench_node_brotli", kind = "static")]
extern "C" {
    fn BrotliEncoderCreateInstance(a: Alloc, f: Free, o: *mut c_void) -> *mut c_void;
    fn BrotliDecoderCreateInstance(a: Alloc, f: Free, o: *mut c_void) -> *mut c_void;
    fn BrotliEncoderDestroyInstance(s: *mut c_void);
    fn BrotliDecoderDestroyInstance(s: *mut c_void);
    fn BrotliEncoderSetParameter(s: *mut c_void, p: c_int, v: u32) -> c_int;
    fn BrotliDecoderSetParameter(s: *mut c_void, p: c_int, v: u32) -> c_int;
    fn BrotliEncoderCompressStream(
        s: *mut c_void,
        op: c_int,
        ain: *mut usize,
        input: *mut *const u8,
        aout: *mut usize,
        out: *mut *mut u8,
        total: *mut usize,
    ) -> c_int;
    fn BrotliDecoderDecompressStream(
        s: *mut c_void,
        ain: *mut usize,
        input: *mut *const u8,
        aout: *mut usize,
        out: *mut *mut u8,
        total: *mut usize,
    ) -> c_int;
    fn BrotliDecoderGetErrorCode(s: *const c_void) -> c_int;
    fn BrotliDecoderErrorString(code: c_int) -> *const c_char;
    fn BrotliEncoderPrepareDictionary(
        kind: c_int,
        size: usize,
        data: *const u8,
        quality: c_int,
        a: Alloc,
        f: Free,
        o: *mut c_void,
    ) -> *mut c_void;
    fn BrotliEncoderDestroyPreparedDictionary(p: *mut c_void);
    fn BrotliEncoderAttachPreparedDictionary(s: *mut c_void, p: *const c_void) -> c_int;
    fn BrotliDecoderAttachDictionary(
        s: *mut c_void,
        kind: c_int,
        size: usize,
        data: *const u8,
    ) -> c_int;
}
pub(crate) struct Codec {
    state: NonNull<c_void>,
    encoding: bool,
    prepared: Option<NonNull<c_void>>,
    _dictionary: Vec<u8>,
}
// State and dictionary allocations have one exclusive owner. The C streaming
// calls borrow input/output only while executing; no guest values cross threads.
unsafe impl Send for Codec {}
impl Drop for Codec {
    fn drop(&mut self) {
        unsafe {
            if self.encoding {
                BrotliEncoderDestroyInstance(self.state.as_ptr())
            } else {
                BrotliDecoderDestroyInstance(self.state.as_ptr())
            }
            if let Some(p) = self.prepared {
                BrotliEncoderDestroyPreparedDictionary(p.as_ptr())
            }
        }
    }
}
fn failure(message: &str, code: &str) -> Failure {
    Failure {
        errno: -1,
        code: code.into(),
        message: message.into(),
    }
}
impl Codec {
    pub(in crate::modules) fn new(
        mode: Mode,
        params: &[u32],
        dictionary: Option<Vec<u8>>,
    ) -> Result<Self, Failure> {
        let encoding = matches!(mode, Mode::BrotliCompress);
        // Null callbacks select the library allocator, released by the matching destroy.
        let state = NonNull::new(unsafe {
            if encoding {
                BrotliEncoderCreateInstance(None, None, ptr::null_mut())
            } else {
                BrotliDecoderCreateInstance(None, None, ptr::null_mut())
            }
        })
        .ok_or_else(|| {
            failure(
                "Could not initialize Brotli instance",
                "ERR_ZLIB_INITIALIZATION_FAILED",
            )
        })?;
        let mut codec = Self {
            state,
            encoding,
            prepared: None,
            _dictionary: dictionary.unwrap_or_default(),
        };
        if !codec._dictionary.is_empty() {
            if encoding {
                codec.prepared = NonNull::new(unsafe {
                    BrotliEncoderPrepareDictionary(
                        RAW_DICTIONARY,
                        codec._dictionary.len(),
                        codec._dictionary.as_ptr(),
                        MAX_QUALITY,
                        None,
                        None,
                        ptr::null_mut(),
                    )
                });
                let prepared = codec.prepared.ok_or_else(|| {
                    failure(
                        "Failed to prepare brotli dictionary",
                        "ERR_ZLIB_DICTIONARY_LOAD_FAILED",
                    )
                })?;
                if unsafe {
                    BrotliEncoderAttachPreparedDictionary(state.as_ptr(), prepared.as_ptr())
                } == 0
                {
                    return Err(failure(
                        "Failed to attach brotli dictionary",
                        "ERR_ZLIB_DICTIONARY_LOAD_FAILED",
                    ));
                }
            } else if unsafe {
                BrotliDecoderAttachDictionary(
                    state.as_ptr(),
                    RAW_DICTIONARY,
                    codec._dictionary.len(),
                    codec._dictionary.as_ptr(),
                )
            } == 0
            {
                return Err(failure(
                    "Failed to attach brotli dictionary",
                    "ERR_ZLIB_DICTIONARY_LOAD_FAILED",
                ));
            }
        }
        for (p, value) in params
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, v)| *v != PARAM_UNSET)
        {
            if unsafe {
                if encoding {
                    BrotliEncoderSetParameter(state.as_ptr(), p as c_int, value)
                } else {
                    BrotliDecoderSetParameter(state.as_ptr(), p as c_int, value)
                }
            } == 0
            {
                return Err(failure(
                    "Setting parameter failed",
                    "ERR_BROTLI_PARAM_SET_FAILED",
                ));
            }
        }
        Ok(codec)
    }
    pub(crate) fn write(
        &mut self,
        flush: i32,
        input: &[u8],
        output: &mut [u8],
    ) -> Result<Step, Failure> {
        let (mut available_in, mut available_out) = (input.len(), output.len());
        let mut next_in = input.as_ptr();
        let mut next_out = output.as_mut_ptr();
        let result = unsafe {
            if self.encoding {
                BrotliEncoderCompressStream(
                    self.state.as_ptr(),
                    flush,
                    &mut available_in,
                    &mut next_in,
                    &mut available_out,
                    &mut next_out,
                    ptr::null_mut(),
                )
            } else {
                BrotliDecoderDecompressStream(
                    self.state.as_ptr(),
                    &mut available_in,
                    &mut next_in,
                    &mut available_out,
                    &mut next_out,
                    ptr::null_mut(),
                )
            }
        };
        if self.encoding && result == 0 {
            return Err(failure(
                "Compression failed",
                "ERR_BROTLI_COMPRESSION_FAILED",
            ));
        }
        if !self.encoding {
            if result == DECODE_ERROR {
                let error = unsafe { BrotliDecoderGetErrorCode(self.state.as_ptr()) };
                let name =
                    unsafe { CStr::from_ptr(BrotliDecoderErrorString(error)) }.to_string_lossy();
                // Node exposes this malformed block stream as
                // FORMAT_BLOCK_SWITCH (-17); the bundled decoder reports
                // the equivalent older FORMAT_BLOCK_LENGTH_1 enum value.
                let (error, name) = if name.ends_with("FORMAT_BLOCK_LENGTH_1") {
                    let prefix = &name[..name.len() - "FORMAT_BLOCK_LENGTH_1".len()];
                    (-17, format!("{prefix}FORMAT_BLOCK_SWITCH").into())
                } else {
                    (error, name)
                };
                return Err(Failure {
                    errno: error,
                    code: format!("ERR_{name}"),
                    message: "Decompression failed".into(),
                });
            }
            if flush == FINISH && result == DECODE_NEEDS_MORE_INPUT {
                return Err(Failure {
                    errno: -5,
                    code: "Z_BUF_ERROR".into(),
                    message: "unexpected end of file".into(),
                });
            }
        }
        Ok(Step {
            consumed: input.len() - available_in,
            produced: output.len() - available_out,
        })
    }
}
