//! The `typeof` names of a value word: the name indexes, the typeof classes and tests.

use super::*;

/// The index in `bytecode::TYPEOF_NAMES` of a name some value's `typeof` is.
pub(super) const fn typeof_name_index(name: &str) -> u64 {
    match name_index(name) {
        Some(index) => index as u64,
        None => panic!("a typeof name"),
    }
}

/// The indexes in `bytecode::TYPEOF_NAMES` of the names a value's tag alone decides: `typeof` of
/// `undefined`, of `null` (an object) and a boolean and a Number.
pub const TYPEOF_UNDEFINED: u64 = typeof_name_index("undefined");

pub const TYPEOF_OBJECT: u64 = typeof_name_index("object");

pub const TYPEOF_BOOLEAN: u64 = typeof_name_index("boolean");

pub const TYPEOF_NUMBER: u64 = typeof_name_index("number");

/// The index in `bytecode::TYPEOF_NAMES` of the one name `typeof` yields for every value of the language
/// set `languages` (the `language` bits), where the set decides one: a set of functions is `function`, a
/// set of Numbers is `number`, and a set of direct objects and arrays (or none) is `object`. A set that
/// may name either (a generic object, or a mix of kinds) has none, so a fold built on it leaves at its test.
pub const fn language_typeof_index(languages: u16) -> Option<u64> {
    if languages == language::FUNCTION {
        Some(typeof_name_index("function"))
    } else if languages == language::NUMBER {
        Some(TYPEOF_NUMBER)
    } else if languages & !(language::OBJECT_PLAIN | language::ARRAY) == 0 {
        Some(TYPEOF_OBJECT)
    } else {
        None
    }
}

/// The index in `bytecode::TYPEOF_NAMES` of `typeof` of every value of type `ty`, where the one language set `ty` lies in
/// decides it (`language_typeof_index`): a Number, a function, or a direct object or array. `None` for `Type::NEVER` and
/// for a type no such set holds. The Number-only fold is `typeof_of(Type::NUMBER)`.
#[cfg_attr(stencil_build, allow(dead_code))]
pub fn typeof_of(ty: crate::typed_ir::Type) -> Option<u64> {
    use crate::typed_ir::Type;
    if ty == Type::NEVER {
        return None;
    }
    [language::NUMBER, language::FUNCTION, language::OBJECT_PLAIN | language::ARRAY].into_iter()
        .find(|&set| ty.only_languages(Type::languages(set)))
        .and_then(language_typeof_index)
}

/// Whether `typeof` of every value of type `ty` is the name at `index` of `bytecode::TYPEOF_NAMES`, negated where `negated`
/// (a `TypeofTest`); `None` where `typeof_of` does not decide the name.
#[cfg_attr(stencil_build, allow(dead_code))]
pub fn typeof_test_of(ty: crate::typed_ir::Type, index: u64, negated: bool) -> Option<bool> {
    typeof_of(ty).map(|found| (found == index) != negated)
}

/// The tag classes `typeof` reads from a value's own word: a Number (int32 or double), a boolean,
/// and `null`. Every `typeof` row selects its name from these.
#[inline(always)]
pub fn typeof_classes<S: Stage>(stage: &mut S, bits: S::Word) -> (S::Cond, S::Cond, S::Cond) {
    let tag = stage.and_imm(bits, ValueLayout::TAG_MASK);
    let integer = stage.eq_imm(tag, ValueLayout::INT_TAG);
    let double = is_double(stage, bits);
    let number = stage.any(integer, double);
    let boolean = stage.eq_imm(tag, crate::Value::TRUE.to_bits() & ValueLayout::TAG_MASK);
    let null = stage.eq_imm(bits, crate::Value::NULL.to_bits());
    (number, boolean, null)
}

/// The kind a heap value's `typeof` name follows from its cell (a string, a BigInt, a symbol, an
/// object or a function), as its index in `bytecode::TYPEOF_NAMES`: what `Frame::type_of` proves.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TypeofKind(u64);

impl TypeofKind {
    /// The kind whose `typeof` name is `bytecode::TYPEOF_NAMES[index]`.
    pub const fn of_index(index: u64) -> Self {
        Self(index)
    }

    /// The index in `bytecode::TYPEOF_NAMES` of this kind's name.
    pub const fn index(self) -> u64 {
        self.0
    }
}

/// The index in `bytecode::TYPEOF_NAMES` of `typeof` of the value `bits` where its tag alone decides
/// it (`undefined` and the deleted marker, `null`, a boolean, an int32 or a double): a heap value,
/// whose cell decides (a string, BigInt, symbol, object or function), or an internal marker leaves
/// at `miss`.
#[inline(always)]
pub fn immediate_typeof_index<S: Stage>(stage: &mut S, bits: S::Word, miss: S::Miss) -> Option<S::Word> {
    // A Number is the first test: its arm runs no boolean, `null`, `undefined` or deleted compare. The
    // other arm is the same ladder as before, its guard left at `miss` where no class decides the word.
    let tag = stage.and_imm(bits, ValueLayout::TAG_MASK);
    let integer = stage.eq_imm(tag, ValueLayout::INT_TAG);
    let double = is_double(stage, bits);
    let number = stage.any(integer, double);
    stage.choose_leaving(number, #[inline(always)] |stage| Some(stage.constant(TYPEOF_NUMBER)), #[inline(always)] |stage| {
        let boolean = stage.eq_imm(tag, crate::Value::TRUE.to_bits() & ValueLayout::TAG_MASK);
        let null = stage.eq_imm(bits, crate::Value::NULL.to_bits());
        // Spelled out, not as array `map`s: the interpreter's row ran each closure as a call.
        let undefined_value = stage.eq_imm(bits, crate::Value::UNDEFINED.to_bits());
        let deleted = stage.eq_imm(bits, crate::Value::DELETED.to_bits());
        let decided = stage.any(boolean, null);
        let decided = stage.any(decided, undefined_value);
        let decided = stage.any(decided, deleted);
        stage.guard(decided, miss)?;
        let undefined = stage.constant(TYPEOF_UNDEFINED);
        let object = stage.constant(TYPEOF_OBJECT);
        let boolean_name = stage.constant(TYPEOF_BOOLEAN);
        let word = stage.select(boolean, boolean_name, undefined);
        Some(stage.select(null, object, word))
    })
}

/// The `typeof` result string of the value `bits` where its tag alone decides it: the entry of `names`,
/// the address of the table of the eight result strings (`Vm::typeof_names`, interned when the heap is
/// made and indexed as `bytecode::TYPEOF_NAMES`), at the value's index; a heap value leaves at `miss`.
#[inline(always)]
pub fn immediate_typeof_string<S: Stage>(stage: &mut S, bits: S::Word, names: S::Word, miss: S::Miss) -> Option<S::Word> {
    let index = immediate_typeof_index(stage, bits, miss)?;
    let offset = stage.mul_imm(index, core::mem::size_of::<crate::Value>() as u64);
    let entry = stage.add(names, offset);
    Some(stage.load_table(Region::HeapMeta.readonly(), entry))
}

#[cfg(not(stencil_build))]
/// The interpreter's `immediate_typeof_index` of `value`.
#[inline(always)]
pub fn concrete_immediate_typeof_index(value: crate::Value) -> Option<usize> {
    let mut stage = crate::staged::Concrete::pure();
    immediate_typeof_index(&mut stage, value.to_bits(), ()).map(|index| index as usize)
}
