//! The declared parameter types a tier checks at entry: one set (`DECLARED_CHECKED`) that the u-jit
//! (`lane_region/scalar.rs`) and the b-jit (`base_region/declared.rs`) both read. A TypeScript
//! annotation is a fact only where some tier's entry code tests it, so a member is in this set when
//! at least one tier tests it, and each tier's own mask names the members its entry code can test
//! (`LANE_TESTED`, `BASE_TESTED`). A declared set is checked by a tier only when every one of its
//! members is in that tier's mask; any other set gets no entry fact in that tier and stays `ANY`.
//!
//! The members are language bits (`typed_ir::language_bit`, the order of `ValueType`), plus the two
//! object-family words the b-jit tags (`ARRAY`, `OBJECT_PLAIN`, `staged::value::language`), which sit
//! past `ValueType`'s discriminants and so name no language a `Type` can hold. Number is in the set
//! but neither mask's tag test covers it: the u-jit proves a Number set exactly (`Type::NUMBER`), and
//! the b-jit proves it through the register's representation (`DeclaredFact::Number`).

#[allow(unused_imports)]
use crate::ext_traits::*;
use quench_runtime::bytecode::Function;
use quench_runtime::typed_ir::language_bit;
use quench_runtime::ValueType;

/// A Number (an int32 or a binary64 word): proved exactly by the u-jit and by representation in the b-jit.
pub(crate) const NUMBER: u16 = language_bit(ValueType::Number);
/// `true` and `false`: one tag test in both tiers.
pub(crate) const BOOLEAN: u16 = language_bit(ValueType::Boolean);
/// `null`: one word test in both tiers.
pub(crate) const NULL: u16 = language_bit(ValueType::Null);
/// `undefined`: one word test in both tiers.
pub(crate) const UNDEFINED: u16 = language_bit(ValueType::Undefined);
/// A direct object family word (the object family's bit, which also admits arrays and plain objects): one tag test in both tiers.
pub(crate) const OBJECT: u16 = language_bit(ValueType::Object);
/// A direct function word: one tag test in both tiers.
pub(crate) const FUNCTION: u16 = language_bit(ValueType::Function);
/// A string: one tag test in both tiers. Every string is a direct string word (`Heap::is_string`), so the
/// family test decides it. The u-jit also takes any other string by the VM's `typeof` on a slow path,
/// which no string needs today.
pub(crate) const STRING: u16 = language_bit(ValueType::String);
/// A direct array word (`DIRECT_ARRAY_TAG`): one tag test in both tiers. The u-jit names it as the
/// language bit `Type::languages(ARRAY)`, which no `ValueType` names (the bit sits past them).
pub(crate) const ARRAY: u16 = quench_runtime::staged::value::language::ARRAY;
/// A plain direct object word (`DIRECT_OBJECT_TAG`): one tag test in both tiers. The u-jit names it as
/// `Type::languages(OBJECT_PLAIN)`, past `ValueType`'s bits like `ARRAY`.
pub(crate) const OBJECT_PLAIN: u16 = quench_runtime::staged::value::language::OBJECT_PLAIN;

/// The one set of declared language types a tier checks at entry: the union of every member some
/// tier's entry code tests. Both tiers read this set, through their own masks below.
pub(crate) const DECLARED_CHECKED: u16 = NUMBER | BOOLEAN | NULL | UNDEFINED | OBJECT | FUNCTION | STRING | ARRAY | OBJECT_PLAIN;

/// The members of `DECLARED_CHECKED` the u-jit's entry conformance (`lane_region.rs`, `conform_language`)
/// tests exactly: the immediates, the direct object, array, plain and function families, and a string. A
/// Number is not in it: the u-jit proves a Number set through its own exact Number path, not a tag test.
pub(crate) const LANE_TESTED: u16 = BOOLEAN | NULL | UNDEFINED | OBJECT | FUNCTION | STRING | ARRAY | OBJECT_PLAIN;

/// The members of `DECLARED_CHECKED` the b-jit's entry test decides with a tag (`Heap::Declared`,
/// `staged::value::tag_languages`): a Number, the immediates, the direct object, array, plain, function
/// and string families.
pub(crate) const BASE_TESTED: u16 = NUMBER | BOOLEAN | NULL | UNDEFINED | OBJECT | FUNCTION | STRING | ARRAY | OBJECT_PLAIN;

/// The entry check a declared parameter type gives: the one derivation of which parameters a function's
/// entry checks and what each check is. Both tiers read it and map each kind to their own test, or to no
/// test with an explicit arm (`base_region::declared::DeclaredFact::of`). There is no
/// Int32 kind: a declared type never names an int32, and the u-jit picks an int32 entry from what the body
/// requires (the solve's entry cell).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EntryCheck {
    /// A Number: an int32 or a binary64 word.
    Number,
    /// A string: a direct string by its tag (both tiers); the u-jit takes any other by the VM's `typeof`.
    String,
    /// A direct array word.
    Array,
    /// A plain direct object word.
    PlainObject,
    /// Any other set of `DECLARED_CHECKED` members: one test of the set, which each tier admits by its mask.
    Languages(u16),
}

impl EntryCheck {
    /// The check the declared language set `declared` gives, or None where no tier's entry test decides
    /// any of it (a BigInt, a Symbol, an open set, no set at all).
    pub(crate) fn of(declared: u16) -> Option<Self> {
        match declared {
            0 => None,
            NUMBER => Some(Self::Number),
            STRING => Some(Self::String),
            ARRAY => Some(Self::Array),
            OBJECT_PLAIN => Some(Self::PlainObject),
            bits if bits & !DECLARED_CHECKED == 0 => Some(Self::Languages(bits)),
            _ => None,
        }
    }
}

/// Each declared parameter of `function` with its entry check, by parameter index
/// (`Function::declared_parameters`). Derived from the declared bits on each use, never stored: a tier
/// reads it once per lowering, and a parameter the unit writes is listed too (the tier drops it there).
pub(crate) fn entry_checks(function: &Function) -> impl Iterator<Item = (usize, EntryCheck)> + '_ {
    function.declared_parameters().iter().enumerate()
        .filter_map(|(index, declared)| Some((index, EntryCheck::of(*declared)?)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use quench_runtime::typed_ir::ANY_LANGUAGES;

    #[test]
    fn each_declared_set_gets_one_check_kind() {
        assert_eq!(EntryCheck::of(NUMBER), Some(EntryCheck::Number));
        assert_eq!(EntryCheck::of(STRING), Some(EntryCheck::String));
        assert_eq!(EntryCheck::of(ARRAY), Some(EntryCheck::Array));
        assert_eq!(EntryCheck::of(OBJECT_PLAIN), Some(EntryCheck::PlainObject));
        assert_eq!(EntryCheck::of(STRING | UNDEFINED), Some(EntryCheck::Languages(STRING | UNDEFINED)));
        assert_eq!(EntryCheck::of(0), None);
        assert_eq!(EntryCheck::of(ANY_LANGUAGES), None);
    }
}
