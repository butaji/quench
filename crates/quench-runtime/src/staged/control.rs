//! The staged `typeof`: the name index of a value its own word decides, written once for the
//! interpreter and every tier.

use super::*;
use crate::bytecode::TYPEOF_NAMES;
use crate::value::ValueLayout;

/// The index in `TYPEOF_NAMES` of `name`: the one owner of the indices the rows below name.
const fn name_index(name: &str) -> i32 {
    let wanted = name.as_bytes();
    let mut index = 0;
    while index < TYPEOF_NAMES.len() {
        let candidate = TYPEOF_NAMES[index].as_bytes();
        let mut at = 0;
        while at < wanted.len() && at < candidate.len() && wanted[at] == candidate[at] { at += 1; }
        if at == wanted.len() && at == candidate.len() { return index as i32; }
        index += 1;
    }
    panic!("a typeof name")
}

const UNDEFINED_NAME: i32 = name_index("undefined");
const OBJECT_NAME: i32 = name_index("object");
const BOOLEAN_NAME: i32 = name_index("boolean");
const NUMBER_NAME: i32 = name_index("number");

/// The `typeof` name index (int32) of `source`, into `destination`, where the value's own
/// word decides it: a Number, a Boolean, `null`, `undefined` or the deleted marker. A heap
/// value's cell decides its name (a string, a function, the IsHTMLDDA object), so it leaves
/// at the frame's miss edge.
#[inline(always)]
pub(super) fn typeof_index<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    destination: Register, source: Register) -> Step<S::Word, F::Control> {
    let miss = frame.miss();
    let bits = frame.tagged(stage, source);
    let tag = stage.and_imm(bits, ValueLayout::TAG_MASK);
    let heap = stage.eq_imm(tag, ValueLayout::HEAP_TAG);
    if stage.guard_false(heap, miss).is_none() {
        return Step::Unresolved;
    }
    let name = immediate_name(stage, bits, tag);
    let result = frame.int_result(stage, name);
    let fact = frame.int_fact();
    frame.write(stage, destination, result, fact);
    Step::Value(result)
}

/// The name index of the immediate `bits` (of tag `tag`): every immediate that is no Number,
/// Boolean or `null` is `undefined` or the deleted marker, whose name it is.
#[inline(always)]
fn immediate_name<S: NumericStage>(stage: &mut S, bits: S::Word, tag: S::Word) -> S::Word {
    let field = stage.shr_imm(bits, ValueLayout::TAG_SHIFT);
    let offset = stage.add_imm(field, ValueLayout::FIRST_TAG_FIELD.wrapping_neg());
    let double = stage.ugt_imm(offset, ValueLayout::TAG_FIELD_SPAN);
    let integer = stage.eq_imm(tag, ValueLayout::INT_TAG);
    let number = stage.either(double, integer);
    let null = stage.eq_imm(bits, crate::Value::NULL.to_bits());
    let boolean = stage.eq_imm(tag, crate::Value::TRUE.to_bits() & ValueLayout::TAG_MASK);
    let mut name = stage.i32_constant(UNDEFINED_NAME);
    for (holds, index) in [(null, OBJECT_NAME), (boolean, BOOLEAN_NAME), (number, NUMBER_NAME)] {
        let named = stage.i32_constant(index);
        name = stage.select(holds, named, name);
    }
    name
}
