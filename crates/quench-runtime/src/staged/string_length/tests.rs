use super::*;
use crate::Value;
use crate::heap::Heap;
use crate::vm::wtf16::{JsString, SHORT_ASCII_CAPACITY};

/// The string row of `value` on the heap's own records.
fn length(heap: &Heap, value: Value) -> Option<Value> {
    concrete_string_length(heap.string_record_base()?, value)
}

#[test]
fn the_row_reads_the_length_of_every_flat_form_through_the_record() {
    let mut heap = Heap::new();
    let long = "k".repeat(SHORT_ASCII_CAPACITY + 5);
    let forms = [
        (heap.alloc_string(JsString::from_str("")), 0),
        (heap.alloc_string(JsString::from_str("hello")), 5),
        (heap.alloc_string(JsString::from_str(&long)), long.len()),
    ];
    for (value, units) in forms {
        assert_eq!(length(&heap, value), Some(Value::length(units)), "the length of a {units}-unit string");
    }
    // A UTF-16 string is held shared (`Repr::Shared`, so a rope can flatten in place), never
    // as a flat `Wide` record, so the row leaves for it and the caller reads its units.
    let wide = heap.alloc_string(JsString::from_units(&[0x61, 0x20ac]));
    assert_eq!(length(&heap, wide), None, "a UTF-16 string has no flat record");
    assert_eq!(length(&heap, Value::integer(3)), None, "no string");
}

#[test]
fn a_slice_leaves() {
    let mut heap = Heap::new();
    let base = heap.alloc_string(JsString::from_str(&"s".repeat(SHORT_ASCII_CAPACITY * 3)));
    let sliced = heap.string(base).unwrap().substring(1..40);
    let slice = heap.alloc_string(sliced);
    assert_eq!(length(&heap, slice), None);
}

/// The row's only operands are the record bases: it names no per-site hole.
#[test]
fn the_row_has_no_per_site_hole() {
    let mut stage = crate::staged::trace::Trace::default();
    let (base, value) = (stage.constant(0), stage.constant(1));
    string_length(&mut stage, base, value, ());
    assert!(stage.holes.is_empty(), "{:?}", stage.holes);
}
