//! ICU converters with Node's streaming, error and BOM lifecycle.
//! Converter ownership stays in Rust; each successful open is closed exactly once.
use std::{
    ffi::{c_char, c_void, CString},
    ptr,
};
type Callback = unsafe extern "C" fn(*const c_void, *mut c_void, *const c_char, i32, i32, *mut i32);
const UCNV_UTF8: i32 = 4;
const UCNV_UTF16_BIG_ENDIAN: i32 = 5;
const UCNV_UTF16_LITTLE_ENDIAN: i32 = 6;
const MAX_UTF16_UNITS_PER_SCALAR: usize = 2;
extern "C" {
    #[link_name = concat!("ucnv_open_", env!("QUENCH_ICU_MAJOR"))]
    fn open(name: *const c_char, status: *mut i32) -> *mut c_void;
    #[link_name = concat!("ucnv_close_", env!("QUENCH_ICU_MAJOR"))]
    fn close(converter: *mut c_void);
    #[link_name = concat!("ucnv_getType_", env!("QUENCH_ICU_MAJOR"))]
    fn kind(converter: *const c_void) -> i32;
    #[link_name = concat!("ucnv_getMinCharSize_", env!("QUENCH_ICU_MAJOR"))]
    fn min_char_size(converter: *const c_void) -> i8;
    #[link_name = concat!("ucnv_setSubstChars_", env!("QUENCH_ICU_MAJOR"))]
    fn substitute(converter: *mut c_void, bytes: *const c_char, length: i8, status: *mut i32);
    #[link_name = concat!("ucnv_setToUCallBack_", env!("QUENCH_ICU_MAJOR"))]
    fn callback(
        converter: *mut c_void,
        action: Option<Callback>,
        context: *const c_void,
        old: *mut Option<Callback>,
        old_context: *mut *const c_void,
        status: *mut i32,
    );
    #[link_name = concat!("UCNV_TO_U_CALLBACK_STOP_", env!("QUENCH_ICU_MAJOR"))]
    fn stop(
        context: *const c_void,
        args: *mut c_void,
        source: *const c_char,
        length: i32,
        reason: i32,
        status: *mut i32,
    );
    #[link_name = concat!("ucnv_toUCountPending_", env!("QUENCH_ICU_MAJOR"))]
    fn pending(converter: *const c_void, status: *mut i32) -> i32;
    #[link_name = concat!("ucnv_reset_", env!("QUENCH_ICU_MAJOR"))]
    fn reset(converter: *mut c_void);
    #[link_name = concat!("ucnv_toUnicode_", env!("QUENCH_ICU_MAJOR"))]
    fn to_unicode(
        converter: *mut c_void,
        target: *mut *mut u16,
        target_limit: *const u16,
        source: *mut *const c_char,
        source_limit: *const c_char,
        offsets: *mut i32,
        flush: i8,
        status: *mut i32,
    );
}
pub(crate) struct Converter {
    raw: ptr::NonNull<c_void>,
    minimum: usize,
    unicode: bool,
    ignore_bom: bool,
    bom_seen: bool,
}
impl Drop for Converter {
    fn drop(&mut self) {
        unsafe { close(self.raw.as_ptr()) }
    }
}
impl Converter {
    pub(crate) fn new(name: &str, fatal: bool, ignore_bom: bool) -> Result<Self, ()> {
        let name = CString::new(name).map_err(|_| ())?;
        let mut status = 0;
        let raw = ptr::NonNull::new(unsafe { open(name.as_ptr(), &mut status) }).ok_or(())?;
        let mut value = Self {
            raw,
            minimum: 0,
            unicode: false,
            ignore_bom,
            bom_seen: false,
        };
        if status > 0 {
            return Err(());
        }
        if fatal {
            unsafe {
                callback(
                    raw.as_ptr(),
                    Some(stop),
                    ptr::null(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    &mut status,
                )
            };
            if status > 0 {
                return Err(());
            }
        }
        let minimum = unsafe { min_char_size(raw.as_ptr()) };
        if minimum <= 0 {
            return Err(());
        }
        value.minimum = usize::try_from(minimum).map_err(|_| ())?;
        let bytes = vec![b'?'; value.minimum];
        status = 0;
        unsafe { substitute(raw.as_ptr(), bytes.as_ptr().cast(), minimum, &mut status) };
        if status > 0 {
            return Err(());
        }
        value.unicode = matches!(
            unsafe { kind(raw.as_ptr()) },
            UCNV_UTF8 | UCNV_UTF16_BIG_ENDIAN | UCNV_UTF16_LITTLE_ENDIAN
        );
        Ok(value)
    }
    pub(crate) fn decode(&mut self, bytes: &[u8], flush: bool) -> Result<Vec<u16>, ()> {
        let result = self.decode_chunk(bytes, flush);
        // Node resets a flushed converter even when decoding fails. Streaming
        // failures retain converter/BOM state until a later explicit flush.
        if flush {
            self.bom_seen = false;
            // SAFETY: this converter exclusively owns the live ICU handle.
            unsafe { reset(self.raw.as_ptr()) };
        }
        result
    }

    fn decode_chunk(&mut self, bytes: &[u8], flush: bool) -> Result<Vec<u16>, ()> {
        let mut status = 0;
        let pending = if flush {
            usize::try_from(unsafe { pending(self.raw.as_ptr(), &mut status) }).map_err(|_| ())?
        } else {
            0
        };
        if status > 0 {
            return Err(());
        }
        let limit = MAX_UTF16_UNITS_PER_SCALAR
            .checked_mul(self.minimum)
            .and_then(|n| n.checked_mul(bytes.len().max(pending)))
            .ok_or(())?;
        let mut output = vec![0_u16; limit];
        let mut empty = [0_u16; 1];
        let begin = if limit == 0 {
            empty.as_mut_ptr()
        } else {
            output.as_mut_ptr()
        };
        let mut target = begin;
        let mut source = bytes.as_ptr().cast();
        status = 0;
        unsafe {
            to_unicode(
                self.raw.as_ptr(),
                &mut target,
                begin.add(limit),
                &mut source,
                bytes.as_ptr().add(bytes.len()).cast(),
                ptr::null_mut(),
                i8::from(flush),
                &mut status,
            )
        };
        let result = if status <= 0 {
            let written = unsafe { target.offset_from(begin) };
            if written < 0 || written as usize > limit {
                Err(())
            } else {
                output.truncate(written as usize);
                if !output.is_empty() && self.unicode && !self.ignore_bom && !self.bom_seen {
                    self.bom_seen = true;
                    if output[0] == 0xfeff {
                        output.remove(0);
                    }
                }
                Ok(output)
            }
        } else {
            Err(())
        };
        result
    }
}

/// A complete conversion uses the same converter configuration as streaming.
/// BOM handling belongs to the caller for this legacy complete-input entry.
pub(crate) fn decode(encoding: &str, bytes: &[u8], fatal: bool) -> Result<String, ()> {
    let units = Converter::new(encoding, fatal, true)?.decode(bytes, true)?;
    String::from_utf16(&units).map_err(|_| ())
}

// Unicode property values from ICU's public uchar.h, shared with Node's
// GetColumnWidth policy in src/node_i18n.cc.
const EAST_ASIAN_WIDTH: i32 = 0x1004;
const EMOJI_PRESENTATION: i32 = 58;
const EMOJI_MODIFIER: i32 = 59;
const SOFT_HYPHEN: u32 = 0x00ad;
const ZERO_WIDTH_JOINER: u32 = 0x200d;
const NONSPACING_MARK: i8 = 6;
const ENCLOSING_MARK: i8 = 7;
const CONTROL: i8 = 15;
const FORMAT: i8 = 16;
const AMBIGUOUS: i32 = 1;
const FULLWIDTH: i32 = 3;
const WIDE: i32 = 5;
extern "C" {
    #[link_name = concat!("u_getIntPropertyValue_", env!("QUENCH_ICU_MAJOR"))]
    fn property(codepoint: i32, property: i32) -> i32;
    #[link_name = concat!("u_hasBinaryProperty_", env!("QUENCH_ICU_MAJOR"))]
    fn binary_property(codepoint: i32, property: i32) -> i8;
    #[link_name = concat!("u_charType_", env!("QUENCH_ICU_MAJOR"))]
    fn category(codepoint: i32) -> i8;
}

pub(crate) fn string_width(units: &[u16], ambiguous: bool, expand_emoji: bool) -> u32 {
    let mut previous = 0;
    let mut width = 0;
    // Lone surrogates remain code points, as in ICU's U16_NEXT macro.
    for decoded in char::decode_utf16(units.iter().copied()) {
        let codepoint = decoded.map_or_else(|e| u32::from(e.unpaired_surrogate()), u32::from);
        // SAFETY: the public ICU property APIs accept every Unicode code point,
        // including surrogate values, and all property selectors are uchar.h enums.
        let (east_asian, emoji, modifier, category) = unsafe {
            (
                property(codepoint as i32, EAST_ASIAN_WIDTH),
                binary_property(codepoint as i32, EMOJI_PRESENTATION) != 0,
                binary_property(codepoint as i32, EMOJI_MODIFIER) != 0,
                category(codepoint as i32),
            )
        };
        if expand_emoji || previous != ZERO_WIDTH_JOINER || !(emoji || modifier) {
            width += match east_asian {
                FULLWIDTH | WIDE => 2,
                AMBIGUOUS if ambiguous => 2,
                0 | AMBIGUOUS if emoji => 2,
                _ if codepoint != SOFT_HYPHEN
                    && (matches!(
                        category,
                        NONSPACING_MARK | ENCLOSING_MARK | CONTROL | FORMAT
                    ) || modifier) =>
                {
                    0
                }
                _ => 1,
            };
        }
        previous = codepoint;
    }
    width
}

const ILLEGAL_ARGUMENT: i32 = 1; // U_ILLEGAL_ARGUMENT_ERROR, utypes.h.
const INVALID_CHARACTER: i32 = 10; // U_INVALID_CHAR_FOUND, utypes.h.
extern "C" {
    #[link_name = concat!("ucnv_getMaxCharSize_", env!("QUENCH_ICU_MAJOR"))]
    fn max_char_size(converter: *const c_void) -> i8;
    #[link_name = concat!("ucnv_convertEx_", env!("QUENCH_ICU_MAJOR"))]
    fn convert_ex(
        to: *mut c_void,
        from: *mut c_void,
        target: *mut *mut c_char,
        target_limit: *const c_char,
        source: *mut *const c_char,
        source_limit: *const c_char,
        pivot_start: *mut u16,
        pivot_source: *mut *mut u16,
        pivot_target: *mut *mut u16,
        pivot_limit: *const u16,
        reset: i8,
        flush: i8,
        status: *mut i32,
    );
    #[link_name = concat!("ucnv_fromUChars_", env!("QUENCH_ICU_MAJOR"))]
    fn from_units(
        converter: *mut c_void,
        target: *mut c_char,
        capacity: i32,
        source: *const u16,
        length: i32,
        status: *mut i32,
    ) -> i32;
    #[link_name = concat!("u_errorName_", env!("QUENCH_ICU_MAJOR"))]
    fn error_name(status: i32) -> *const c_char;
}

pub(crate) fn status_name(status: i32) -> String {
    // SAFETY: ICU returns a static NUL-terminated name for every status value.
    unsafe { std::ffi::CStr::from_ptr(error_name(status)) }
        .to_string_lossy()
        .into_owned()
}

pub(crate) fn transcode(bytes: &[u8], from: &str, to: &str) -> Result<Vec<u8>, i32> {
    let encoding = |name| match name {
        "ascii" => Some("us-ascii"),
        "latin1" => Some("iso8859-1"),
        "utf8" => Some("utf-8"),
        "utf16le" => Some("utf16le"),
        _ => None,
    };
    let from_name = encoding(from).ok_or(ILLEGAL_ARGUMENT)?;
    let to_name = encoding(to).ok_or(ILLEGAL_ARGUMENT)?;
    if matches!(from, "ascii" | "latin1") && to == "utf16le" {
        // Node's SIMD Latin-1 path preserves all eight bits even for ASCII input.
        return Ok(bytes.iter().flat_map(|b| [*b, 0]).collect());
    }
    if from == "utf8" && to == "utf16le" {
        let text = std::str::from_utf8(bytes).map_err(|_| INVALID_CHARACTER)?;
        if text.is_empty() {
            return Err(INVALID_CHARACTER);
        }
        return Ok(text.encode_utf16().flat_map(u16::to_le_bytes).collect());
    }
    if from == "utf16le" && to == "utf8" {
        let units = bytes
            .chunks_exact(2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .collect::<Vec<_>>();
        let text = String::from_utf16(&units).map_err(|_| INVALID_CHARACTER)?;
        if text.is_empty() {
            return Err(INVALID_CHARACTER);
        }
        return Ok(text.into_bytes());
    }
    let target_converter = Converter::new(to_name, false, true).map_err(|_| ILLEGAL_ARGUMENT)?;
    let mut status = 0;
    if from == "utf16le" && to != "utf16le" {
        let units = bytes
            .chunks_exact(2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .collect::<Vec<_>>();
        let length = i32::try_from(units.len()).map_err(|_| ILLEGAL_ARGUMENT)?;
        let mut output = vec![0_u8; units.len()];
        // SAFETY: all buffers cover the checked lengths; the uniquely owned
        // destination converter remains live for the complete conversion.
        let written = unsafe {
            from_units(
                target_converter.raw.as_ptr(),
                output.as_mut_ptr().cast(),
                length,
                units.as_ptr(),
                length,
                &mut status,
            )
        };
        if status > 0 {
            return Err(status);
        }
        output.truncate(written as usize);
        return Ok(output);
    }
    // Node leaves the source converter at ICU's default substitution policy.
    let name = CString::new(from_name).map_err(|_| ILLEGAL_ARGUMENT)?;
    // SAFETY: the name is NUL-terminated and the status output is live.
    let raw = unsafe { open(name.as_ptr(), &mut status) };
    let raw = ptr::NonNull::new(raw).ok_or(status)?;
    let source_converter = Converter {
        raw,
        minimum: 0,
        unicode: false,
        ignore_bom: true,
        bom_seen: false,
    };
    if status > 0 {
        return Err(status);
    }
    // SAFETY: the target converter is live; ICU's maximum width is positive.
    let maximum = unsafe { max_char_size(target_converter.raw.as_ptr()) };
    let capacity = bytes
        .len()
        .checked_mul(maximum as usize)
        .ok_or(ILLEGAL_ARGUMENT)?;
    let mut output = vec![0_u8; capacity.max(1)];
    let begin = output.as_mut_ptr().cast::<c_char>();
    let mut target = begin;
    let mut source = bytes.as_ptr().cast();
    status = 0;
    // SAFETY: both converters are uniquely owned, all bounds refer to live
    // allocations, and null pivot pointers request ICU's internal pivot buffer.
    unsafe {
        convert_ex(
            target_converter.raw.as_ptr(),
            source_converter.raw.as_ptr(),
            &mut target,
            begin.add(capacity),
            &mut source,
            bytes.as_ptr().add(bytes.len()).cast(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null(),
            1,
            1,
            &mut status,
        );
    }
    if status > 0 {
        return Err(status);
    }
    // SAFETY: successful ICU conversion leaves target within this allocation.
    let written = unsafe { target.offset_from(begin) } as usize;
    output.truncate(written);
    Ok(output)
}
