//! The hot builtins' steady cases as staged rows: `String.prototype.charCodeAt` and `charAt` of
//! a flat string, string and array `length`, `Array.prototype.push` and `pop` of a dense array,
//! and `Number.isInteger`. The interpreter runs each on `Concrete` (`concrete_*`), a
//! specializer emits it. A row leaves at its `miss` for any case the builtin's own steps
//! decide: a rope, a non-int32 index, a hole, a frozen or shared backing.
//!
//! A row reads only what its caller proved once: the receiver's storage form (`UnitWidth`, a
//! static argument that leaves no code), the array backing (`elements::array_backing`) and the
//! prototype chain holding no index, which is a dependency, not a per-call test.

use super::elements::{Backing, append_element, array_backing, other, recount, resizable_backing};
use super::math::FloatUnary;
use super::numeric::{Domain, NumericStage};
use super::region::Region;
use super::{Concrete, Stage};
use crate::Value;
use crate::heap::ArrayObject;
use crate::vm::operations::{NumberArithmetic, NumberRelation};

/// The storage width of a flat string's units, a property of the string's form.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum UnitWidth {
    /// ASCII or Latin-1 text, a byte a unit (`Units::Ascii`, `Units::Latin1`).
    Narrow,
    /// UTF-16 text (`Units::Wide`).
    Wide,
}

/// The largest ASCII code unit.
const ASCII_MAX: u64 = 0x7f;
/// The largest length a Number holds as an int32 (`Value::length`).
const INT32_MAX: u64 = i32::MAX as u64;

/// The int32 payload of the value word `bits`, zero-extended: `None` (leaving at `miss`) unless
/// `bits` is an int32. A negative int32's payload is above every length, so an index test
/// against a length rejects it.
#[inline(always)]
fn int32_payload<S: Stage>(stage: &mut S, bits: S::Word, miss: S::Miss) -> Option<S::Word> {
    let is_int = super::value::is_int(stage, bits);
    stage.guard(is_int, miss)?;
    Some(stage.and_imm(bits, u64::from(u32::MAX)))
}


/// The unit at `index` of the `units` run, which `index` is within.
#[inline(always)]
fn unit_at<S: Stage>(stage: &mut S, width: UnitWidth, units: S::Word, index: S::Word) -> S::Word {
    super::code_units::code_unit(stage, width == UnitWidth::Wide, units, index)
}

