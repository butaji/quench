//! The flat string forms native code reads in place: a string record's discriminant, and where
//! each form keeps its units and their count. `FlatUnits` is `repr(u8)`, so a variant is a
//! `repr(C)` struct that starts with the tag, and the mirrors below name its fields' offsets
//! by the compiler's own layout. Any other form (a slice, an append-buffer prefix, parts, a
//! rope) is none of these tags, and a row reading them leaves at its miss.
use super::{JsString, ShortAscii};
use std::mem::offset_of;
use std::rc::Rc;

pub(crate) use quench_bjit::shared_words::string_tag::{TAG_ASCII, TAG_LATIN1, TAG_OFFSET, TAG_SHORT, TAG_WIDE};

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
/// Where a `Short` record keeps its unit count, and its units.
pub(crate) const SHORT_LENGTH_OFFSET: i32 = (offset_of!(ShortForm, text) + offset_of!(ShortAscii, length)) as i32;
pub(crate) const SHORT_UNITS_OFFSET: u64 = (offset_of!(ShortForm, text) + offset_of!(ShortAscii, bytes)) as u64;
/// Where an `Ascii` or `Wide` record keeps the address of its shared allocation and its unit
/// count (the pointer of a `Rc<str>` or `Rc<[u16]>`, then its length).
pub(crate) const SHARED_POINTER_OFFSET: i32 = offset_of!(SharedForm<str>, storage) as i32;
pub(crate) const SHARED_LENGTH_OFFSET: i32 = SHARED_POINTER_OFFSET + size_of::<usize>() as i32;
/// How far into that allocation the units start: where `Rc::as_ptr` points from the pointer the
/// record keeps. A constant, so a string read folds it into its address; a test probes the
/// compiler's own `Rc` for both bytes and UTF-16 units.
#[inline(always)]
pub(crate) fn shared_units_offset() -> u64 {
    SHARED_UNITS_OFFSET
}

/// `Rc`'s allocation is `#[repr(C)] RcInner { strong, weak, value }`, so units of an
/// alignment no larger than a word start two words in; the test `shared_units_offset_matches_rc`
/// probes the compiler's own `Rc` against it.
const SHARED_UNITS_OFFSET: u64 = 2 * size_of::<usize>() as u64;

#[cfg(test)]
#[test]
fn shared_units_offset_matches_rc() {
    let ascii: Rc<str> = Rc::from("probe");
    let wide: Rc<[u16]> = Rc::from([u16::from(b'a'), u16::from(b'b')]);
    let distance = |data: *const u8, handle: *const u8| {
        // SAFETY: a handle starts with the pointer to its allocation, which `SharedForm` places.
        let allocation = unsafe { (handle as *const usize).read() };
        (data as usize - allocation) as u64
    };
    assert_eq!(distance(Rc::as_ptr(&ascii) as *const u8, &ascii as *const Rc<str> as *const u8), SHARED_UNITS_OFFSET);
    assert_eq!(distance(Rc::as_ptr(&wide) as *const u8, &wide as *const Rc<[u16]> as *const u8), SHARED_UNITS_OFFSET);
}

const _: () = assert!(offset_of!(SharedForm<str>, storage) == offset_of!(SharedForm<[u16]>, storage));

#[cfg(test)]
mod tests {
    use super::*;

    /// The units of `string` as the offsets above read them: its tag, width and text.
    fn through_layout(string: &JsString) -> Option<(bool, Vec<u16>)> {
        let record = string as *const JsString as *const u8;
        // SAFETY: the offsets address `string`'s own bytes and the allocation its form shares.
        unsafe {
            let tag = record.offset(TAG_OFFSET as isize).read();
            let word = |offset: i32| (record.offset(offset as isize) as *const usize).read();
            let (units, length, wide) = match tag {
                TAG_SHORT => (record.add(SHORT_UNITS_OFFSET as usize), record.offset(SHORT_LENGTH_OFFSET as isize).read() as usize, false),
                TAG_ASCII | TAG_WIDE => (
                    (word(SHARED_POINTER_OFFSET) as *const u8).add(shared_units_offset() as usize),
                    word(SHARED_LENGTH_OFFSET), tag == TAG_WIDE),
                _ => return None,
            };
            Some((wide, (0..length).map(|at| if wide {
                (units as *const u16).add(at).read()
            } else {
                u16::from(units.add(at).read())
            }).collect()))
        }
    }

    #[test]
    fn the_offsets_read_each_flat_form_of_a_string_record() {
        let long = "x".repeat(crate::vm::wtf16::SHORT_ASCII_CAPACITY + 9);
        let wide = JsString::from_units(&[0x61, 0x20ac, 0xd800]);
        for text in ["", "a", "short text", long.as_str()] {
            let string = JsString::from_str(text);
            let expected: Vec<u16> = text.bytes().map(u16::from).collect();
            assert_eq!(through_layout(&string), Some((false, expected)), "{text:?}");
        }
        // Wide units live behind `Repr::Shared`, not in the record: the record is not a flat form.
        assert_eq!(through_layout(&wide), None);
    }

    #[test]
    fn a_slice_is_none_of_the_forms_a_row_reads() {
        let long = JsString::from_str(&"y".repeat(crate::vm::wtf16::SHORT_ASCII_CAPACITY * 3));
        assert_eq!(through_layout(&long.substring(1..40)), None);
    }
}
