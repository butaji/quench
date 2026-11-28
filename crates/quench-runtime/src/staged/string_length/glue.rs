#![allow(unused_imports)]
use super::*;
use crate::staged::{Concrete, Stage};
use crate::Value;

/// The interpreter's `string_length` of the string `value` among the records at `record_base`:
/// `None` for a form the row does not read, which the caller reads through the string's units.
#[inline(always)]
pub fn concrete_string_length(record_base: *const crate::vm::wtf16::JsString, value: Value) -> Option<Value> {
    let mut stage = crate::staged::Concrete::pure();
    string_length(&mut stage, record_base as u64, value.to_bits(), ()).map(Value::from_bits)
}

/// The interpreter's `length_of` of `value`, among the array records at `arrays` and the string
/// records at `strings`.
#[inline(always)]
pub fn concrete_length_of(arrays: *const crate::heap::ArrayObject, strings: *const crate::vm::wtf16::JsString, value: Value) -> Option<Value> {
    let mut stage = crate::staged::Concrete::pure();
    length_of(&mut stage, (arrays as u64, strings as u64), value.to_bits(), ()).map(Value::from_bits)
}
