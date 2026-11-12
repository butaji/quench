//! `typeof`'s name index (`UNARY_TYPEOF_INDEX`) as a row: a total function over the value's tag.
//! A register whose type fact names the answer folds to a constant (Static); any other
//! non-heap word is decided by its tag; a heap value's cell decides, so the interpreter's frame
//! calls out (`CallOut::Typeof`) and a specializer leaves at the miss edge.

use super::callout::calling;
use super::value::is_double;
use super::*;
use crate::value::ValueLayout;
use crate::ValueType;

/// The kinds whose name a type fact alone decides, in the order a fact is tried.
const FOLDED_KINDS: [ValueType; 7] = [ValueType::Number, ValueType::Boolean, ValueType::Null,
    ValueType::Undefined, ValueType::String, ValueType::BigInt, ValueType::Symbol];

/// The kind `ty` is a subtype of, if it is exactly one of the folded kinds.
fn folded_kind(ty: Type) -> Option<ValueType> {
    FOLDED_KINDS.into_iter().find(|kind| ty != Type::NEVER && ty.is_subtype(Type::language(*kind)))
}

/// The name index of `kind` in the frame's representation of an int32 constant.
#[inline(always)]
fn name<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, kind: ValueType) -> (S::Word, F::Fact) {
    let index = crate::vm::typeof_name_index(kind) as i32;
    frame.constant(stage, Constant { value: crate::Value::integer(index), ty: Type::INT32 })
}

/// Whether `bits` is a heap value, and the name index its tag gives any other value: `undefined`
/// for the remaining immediates (and the deleted marker), whose name it is.
#[inline(always)]
fn by_tag<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, bits: S::Word) -> (S::Cond, S::Word) {
    let tag = stage.and_imm(bits, ValueLayout::TAG_MASK);
    let heap = stage.eq_imm(tag, ValueLayout::HEAP_TAG);
    let double = is_double(stage, bits);
    let integer = stage.eq_imm(tag, ValueLayout::INT_TAG);
    let number = stage.any(double, integer);
    let boolean = stage.eq_imm(tag, crate::Value::TRUE.to_bits() & ValueLayout::TAG_MASK);
    let null = stage.eq_imm(bits, crate::Value::NULL.to_bits());
    let (mut index, _) = name(stage, frame, ValueType::Undefined);
    for (holds, kind) in [(null, ValueType::Null), (boolean, ValueType::Boolean), (number, ValueType::Number)] {
        let (named, _) = name(stage, frame, kind);
        index = stage.select(holds, named, index);
    }
    (heap, index)
}

/// `typeof source`'s name index into `destination`: a constant where the type fact decides it,
/// else by the tag; a heap value is the VM's (`CallOut::Typeof`) where the stage knows it is
/// one, and leaves at the miss edge where it cannot.
#[inline(always)]
pub(super) fn step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, destination: Register,
    source: Register) -> Step<S::Word, F::Control> {
    if let Some(kind) = folded_kind(frame.value_fact(source)) {
        let (index, fact) = name(stage, frame, kind);
        frame.write(stage, destination, index, fact);
        return Step::Value(index);
    }
    let bits = frame.tagged(stage, source);
    let (heap, index) = by_tag(stage, frame, bits);
    if stage.known(heap) == Some(true) {
        return calling(stage, frame, CallOut::Typeof { source }, Some(destination));
    }
    let miss = frame.miss();
    if stage.guard_false(heap, miss).is_none() { return Step::Unresolved; }
    let fact = frame.int_fact();
    frame.write(stage, destination, index, fact);
    Step::Value(index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::TYPEOF_NAMES;
    use crate::staged::tests::ConcreteFrame;
    use crate::Value;

    fn typeof_row(value: Value) -> (Step<u64, crate::staged::Edge>, u64) {
        let mut frame = ConcreteFrame { registers: vec![value.to_bits(), 0], heap: None };
        let step = crate::staged::step(&mut Concrete::new(std::ptr::null()), &mut frame,
            Operation::<u64, ()>::Typeof { destination: 1, source: 0 });
        (step, frame.registers[1])
    }

    /// The tag decides every immediate's name, written to the destination as an int32.
    #[test]
    fn the_tag_names_every_value_outside_the_heap() {
        for (value, name) in [(Value::number(3.5), "number"), (Value::integer(3), "number"), (Value::number(f64::NAN), "number"),
            (Value::TRUE, "boolean"), (Value::FALSE, "boolean"), (Value::NULL, "object"),
            (Value::UNDEFINED, "undefined"), (Value::DELETED, "undefined")] {
            let (step, written) = typeof_row(value);
            assert!(matches!(step, Step::Value(_)), "{value:?}");
            let index = Value::from_bits(written).as_int().expect("an int32 index");
            assert_eq!(TYPEOF_NAMES[index as usize], name, "{value:?}");
        }
    }

    /// A heap value's cell decides its name: a frame with no host leaves it, writing nothing.
    #[test]
    fn a_heap_value_is_the_vms_and_writes_nothing_without_a_host() {
        let (step, written) = typeof_row(Value::from_bits(ValueLayout::DIRECT_OBJECT_TAG));
        assert!(matches!(step, Step::Unresolved));
        assert_eq!(written, 0);
    }

    /// A type fact that is exactly one language kind decides the name; any other does not.
    #[test]
    fn a_type_fact_folds_only_where_it_names_one_kind() {
        assert_eq!(folded_kind(Type::NUMBER), Some(ValueType::Number));
        assert_eq!(folded_kind(Type::INT32), Some(ValueType::Number));
        assert_eq!(folded_kind(Type::language(ValueType::String)), Some(ValueType::String));
        assert_eq!(folded_kind(Type::ANY), None);
        assert_eq!(folded_kind(Type::NEVER), None);
    }
}
