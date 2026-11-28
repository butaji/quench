//! The tag and family tests of a value word: the heap family, the `TagOps` tests over any stage, and the int and heap classes.

use super::*;

/// The family field is the word's top bits, so a family test is one shift and a compare with
/// a small immediate, with no 64-bit mask or tag to load.
const FAMILY_SHIFT: u32 = ValueLayout::HEAP_FAMILY_MASK.trailing_zeros();

const _: () = assert!(ValueLayout::HEAP_FAMILY_MASK == u64::MAX << FAMILY_SHIFT);

#[inline(always)]
pub(super) fn family_is<S: Stage>(stage: &mut S, bits: S::Word, tag: u64) -> S::Cond {
    let family = stage.shr_imm(bits, FAMILY_SHIFT);
    stage.eq_imm(family, tag >> FAMILY_SHIFT)
}

/// Whether `bits` is a direct object reference, whose low half names its record
/// (`Value::as_direct_object`).
#[inline(always)]
pub fn is_direct_object<S: Stage>(stage: &mut S, bits: S::Word) -> S::Cond {
    use crate::typed_ir::Type;
    if let Some(known) = decided(stage, bits, Type::is_direct_object_family,
        |fact| fact.may_be_languages(Type::language(crate::ValueType::Object))) { return known; }
    family_is(stage, bits, ValueLayout::DIRECT_OBJECT_TAG)
}

/// The id (granule) of the direct object a reference names, its low half (`Value::direct_object_index`).
/// Every object-id read goes through here, so R4-B2b changes this one body.
#[inline(always)]
pub fn direct_object_id<S: Stage>(stage: &mut S, bits: S::Word) -> S::Word {
    stage.and_imm(bits, u64::from(u32::MAX))
}

/// The immediate operations a tag test is written in. A `Stage` is one (its words and conditions);
/// a `MachineScalar` is one through `MachineTags`, whose comparisons are 0-or-1 flag words. Each tag
/// test is written once over this, so a layout change is one definition for every stage and machine.
pub trait TagOps {
    type Word: Copy;
    /// A comparison's result: a condition at a stage, a flag word at a machine.
    type Flag: Copy;
    fn and_imm(&mut self, a: Self::Word, imm: u64) -> Self::Word;
    fn shr_imm(&mut self, a: Self::Word, shift: u64) -> Self::Word;
    fn add_imm(&mut self, a: Self::Word, imm: u64) -> Self::Word;
    fn eq_imm(&mut self, a: Self::Word, imm: u64) -> Self::Flag;
    fn ugt_imm(&mut self, a: Self::Word, imm: u64) -> Self::Flag;
    /// The fact of `bits` (`Stage::fact`); a machine keeps none.
    #[inline(always)]
    fn fact_of(&self, bits: Self::Word) -> crate::typed_ir::Type { let _ = bits; crate::typed_ir::Type::ANY }
    /// The test result that is `value` at every run (`Stage::cond_of`).
    fn flag_of(&mut self, value: bool) -> Self::Flag;
}

/// What the fact of `bits` decides of a class test: `Some(true)` where every value of the fact is in
/// the class (`within`), `Some(false)` where none may be (`possible` false), else `None` and the test
/// is built. The one place a row's class test asks the operand's type (R2).
#[inline(always)]
pub fn decided<T: TagOps>(ops: &mut T, bits: T::Word, within: impl FnOnce(crate::typed_ir::Type) -> bool,
    possible: impl FnOnce(crate::typed_ir::Type) -> bool) -> Option<T::Flag> {
    let fact = ops.fact_of(bits);
    if within(fact) { Some(ops.flag_of(true)) } else if !possible(fact) { Some(ops.flag_of(false)) } else { None }
}

/// Every stage is a `TagOps` through its own immediate operations: a `u64` operand converts to the
/// stage's `Imm` (`From<u64>`), so the stage receives the same constant it received before.
impl<S: Stage> TagOps for S {
    type Word = S::Word;
    type Flag = S::Cond;
    #[inline(always)]
    fn and_imm(&mut self, a: S::Word, imm: u64) -> S::Word {
        Stage::and_imm(self, a, imm)
    }
    #[inline(always)]
    fn shr_imm(&mut self, a: S::Word, shift: u64) -> S::Word {
        Stage::shr_imm(self, a, shift)
    }
    #[inline(always)]
    fn add_imm(&mut self, a: S::Word, imm: u64) -> S::Word {
        Stage::add_imm(self, a, imm)
    }
    #[inline(always)]
    fn eq_imm(&mut self, a: S::Word, imm: u64) -> S::Cond {
        Stage::eq_imm(self, a, imm)
    }
    #[inline(always)]
    fn ugt_imm(&mut self, a: S::Word, imm: u64) -> S::Cond {
        Stage::ugt_imm(self, a, imm)
    }
    #[inline(always)]
    fn fact_of(&self, bits: S::Word) -> crate::typed_ir::Type { Stage::fact(self, bits) }
    #[inline(always)]
    fn flag_of(&mut self, value: bool) -> S::Cond { Stage::cond_of(self, value) }
}

/// A `MachineScalar` read as a `TagOps`: its words are its words, and a comparison is its flag word.
/// The immediate is the `i64` with the same bits as the `u64` operand.
pub(super) struct MachineTags<'a, M>(pub(crate) &'a mut M);

impl<M: MachineScalar> TagOps for MachineTags<'_, M> {
    type Word = M::Word;
    type Flag = M::Word;
    #[inline(always)]
    fn and_imm(&mut self, a: M::Word, imm: u64) -> M::Word {
        self.0.raw_binary_imm(Binary::And, a, imm as i64)
    }
    #[inline(always)]
    fn shr_imm(&mut self, a: M::Word, shift: u64) -> M::Word {
        self.0.raw_binary_imm(Binary::ShiftRightUnsigned, a, shift as i64)
    }
    #[inline(always)]
    fn add_imm(&mut self, a: M::Word, imm: u64) -> M::Word {
        self.0.raw_binary_imm(Binary::Add, a, imm as i64)
    }
    #[inline(always)]
    fn eq_imm(&mut self, a: M::Word, imm: u64) -> M::Word {
        self.0.raw_compare_imm(Relation::Equal, a, imm as i64)
    }
    #[inline(always)]
    fn ugt_imm(&mut self, a: M::Word, imm: u64) -> M::Word {
        self.0.raw_compare_imm(Relation::UnsignedGreater, a, imm as i64)
    }
    #[inline(always)]
    fn flag_of(&mut self, value: bool) -> M::Word { self.0.raw_const(Lane::I64, i64::from(value)) }
}

/// Whether `bits` is an int32 (`Value::as_int`); an int32 has one tagged form, so two
/// distinct int-tagged words are two distinct values. At a machine it is a flag word.
#[inline(always)]
pub fn is_int<T: TagOps>(ops: &mut T, bits: T::Word) -> T::Flag {
    use crate::typed_ir::Type;
    if let Some(known) = decided(ops, bits, Type::is_int, |fact| fact.may_be_languages(Type::NUMBER)) { return known; }
    let tag = ops.and_imm(bits, ValueLayout::TAG_MASK);
    ops.eq_imm(tag, ValueLayout::INT_TAG)
}

/// Whether `bits` is a heap reference (`Value::is_heap`).
#[inline(always)]
pub fn is_heap<S: Stage>(stage: &mut S, bits: S::Word) -> S::Cond {
    let tag = stage.and_imm(bits, ValueLayout::TAG_MASK);
    stage.eq_imm(tag, ValueLayout::HEAP_TAG)
}