/// `charCodeAt(index)` of the flat string of `length` units at `units`, `index` a value word:
/// the unit as an int32, or NaN outside the string. `None` (leaving at `miss`) for an index
/// that is no int32, which the native coerces.
#[inline(always)]
pub(crate) fn char_code_at<S: Stage>(stage: &mut S, width: UnitWidth, units: S::Word, length: S::Word,
    index: S::Word, miss: S::Miss) -> Option<S::Word> {
    let index = int32_payload(stage, index, miss)?;
    let inside = stage.ult(index, length);
    Some(stage.choose(inside, #[inline(always)] |stage| {
        let unit = unit_at(stage, width, units, index);
        super::value::int_word(stage, unit)
    }, #[inline(always)] |stage| stage.constant(Value::NAN.to_bits())))
}

/// `charCodeAt(index)` of the flat string of width `width` at `units` for an index the caller proves
/// an int32 below the string's length: the unit as an int32, with no bounds test (`char_code_at`).
/// `None` (leaving at `miss`) for an index that is no int32, which the native coerces.
#[inline(always)]
pub(crate) fn char_code_at_within<S: Stage>(stage: &mut S, width: UnitWidth, units: S::Word,
    index: S::Word, miss: S::Miss) -> Option<S::Word> {
    let index = int32_payload(stage, index, miss)?;
    let unit = unit_at(stage, width, units, index);
    Some(super::value::int_word(stage, unit))
}

/// `charAt(index)` of the flat string at `units` where the unit is ASCII: that unit, whose
/// one-character string the caller names (a shared string per unit). `None` (leaving at
/// `miss`) for an index that is no int32 or outside the string, or a unit past ASCII.
#[inline(always)]
pub(crate) fn char_at_ascii<S: Stage>(stage: &mut S, width: UnitWidth, units: S::Word, length: S::Word,
    index: S::Word, miss: S::Miss) -> Option<S::Word> {
    let index = int32_payload(stage, index, miss)?;
    let inside = stage.ult(index, length);
    stage.guard(inside, miss)?;
    char_at_unit(stage, width, units, index, miss)
}

/// `char_at_ascii` for an index the caller proves an int32 below the string's length: the same unit
/// with no bounds test.
#[inline(always)]
pub(crate) fn char_at_ascii_within<S: Stage>(stage: &mut S, width: UnitWidth, units: S::Word,
    index: S::Word, miss: S::Miss) -> Option<S::Word> {
    let index = int32_payload(stage, index, miss)?;
    char_at_unit(stage, width, units, index, miss)
}

/// The ASCII unit at `index` (inside the string) of the flat string at `units`, leaving at `miss`
/// for a unit past ASCII in a wide string.
#[inline(always)]
fn char_at_unit<S: Stage>(stage: &mut S, width: UnitWidth, units: S::Word, index: S::Word, miss: S::Miss) -> Option<S::Word> {
    let unit = unit_at(stage, width, units, index);
    // A narrow string's units are ASCII by its form.
    if width == UnitWidth::Wide {
        let past = stage.ugt_imm(unit, ASCII_MAX);
        let ascii = stage.not(past);
        stage.guard(ascii, miss)?;
    }
    Some(unit)
}


/// The longest length a push grows within int32 (`Value::length`).
const PUSH_FROM_MAX: u64 = INT32_MAX - 1;

/// `push(value)` of the dense array `backing`, whose elements have room for `room` values,
/// where OrdinarySet of the index at its length is a plain append: the new `length`, as a
/// value word. `None` (leaving at `miss`, before any write) for a shared, sealed or extended
/// backing, a full one or one at the last int32 length, which the generic steps grow or
/// refuse. The caller proves the prototype chain holds no index (`Vm::chain_holds_no_index`),
/// a dependency, and that the store of `value` passes the write barrier unlogged.
#[inline(always)]
pub fn array_push_one<S: Stage>(stage: &mut S, backing: Backing<S::Word>, room: S::Word, value: S::Word,
    miss: S::Miss) -> Option<S::Word> {
    resizable_backing(stage, backing, miss)?;
    let past = stage.ugt_imm(backing.length, PUSH_FROM_MAX);
    let within = stage.not(past);
    stage.guard(within, miss)?;
    let grown = append_element(stage, backing.values, backing.length, room, backing.length, value, miss)?;
    Some(super::value::int_word(stage, grown))
}

/// Continues where the value word `value` holds no heap reference, so a store of it passes
/// the write barrier unlogged; `None` (leaving at `miss`) for a reference, whose store the
/// heap logs (`Heap::record_array_store_mut`).
#[inline(always)]
pub fn unreferenced<S: Stage>(stage: &mut S, value: S::Word, miss: S::Miss) -> Option<()> {
    let reference = super::value::is_heap(stage, value);
    let plain = stage.not(reference);
    stage.guard(plain, miss)
}

/// Removes the last element of the backing at `values` of `length` values and answers it, or
/// `undefined` for an empty one. `None` (leaving at `miss`) at a hole, which the prototypes
/// answer. The element's count among the backing's others follows (`elements::recount`).
#[inline(always)]
pub(crate) fn pop_last<S: Stage>(stage: &mut S, values: S::Word, length: S::Word, miss: S::Miss) -> Option<S::Word> {
    let empty = stage.eq_imm(length, 0u32);
    stage.choose_leaving(empty, #[inline(always)] |stage| Some(stage.constant(Value::UNDEFINED.to_bits())), #[inline(always)] |stage| {
        let one = stage.constant(1);
        let last = stage.sub(length, one);
        let offset = stage.mul_imm(last, size_of::<Value>() as u64);
        let address = stage.add(values, offset);
        let value = stage.load_u64(Region::Elements.mutable(), address, 0);
        let present = stage.ne_imm(value, Value::DELETED.to_bits());
        stage.guard(present, miss)?;
        let others = stage.load_u32(Region::Elements.mutable(), values, crate::heap::OTHERS_FROM_VALUES);
        let (added, removed) = (stage.constant(0), other(stage, value));
        let others = recount(stage, others, added, removed);
        stage.store_u32(Region::Elements.mutable(), values, crate::heap::OTHERS_FROM_VALUES, others);
        stage.store_u32(Region::Elements.mutable(), values, crate::heap::LENGTH_FROM_VALUES, last);
        Some(value)
    })
}

