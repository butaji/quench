//! The `Unary` operators past oxc's numbering: a template part's string primitive and the
//! `typeof` tests and index, which no source operator spells.

/// The `Unary` operator a template literal substitution compiles to when a later one follows:
/// the substitution itself where ToString of it runs no guest code and raises nothing, else
/// its ToString (ECMA-262 13.15.5.1 step 5), so every conversion that can be observed runs in
/// source order and `StringConcat` converts only what cannot be. No source operator spells
/// it; it extends oxc's operator numbering.
pub const UNARY_TO_STRING_PRIMITIVE: u32 = oxc_ast::ast::UnaryOperator::Delete as u32 + 1;
pub use super::typeof_names::{TYPEOF_NAMES, typeof_name_index};
/// The `Unary` operators that compare `typeof` of their operand with a name in
/// `TYPEOF_NAMES` (`typeof x === "function"`): this plus the name's index tests equality,
/// and that plus `TYPEOF_NAMES.len()` inequality. The comparison of two strings that
/// `typeof` and a literal would make has no effects, so it needs neither string.
pub(crate) const UNARY_TYPEOF_IS: u32 = UNARY_TO_STRING_PRIMITIVE + 1;
const UNARY_TYPEOF_TESTS: u32 = 2 * TYPEOF_NAMES.len() as u32;

/// The `UNARY_TYPEOF_IS` operator testing `typeof x` against `name`, negated for an
/// inequality; `None` when no value's `typeof` is `name`.
pub(crate) fn typeof_test_operator(name: &str, negated: bool) -> Option<u32> {
    let index = typeof_name_index(name)? as u32;
    Some(UNARY_TYPEOF_IS + index + if negated { TYPEOF_NAMES.len() as u32 } else { 0 })
}

/// The `Unary` operator giving the index in `TYPEOF_NAMES` of the name `typeof` yields for
/// its operand, as an integer: `switch (typeof x)` with string cases compares it with each
/// case's index (`FunctionCompiler::switch_statement`), as `typeof x === "name"` would
/// compare the two strings, and like that comparison it needs neither string.
pub const UNARY_TYPEOF_INDEX: u32 = UNARY_TYPEOF_IS + UNARY_TYPEOF_TESTS;
/// The `Unary` operator an operand of a `"text" + a + b` chain compiles to
/// (`FunctionCompiler::string_chain`): the operand itself where its conversion runs no guest
/// code and raises nothing, else ToString of its ToPrimitive with the default hint, which
/// is what the chain's `+` of that operand does, in the order the `+`s run.
pub const UNARY_ADD_OPERAND_STRING: u32 = UNARY_TYPEOF_INDEX + 1;

/// The `Unary` operators testing whether an array pattern of `n` elements and no rest reads
/// its elements by index with no observable iteration step (`FunctionCompiler::direct_array_pattern`):
/// this plus `n - 1`, for `n` up to `ARRAY_PATTERN_MAX_ELEMENTS` (`Vm::array_pattern_fits`).
pub(crate) const UNARY_ARRAY_PATTERN: u32 = UNARY_ADD_OPERAND_STRING + 1;
pub(crate) const ARRAY_PATTERN_MAX_ELEMENTS: u32 = 16;

/// The element count a `UNARY_ARRAY_PATTERN` operator tests.
#[inline(always)]
pub fn array_pattern_test(operator: u32) -> Option<usize> {
    let offset = operator.checked_sub(UNARY_ARRAY_PATTERN).filter(|offset| *offset < ARRAY_PATTERN_MAX_ELEMENTS)?;
    Some(offset as usize + 1)
}

/// The name index and negation a `UNARY_TYPEOF_IS` operator tests.
#[inline(always)]
pub fn typeof_test(operator: u32) -> Option<(usize, bool)> {
    let offset = operator.checked_sub(UNARY_TYPEOF_IS).filter(|offset| *offset < UNARY_TYPEOF_TESTS)?;
    let names = TYPEOF_NAMES.len() as u32;
    Some(((offset % names) as usize, offset >= names))
}
