//! The flat string forms native code reads in place: a string record's discriminant, and where
//! each form keeps its unit count. `FlatUnits` is `repr(u8)`, so a variant is a `repr(C)`
//! struct that starts with the tag, and the mirrors below name its fields' offsets by the
//! compiler's own layout. Any other form (a slice, an append-buffer prefix, parts, a rope) is
//! none of these tags, and a row reading them leaves at its miss.
use super::{JsString, ShortAscii};
use std::mem::offset_of;
use std::rc::Rc;

/// `FlatUnits::Short`: ASCII text inline in the record.
pub(crate) const TAG_SHORT: u8 = 0;
/// `FlatUnits::Ascii`: ASCII text in a shared allocation of its bytes.
pub(crate) const TAG_ASCII: u8 = 1;
/// `FlatUnits::AsciiSlice`, which follows `Ascii`.
const TAG_ASCII_SLICE: u8 = TAG_ASCII + 1;
/// `FlatUnits::Wide`: UTF-16 text in a shared allocation of its units.
pub(crate) const TAG_WIDE: u8 = TAG_ASCII_SLICE + 1;

#[repr(C)]
struct ShortForm {
    tag: u8,
    text: ShortAscii,
}

#[repr(C)]
struct SharedForm<T: ?Sized> {
    tag: u8,
    storage: Rc<T>,
}

/// Bytes a string record occupies.
pub(crate) const RECORD_SIZE: usize = size_of::<JsString>();
/// Where a record keeps its form's tag: the record is its `FlatUnits`.
pub(crate) const TAG_OFFSET: i32 = 0;
/// Where a `Short` record keeps its unit count.
pub(crate) const SHORT_LENGTH_OFFSET: i32 = (offset_of!(ShortForm, text) + offset_of!(ShortAscii, length)) as i32;
/// Where an `Ascii` or `Wide` record keeps its unit count: after the pointer of its shared
/// allocation (the pointer of a `Rc<str>` or `Rc<[u16]>`, then its length).
pub(crate) const SHARED_LENGTH_OFFSET: i32 = offset_of!(SharedForm<str>, storage) as i32 + size_of::<usize>() as i32;

const _: () = assert!(offset_of!(SharedForm<str>, storage) == offset_of!(SharedForm<[u16]>, storage));

#[cfg(test)]
mod tests {
    use super::*;

    /// The unit count of `string` as the offsets above read it: `None` for a form no row reads.
    fn length_through_layout(string: &JsString) -> Option<usize> {
        let record = string as *const JsString as *const u8;
        // SAFETY: the offsets address `string`'s own bytes.
        unsafe {
            match record.offset(TAG_OFFSET as isize).read() {
                TAG_SHORT => Some(record.offset(SHORT_LENGTH_OFFSET as isize).read() as usize),
                TAG_ASCII | TAG_WIDE => Some((record.offset(SHARED_LENGTH_OFFSET as isize) as *const usize).read()),
                _ => None,
            }
        }
    }

    #[test]
    fn the_offsets_read_the_length_of_each_flat_form_of_a_string_record() {
        let long = "x".repeat(crate::vm::wtf16::SHORT_ASCII_CAPACITY + 9);
        for text in ["", "a", "short text", long.as_str()] {
            assert_eq!(length_through_layout(&JsString::from_str(text)), Some(text.len()), "{text:?}");
        }
        assert_eq!(length_through_layout(&JsString::from_units(&[0x61, 0x20ac, 0xd800])), Some(3));
    }

    #[test]
    fn a_slice_is_none_of_the_forms_a_row_reads() {
        let long = JsString::from_str(&"y".repeat(crate::vm::wtf16::SHORT_ASCII_CAPACITY * 3));
        assert_eq!(length_through_layout(&long.substring(1..40)), None);
    }
}
