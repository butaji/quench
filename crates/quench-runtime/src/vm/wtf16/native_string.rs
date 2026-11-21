//! The flat string forms native code reads in place: a string record's discriminant, and where
//! each form keeps its units and their count. `FlatUnits` is `repr(u8)`, so a variant is a
//! `repr(C)` struct that starts with the tag, and the mirrors below name its fields' offsets
//! by the compiler's own layout. Any other form (a slice, an append-buffer prefix, parts, a
//! rope) is none of these tags, and a row reading them leaves at its miss.
use super::{JsString, ShortAscii};
use crate::staged::string_form::{ShortFormHead, SharedFormHead};
use std::mem::offset_of;
use std::rc::Rc;

pub(crate) use quench_bjit::shared_words::string_tag::{TAG_ASCII, TAG_ASCII_PREFIX, TAG_LATIN1, TAG_OFFSET, TAG_SHORT, TAG_WIDE};
/// The offsets and sizes are the shared head's (`staged/string_form.rs`), which the stencils take too.
pub(crate) use crate::staged::string_form::{
    shared_units_offset, RECORD_SIZE, SHARED_LENGTH_OFFSET, SHARED_POINTER_OFFSET, SHORT_LENGTH_OFFSET,
    SHORT_UNITS_OFFSET,
};

/// The runtime's short form: the head with its inline `ShortAscii`.
type ShortForm = ShortFormHead<ShortAscii>;
/// The runtime's shared form: the head with its `Rc` handle.
type SharedForm<T> = SharedFormHead<Rc<T>>;

const _: () = assert!(size_of::<JsString>() == RECORD_SIZE);
const _: () = assert!(offset_of!(ShortForm, text) + offset_of!(ShortAscii, length) == SHORT_LENGTH_OFFSET as usize);
const _: () = assert!(offset_of!(ShortForm, text) + offset_of!(ShortAscii, bytes) == SHORT_UNITS_OFFSET as usize);
const _: () = assert!(offset_of!(SharedForm<str>, storage) == SHARED_POINTER_OFFSET as usize);
const _: () = assert!(offset_of!(SharedForm<[u16]>, storage) == SHARED_POINTER_OFFSET as usize);
/// The runtime's prefix form: the head with its buffer's `Rc`.
type PrefixForm = crate::staged::string_form::PrefixFormHead<Rc<super::append_buffer::AppendBuffer<u8>>>;
const _: () = assert!(offset_of!(PrefixForm, buffer) == crate::staged::string_form::PREFIX_BUFFER_OFFSET as usize);
const _: () = assert!(offset_of!(PrefixForm, length) == crate::staged::string_form::PREFIX_LENGTH_OFFSET as usize);

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
    assert_eq!(distance(Rc::as_ptr(&ascii) as *const u8, &ascii as *const Rc<str> as *const u8), shared_units_offset());
    assert_eq!(distance(Rc::as_ptr(&wide) as *const u8, &wide as *const Rc<[u16]> as *const u8), shared_units_offset());
}

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