/// `pop()` of the dense array `backing` (`pop_last`), where its backing is resizable
/// (`elements::resizable_backing`): the generic steps' Set of `length` is then one store. The
/// caller retires the array storage proofs a shrinking backing falsifies.
#[inline(always)]
pub fn array_pop<S: Stage>(stage: &mut S, backing: Backing<S::Word>, miss: S::Miss) -> Option<S::Word> {
    resizable_backing(stage, backing, miss)?;
    pop_last(stage, backing.values, backing.length, miss)
}

/// A native of a dense array's backing: `push(value)` within `room` values (`array_push_one`;
/// the caller proves the store unlogged, `unreferenced`), or `pop()` (`array_pop`). The
/// interpreter (`concrete_array_push`, `concrete_array_pop`), the b-jit's builtin rows and the
/// u-jit's lowering run it through the one step, `step_array_native_of`.
#[derive(Clone, Copy)]
pub enum ArrayNative<W> {
    /// `push(value)` where the backing has room for `room` values.
    Push { room: W, value: W },
    /// `pop()`.
    Pop,
}

/// The value word `native` answers on the dense array `backing`: the one step of an array
/// native, on any stage. `None` (leaving at `miss`, before any write) where its row leaves.
#[inline(always)]
pub fn step_array_native_of<S: Stage>(stage: &mut S, native: ArrayNative<S::Word>, backing: Backing<S::Word>, miss: S::Miss)
    -> Option<S::Word> {
    match native {
        ArrayNative::Push { room, value } => array_push_one(stage, backing, room, value, miss),
        ArrayNative::Pop => array_pop(stage, backing, miss),
    }
}

/// `Array.isArray` of an ordinary array: `true` where `value` is a direct array among the array records at
/// `record_base` whose record holds no extras (an arguments object always holds extras, its brand being one).
/// Any other value leaves at `miss`, before anything is written, where the native's own steps answer it
/// (`Vm::steady_is_array`: a plain object, a proxy). The b-jit's builtin row and the u-jit's lowering run this one step.
#[inline(always)]
pub fn is_plain_array<S: Stage>(stage: &mut S, record_base: S::Word, value: S::Word, miss: S::Miss) -> Option<S::Word> {
    use crate::heap::NATIVE_ARRAY_LAYOUT as ARRAY;
    let direct = super::value::is_direct_array(stage, value);
    stage.guard(direct, miss)?;
    let present = stage.ne_imm(record_base, 0u32);
    stage.guard(present, miss)?;
    let record = super::elements::array_record(stage, record_base, value);
    let payload = stage.load_u64(Region::Payload.mutable(), record, (ARRAY.elements_offset + ARRAY.elements_header_offset) as i32);
    let plain = super::object::plain_payload(stage, payload);
    stage.guard(plain, miss)?;
    Some(stage.constant(Value::boolean(true).to_bits()))
}

/// Whether the Number word `number` of `domain` is a finite integral value
/// (`Number.isInteger` of a Number): an int32 always, a double when truncation leaves it and
/// it differs from itself by something other than NaN, which only infinities do.
#[inline(always)]
pub fn is_integral<S: NumericStage>(stage: &mut S, domain: Domain, number: S::Word) -> S::Cond {
    match domain {
        Domain::Int32 => stage.relation(NumberRelation::Equal, domain, number, number),
        Domain::Float64 => {
            let truncated = stage.float_unary(FloatUnary::Trunc, number);
            let whole = stage.relation(NumberRelation::Equal, domain, number, truncated);
            let spread = stage.float_arithmetic(NumberArithmetic::Subtract, number, number);
            let finite = stage.relation(NumberRelation::Equal, domain, spread, spread);
            stage.both(whole, finite)
        }
    }
}

/// The interpreter's `Number.isInteger` of `value` (`call_native::step_number_native_of`): false for any value
/// that is no Number.
#[inline(always)]
pub(crate) fn concrete_is_integral(value: Value) -> bool {
    concrete_number_predicate(super::call_native::NumberNative::IsInteger, value)
}

/// The interpreter's `Number.isInteger`, `isSafeInteger`, `isNaN` or `isFinite` of `value`
/// (`call_native::step_number_native_of`): false for any value that is no Number.
#[inline(always)]
pub(crate) fn concrete_number_predicate(native: super::call_native::NumberNative, value: Value) -> bool {
    let mut stage = Concrete::new(std::ptr::null());
    let answer = super::call_native::step_number_native_of(&mut stage, native, &[value.to_bits()], ());
    answer == Some(Value::boolean(true).to_bits())
}

