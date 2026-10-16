//! ICU's legacy converter tables and error consumption differ from WHATWG
//! replacements for Big5, Shift-JIS and EUC. Use the same stable C API as Node.
use std::{
    ffi::{c_char, c_void, CString},
    ptr,
};

type ToUnicodeCallback =
    unsafe extern "C" fn(*const c_void, *mut c_void, *const c_char, i32, i32, *mut i32);
const BUFFER_OVERFLOW: i32 = 15; // U_BUFFER_OVERFLOW_ERROR, utypes.h.

extern "C" {
    #[link_name = concat!("ucnv_open_", env!("QUENCH_ICU_MAJOR"))]
    fn open(name: *const c_char, status: *mut i32) -> *mut c_void;
    #[link_name = concat!("ucnv_close_", env!("QUENCH_ICU_MAJOR"))]
    fn close(converter: *mut c_void);
    #[link_name = concat!("ucnv_setToUCallBack_", env!("QUENCH_ICU_MAJOR"))]
    fn set_callback(
        converter: *mut c_void,
        callback: Option<ToUnicodeCallback>,
        context: *const c_void,
        old: *mut Option<ToUnicodeCallback>,
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
    #[link_name = concat!("ucnv_getMinCharSize_", env!("QUENCH_ICU_MAJOR"))]
    fn min_char_size(converter: *const c_void) -> i8;
    #[link_name = concat!("ucnv_setSubstChars_", env!("QUENCH_ICU_MAJOR"))]
    fn set_substitution(converter: *mut c_void, bytes: *const c_char, length: i8, status: *mut i32);
    #[link_name = concat!("ucnv_toUChars_", env!("QUENCH_ICU_MAJOR"))]
    fn to_utf16(
        converter: *mut c_void,
        output: *mut u16,
        capacity: i32,
        source: *const c_char,
        length: i32,
        status: *mut i32,
    ) -> i32;
}

struct Converter(*mut c_void);
impl Drop for Converter {
    fn drop(&mut self) {
        // SAFETY: this guard uniquely owns a successful ucnv_open result.
        unsafe { close(self.0) }
    }
}

pub(crate) fn decode(encoding: &str, bytes: &[u8], fatal: bool) -> Result<String, ()> {
    let name = CString::new(encoding).map_err(|_| ())?;
    let length = i32::try_from(bytes.len()).map_err(|_| ())?;
    let mut status = 0;
    // SAFETY: name is NUL-terminated; ICU writes only the supplied status slot.
    let raw = unsafe { open(name.as_ptr(), &mut status) };
    if raw.is_null() {
        return Err(());
    }
    let converter = Converter(raw);
    if status > 0 {
        return Err(());
    }
    if fatal {
        // SAFETY: converter is live. The ICU callback has the declared C ABI;
        // no callback context or previous-action outputs are requested.
        unsafe {
            set_callback(
                converter.0,
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
    // Node configures '?' as the converter's byte substitution. ICU also uses
    // this setting to choose U+FFFD rather than a table's legacy SUB character
    // for invalid input on its Unicode side.
    // SAFETY: converter is live; ICU's minimum byte width is a positive int8.
    let minimum = unsafe { min_char_size(converter.0) };
    let substitution = vec![b'?'; usize::try_from(minimum).map_err(|_| ())?];
    status = 0;
    // SAFETY: substitution stays live for its exact, ICU-derived byte count.
    unsafe {
        set_substitution(
            converter.0,
            substitution.as_ptr().cast(),
            minimum,
            &mut status,
        )
    };
    if status > 0 {
        return Err(());
    }
    status = 0;
    // SAFETY: ICU's documented preflight accepts a null destination at capacity
    // zero; source stays live for the exact checked byte count.
    let needed = unsafe {
        to_utf16(
            converter.0,
            ptr::null_mut(),
            0,
            bytes.as_ptr().cast(),
            length,
            &mut status,
        )
    };
    if status > 0 && status != BUFFER_OVERFLOW {
        return Err(());
    }
    let mut output = vec![0_u16; usize::try_from(needed).map_err(|_| ())?];
    status = 0;
    // SAFETY: preflight supplies the capacity; toUChars resets converter state
    // for each complete conversion and respects the supplied destination size.
    let written = unsafe {
        to_utf16(
            converter.0,
            output.as_mut_ptr(),
            needed,
            bytes.as_ptr().cast(),
            length,
            &mut status,
        )
    };
    if status > 0 || written < 0 || written > needed {
        return Err(());
    }
    output.truncate(written as usize);
    String::from_utf16(&output).map_err(|_| ())
}
