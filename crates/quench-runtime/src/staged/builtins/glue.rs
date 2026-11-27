#![allow(unused_imports)]
use super::*;
use crate::staged::elements::{Backing, append_element, array_backing, other, recount, resizable_backing};
use crate::staged::math::FloatUnary;
use crate::staged::numeric::{Domain, NumericStage};
use crate::staged::region::Region;
use crate::staged::{Concrete, Stage};
use crate::Value;
use crate::heap::ArrayObject;

/// The interpreter's `push` of `array` (`step_array_native_of`), a direct array among the records at
/// `record_base` whose backing has room for `room` values: the new length.
#[inline(always)]
pub fn concrete_array_push(record_base: *const ArrayObject, array: Value, room: usize, value: Value) -> Option<Value> {
    let mut stage = Concrete::pure();
    let backing = array_backing(&mut stage, record_base as u64, array.to_bits(), ())?;
    step_array_native_of(&mut stage, ArrayNative::Push { room: room as u64, value: value.to_bits() }, backing, ()).map(Value::from_bits)
}

/// The interpreter's `pop` of `array` (`step_array_native_of`), a direct array among the records at `record_base`.
#[inline(always)]
pub fn concrete_array_pop(record_base: *const ArrayObject, array: Value) -> Option<Value> {
    let mut stage = Concrete::pure();
    let backing = array_backing(&mut stage, record_base as u64, array.to_bits(), ())?;
    step_array_native_of(&mut stage, ArrayNative::Pop, backing, ()).map(Value::from_bits)
}