/// SameValue of the Numbers `left` and `right` (`call_native::NumberNative::SameValue`); `None`
/// when either is no Number.
#[inline(always)]
pub(crate) fn concrete_same_number(left: Value, right: Value) -> Option<bool> {
    let mut stage = Concrete::new(std::ptr::null());
    let answer = super::call_native::step_number_native_of(&mut stage, super::call_native::NumberNative::SameValue, &[left.to_bits(), right.to_bits()], ())?;
    Some(answer == Value::boolean(true).to_bits())
}

/// The interpreter's `push` of `array` (`step_array_native_of`), a direct array among the records at
/// `record_base` whose backing has room for `room` values: the new length.
#[inline(always)]
pub(crate) fn concrete_array_push(record_base: *const ArrayObject, array: Value, room: usize, value: Value) -> Option<Value> {
    let mut stage = Concrete::new(std::ptr::null());
    let backing = array_backing(&mut stage, record_base as u64, array.to_bits(), ())?;
    step_array_native_of(&mut stage, ArrayNative::Push { room: room as u64, value: value.to_bits() }, backing, ()).map(Value::from_bits)
}

/// The interpreter's `pop` of `array` (`step_array_native_of`), a direct array among the records at `record_base`.
#[inline(always)]
pub(crate) fn concrete_array_pop(record_base: *const ArrayObject, array: Value) -> Option<Value> {
    let mut stage = Concrete::new(std::ptr::null());
    let backing = array_backing(&mut stage, record_base as u64, array.to_bits(), ())?;
    step_array_native_of(&mut stage, ArrayNative::Pop, backing, ()).map(Value::from_bits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heap::{ArrayElements, NATIVE_ARRAY_LAYOUT as ARRAY, NATIVE_OBJECT_LAYOUT as OBJECT};

    fn stage() -> Concrete { Concrete::new(std::ptr::null()) }

    #[test]
    fn length_is_an_int32_word_up_to_int32_and_leaves_past_it() {
        let length = |length: u64| super::super::value::length_value(&mut stage(), length, ());
        assert_eq!(length(0), Some(Value::integer(0).to_bits()));
        assert_eq!(length(i32::MAX as u64), Some(Value::integer(i32::MAX).to_bits()));
        assert_eq!(length(i32::MAX as u64 + 1), None);
    }

    /// The backing of `elements` with room for `room` more values, and a record of a plain
    /// array whose meta word is `meta`, as `writable_backing` reads one.
    fn backing_of(elements: &mut ArrayElements, room: usize, meta: u32, record: &mut [u64]) -> (Backing<u64>, u64) {
        let unique = elements.make_mut();
        unique.reserve(room);
        let capacity = unique.capacity() as u64;
        let header = elements.as_ptr() as u64;
        let record_address = record.as_mut_ptr() as u64;
        let mut stage = stage();
        // Without extras, the object's payload word is the backing's header pointer.
        stage.store_u64(Region::Payload.mutable(), record_address, (ARRAY.elements_offset + ARRAY.elements_header_offset) as i32, header);
        let layout = crate::heap::layouts::intern(u64::from(meta) << u32::BITS);
        stage.store_u32(Region::ShapeWord.mutable(), record_address, (ARRAY.object_offset + OBJECT.layout_offset) as i32, u64::from(layout));
        let values = header + ARRAY.header_values_offset as u64;
        let length = stage.load_u32(Region::Elements.mutable(), header, ARRAY.header_length_offset as i32);
        (Backing { record: record_address, header, values, length }, capacity)
    }

    fn record() -> Vec<u64> { vec![0; ARRAY.record_size.div_ceil(size_of::<u64>())] }

    #[test]
    fn push_appends_within_room_and_answers_the_new_length() {
        let mut elements = ArrayElements::from_slice(&[Value::integer(1), Value::number(0.5)]);
        let (mut record, mut s) = (record(), stage());
        let (backing, capacity) = backing_of(&mut elements, 2, 0, &mut record);
        let pushed = array_push_one(&mut s, backing, capacity, Value::integer(9).to_bits(), ());
        assert_eq!(pushed, Some(Value::integer(3).to_bits()));
        assert_eq!(elements.len(), 3);
        assert_eq!(elements[2].to_bits(), Value::integer(9).to_bits());
    }

    #[test]
    fn push_on_a_frozen_or_full_backing_leaves_for_the_generic_path() {
        let mut elements = ArrayElements::from_slice(&[Value::integer(1)]);
        let (mut record, mut s) = (record(), stage());
        let (frozen, capacity) = backing_of(&mut elements, 2, ARRAY.frozen_mask, &mut record);
        assert_eq!(array_push_one(&mut s, frozen, capacity, Value::integer(9).to_bits(), ()), None, "frozen");
        assert_eq!(elements.len(), 1, "nothing was written");
        let (open, _) = backing_of(&mut elements, 0, 0, &mut record);
        let full = elements.len() as u64;
        assert_eq!(array_push_one(&mut s, open, full, Value::integer(9).to_bits(), ()), None, "no room");
        assert_eq!(elements.len(), 1);
    }

    #[test]
    fn pop_answers_undefined_when_empty_the_last_value_and_leaves_at_a_hole() {
        let mut s = stage();
        let mut elements = ArrayElements::from_slice(&[Value::integer(1), Value::number(0.5), Value::DELETED]);
        let mut record = record();
        let (backing, _) = backing_of(&mut elements, 0, 0, &mut record);
        assert_eq!(array_pop(&mut s, backing, ()), None, "a hole leaves");
        assert_eq!(elements.len(), 3);
        let mut elements = ArrayElements::from_slice(&[Value::integer(1), Value::number(0.5)]);
        let (backing, _) = backing_of(&mut elements, 0, 0, &mut record);
        assert_eq!(array_pop(&mut s, backing, ()), Some(Value::number(0.5).to_bits()));
        assert_eq!(elements.len(), 1);
        let others = s.load_u32(Region::Elements.mutable(), backing.values, crate::heap::OTHERS_FROM_VALUES);
        assert_eq!(others, 0, "the double left the count of others");
        let (backing, _) = backing_of(&mut elements, 0, 0, &mut record);
        assert_eq!(array_pop(&mut s, backing, ()), Some(Value::integer(1).to_bits()));
        let (backing, _) = backing_of(&mut elements, 0, 0, &mut record);
        assert_eq!(array_pop(&mut s, backing, ()), Some(Value::UNDEFINED.to_bits()), "empty");
        assert_eq!(elements.len(), 0);
    }

    #[test]
    fn pop_on_a_frozen_backing_leaves_it_alone() {
        let mut elements = ArrayElements::from_slice(&[Value::integer(1)]);
        let mut record = record();
        let (backing, _) = backing_of(&mut elements, 0, ARRAY.frozen_mask, &mut record);
        assert_eq!(array_pop(&mut stage(), backing, ()), None);
        assert_eq!(elements.len(), 1);
    }

    #[test]
    fn a_store_of_a_reference_is_left_to_the_logging_heap() {
        let mut s = stage();
        for value in [Value::integer(1), Value::number(0.5), Value::TRUE, Value::UNDEFINED, Value::NULL] {
            assert_eq!(unreferenced(&mut s, value.to_bits(), ()), Some(()), "{:#x}", value.to_bits());
        }
        for value in [Value::direct_object(1), Value::direct_array(1), Value::direct_string(1)] {
            assert_eq!(unreferenced(&mut s, value.to_bits(), ()), None, "{:#x}", value.to_bits());
        }
    }

    #[test]
    fn push_on_a_non_extensible_backing_leaves_for_the_generic_path() {
        let mut elements = ArrayElements::from_slice(&[Value::integer(1)]);
        let mut record = record();
        let (backing, capacity) = backing_of(&mut elements, 2, crate::value_vec::VALUE_VEC_INTEGRITY_BITS & !ARRAY.frozen_mask, &mut record);
        assert_eq!(array_push_one(&mut stage(), backing, capacity, Value::integer(9).to_bits(), ()), None);
        assert_eq!(array_pop(&mut stage(), backing, ()), None);
        assert_eq!(elements.len(), 1, "nothing was written");
    }

    #[test]
    fn is_integral_takes_integers_and_leaves_fractions_nan_and_infinities() {
        for (value, expected) in [
            (Value::integer(0), true), (Value::integer(-7), true), (Value::number(4.0), true),
            (Value::number(-0.0), true), (Value::number(9007199254740992.0), true),
            (Value::number(1.5), false), (Value::number(f64::NAN), false),
            (Value::number(f64::INFINITY), false), (Value::number(f64::NEG_INFINITY), false),
            (Value::TRUE, false), (Value::UNDEFINED, false), (Value::NULL, false),
        ] {
            assert_eq!(concrete_is_integral(value), expected, "{:#x}", value.to_bits());
        }
    }
}
