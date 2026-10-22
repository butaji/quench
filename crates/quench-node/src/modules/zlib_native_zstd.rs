//! Rust ownership of the Zstd streaming ABI already linked by the Node host.
use super::{Failure, Step};
use crate::modules::zlib_constants::Mode;
use std::{
    ffi::{c_int, c_void},
    ptr::NonNull,
};
use zstd::zstd_safe::{self, zstd_sys as sys};
const PARAM_UNSET: u32 = u32::MAX;
const END: c_int = 2;
const SKIPPABLE_TAG_MASK: u8 = 0xf0;
extern "C" {
    // Accept the native integer domain, including unsupported parameters that
    // must return a library error rather than form an invalid Rust enum.
    fn ZSTD_CCtx_setParameter(s: *mut sys::ZSTD_CCtx, p: c_int, v: c_int) -> usize;
    fn ZSTD_DCtx_setParameter(s: *mut sys::ZSTD_DCtx, p: c_int, v: c_int) -> usize;
    fn ZSTD_compressStream2(
        s: *mut sys::ZSTD_CCtx,
        o: *mut sys::ZSTD_outBuffer,
        i: *mut sys::ZSTD_inBuffer,
        end: c_int,
    ) -> usize;
}
enum State {
    Compress(NonNull<sys::ZSTD_CCtx>),
    Decompress(NonNull<sys::ZSTD_DCtx>),
}
pub(crate) struct Codec {
    state: State,
    /// The configured pledge, restored by reset; `pledge` is the current frame's.
    pledged: Option<u64>,
    pledge: Option<u64>,
    consumed: u64,
    frame_complete: bool,
    /// A compressor's current frame has written bytes that a reset cannot take back.
    frame_output_emitted: bool,
    after_frame_prefix: Option<Prefix>,
    ignoring_trailing: bool,
    reject_trailing: bool,
}
struct Prefix {
    size: usize,
    standard: bool,
    skippable: bool,
}
const FRAME_MAGIC: [u8; 4] = sys::ZSTD_MAGICNUMBER.to_le_bytes();
const SKIPPABLE_MAGIC: [u8; 4] = sys::ZSTD_MAGIC_SKIPPABLE_START.to_le_bytes();
// One owner accesses the codec, and borrowed buffers exist only during write.
unsafe impl Send for Codec {}
impl Drop for Codec {
    fn drop(&mut self) {
        unsafe {
            match self.state {
                State::Compress(p) => {
                    sys::ZSTD_freeCCtx(p.as_ptr());
                }
                State::Decompress(p) => {
                    sys::ZSTD_freeDCtx(p.as_ptr());
                }
            }
        }
    }
}
fn initialization(message: &str, code: &str) -> Failure {
    Failure {
        errno: -1,
        code: code.into(),
        message: message.into(),
    }
}
fn error(result: usize) -> Option<Failure> {
    if unsafe { sys::ZSTD_isError(result) } == 0 {
        None
    } else {
        let code = unsafe { sys::ZSTD_getErrorCode(result) };
        Some(Failure {
            errno: code as i32,
            code: format!("{code:?}"),
            message: zstd_safe::get_error_name(result).into(),
        })
    }
}
impl Codec {
    pub(in crate::modules) fn new(
        mode: Mode,
        params: &[u32],
        pledge: Option<u64>,
        dictionary: Option<Vec<u8>>,
        reject_trailing: bool,
    ) -> Result<Self, Failure> {
        // ZSTD_CONTENTSIZE_UNKNOWN is the unsigned representation of -1.
        let pledge = pledge.filter(|size| *size != u64::MAX);
        let state = unsafe {
            if matches!(mode, Mode::ZstdCompress) {
                NonNull::new(sys::ZSTD_createCCtx()).map(State::Compress)
            } else {
                NonNull::new(sys::ZSTD_createDCtx()).map(State::Decompress)
            }
        }
        .ok_or_else(|| {
            initialization(
                "Could not initialize zstd instance",
                "ERR_ZLIB_INITIALIZATION_FAILED",
            )
        })?;
        let codec = Self {
            frame_complete: matches!(state, State::Compress(_)),
            state,
            pledged: pledge,
            pledge,
            consumed: 0,
            frame_output_emitted: false,
            after_frame_prefix: None,
            ignoring_trailing: false,
            reject_trailing,
        };
        if let Some(dictionary) = dictionary.filter(|v| !v.is_empty()) {
            let result = unsafe {
                match codec.state {
                    State::Compress(p) => sys::ZSTD_CCtx_loadDictionary(
                        p.as_ptr(),
                        dictionary.as_ptr().cast::<c_void>(),
                        dictionary.len(),
                    ),
                    State::Decompress(p) => sys::ZSTD_DCtx_loadDictionary(
                        p.as_ptr(),
                        dictionary.as_ptr().cast::<c_void>(),
                        dictionary.len(),
                    ),
                }
            };
            if error(result).is_some() {
                return Err(initialization(
                    "Failed to load zstd dictionary",
                    "ERR_ZLIB_DICTIONARY_LOAD_FAILED",
                ));
            }
        }
        if let State::Compress(p) = codec.state {
            if error(unsafe {
                sys::ZSTD_CCtx_setPledgedSrcSize(p.as_ptr(), pledge.unwrap_or(u64::MAX))
            })
            .is_some()
            {
                return Err(initialization(
                    "Could not set pledged src size",
                    "ERR_ZLIB_INITIALIZATION_FAILED",
                ));
            }
        }
        for (p, v) in params
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, v)| *v != PARAM_UNSET)
        {
            let result = unsafe {
                match codec.state {
                    State::Compress(s) => {
                        ZSTD_CCtx_setParameter(s.as_ptr(), p as c_int, v as c_int)
                    }
                    State::Decompress(s) => {
                        ZSTD_DCtx_setParameter(s.as_ptr(), p as c_int, v as c_int)
                    }
                }
            };
            if error(result).is_some() {
                return Err(initialization(
                    "Setting parameter failed",
                    "ERR_ZSTD_PARAM_SET_FAILED",
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
        let mut i = sys::ZSTD_inBuffer {
            src: input.as_ptr().cast(),
            size: input.len(),
            pos: 0,
        };
        let mut o = sys::ZSTD_outBuffer {
            dst: output.as_mut_ptr().cast(),
            size: output.len(),
            pos: 0,
        };
        if matches!(self.state, State::Decompress(_)) && self.frame_complete && input.is_empty() {
            return Ok(Step {
                consumed: 0,
                produced: 0,
            });
        }
        if self.ignoring_trailing {
            return Ok(Step {
                consumed: 0,
                produced: 0,
            });
        }
        let result = match self.state {
            State::Compress(p) => unsafe {
                ZSTD_compressStream2(p.as_ptr(), &mut o, &mut i, flush)
            },
            State::Decompress(p) => loop {
                if self.frame_complete {
                    self.after_frame_prefix = Some(Prefix {
                        size: 0,
                        standard: true,
                        skippable: true,
                    });
                }
                if let Some(prefix) = self.after_frame_prefix.as_mut() {
                    let mut offset = 0;
                    while prefix.size < FRAME_MAGIC.len() && i.pos + offset < i.size {
                        let index = prefix.size;
                        let byte = input[i.pos + offset];
                        prefix.standard &= byte == FRAME_MAGIC[index];
                        prefix.skippable &= if index == 0 {
                            byte & SKIPPABLE_TAG_MASK == SKIPPABLE_MAGIC[index]
                        } else {
                            byte == SKIPPABLE_MAGIC[index]
                        };
                        prefix.size += 1;
                        offset += 1;
                    }
                    if !prefix.standard && !prefix.skippable {
                        self.frame_complete = true;
                        self.after_frame_prefix = None;
                        if self.reject_trailing {
                            return Err(Failure {
                                errno: sys::ZSTD_ErrorCode::ZSTD_error_GENERIC as i32,
                                code: "ERR_TRAILING_JUNK_AFTER_STREAM_END".into(),
                                message:
                                    "Trailing junk found after the end of the compressed stream"
                                        .into(),
                            });
                        }
                        self.ignoring_trailing = true;
                        return Ok(Step {
                            consumed: i.pos,
                            produced: o.pos,
                        });
                    }
                }
                let result = unsafe { sys::ZSTD_decompressStream(p.as_ptr(), &mut o, &mut i) };
                if let Some(e) = error(result) {
                    self.frame_complete = false;
                    return Err(e);
                }
                self.frame_complete = result == 0;
                if self.frame_complete {
                    self.after_frame_prefix = None;
                }
                if !self.frame_complete || i.pos == i.size || o.pos == o.size {
                    break result;
                }
            },
        };
        if matches!(self.state, State::Compress(_)) {
            self.frame_output_emitted |= o.pos > 0;
            self.frame_complete = error(result).is_none() && result == 0 && flush == END;
            if self.frame_complete {
                self.frame_output_emitted = false;
            }
        }
        if let Some(e) = error(result) {
            return Err(e);
        }
        if matches!(self.state, State::Compress(_)) {
            self.consumed += i.pos as u64;
            if result == 0 && flush == END {
                if let Some(pledge) = self.pledge.take() {
                    if pledge != self.consumed {
                        return Err(Failure {
                            errno: sys::ZSTD_ErrorCode::ZSTD_error_srcSize_wrong as i32,
                            code: "ZSTD_error_srcSize_wrong".into(),
                            message: "Src size is incorrect".into(),
                        });
                    }
                }
            }
        } else if flush == END && !self.frame_complete && i.pos == i.size && o.pos < o.size {
            if self
                .after_frame_prefix
                .as_ref()
                .is_some_and(|p| p.size < FRAME_MAGIC.len())
            {
                if self.reject_trailing {
                    return Err(initialization(
                        "Trailing junk found after the end of the compressed stream",
                        "ERR_TRAILING_JUNK_AFTER_STREAM_END",
                    ));
                }
            } else {
                return Err(Failure {
                    errno: -5,
                    code: "Z_BUF_ERROR".into(),
                    message: "unexpected end of file".into(),
                });
            }
        }
        Ok(Step {
            consumed: i.pos,
            produced: o.pos,
        })
    }

    /// node_zlib.cc `ZstdCompressContext::ResetStream`/`ZstdDecompressContext::ResetStream`.
    pub(crate) fn reset(&mut self) -> Result<(), Failure> {
        let result = unsafe {
            match self.state {
                State::Compress(p) => {
                    if !self.frame_complete && self.frame_output_emitted {
                        return Err(Failure {
                            errno: sys::ZSTD_ErrorCode::ZSTD_error_stage_wrong as i32,
                            code: "ERR_ZLIB_INCOMPLETE_FRAME".into(),
                            message: "Cannot reset a zstd stream with an incomplete frame; end the frame or discard the output produced so far".into(),
                        });
                    }
                    let result = sys::ZSTD_CCtx_reset(p.as_ptr(), sys::ZSTD_ResetDirective::ZSTD_reset_session_only);
                    if error(result).is_some() {
                        result
                    } else {
                        sys::ZSTD_CCtx_setPledgedSrcSize(p.as_ptr(), self.pledged.unwrap_or(u64::MAX))
                    }
                }
                State::Decompress(p) => sys::ZSTD_DCtx_reset(p.as_ptr(), sys::ZSTD_ResetDirective::ZSTD_reset_session_only),
            }
        };
        if let Some(e) = error(result) {
            return Err(e);
        }
        self.pledge = self.pledged;
        self.consumed = 0;
        self.frame_complete = matches!(self.state, State::Compress(_));
        self.frame_output_emitted = false;
        self.after_frame_prefix = None;
        self.ignoring_trailing = false;
        Ok(())
    }
}
