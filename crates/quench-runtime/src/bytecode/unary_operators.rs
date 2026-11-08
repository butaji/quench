//! The `Unary` operators past oxc's numbering: a template part's string primitive and the
//! `typeof` tests and index, which no source operator spells.

/// The `Unary` operator a template literal substitution compiles to when a later one follows:
/// the substitution itself where ToString of it runs no guest code and raises nothing, else
/// its ToString (ECMA-262 13.15.5.1 step 5), so every conversion that can be observed runs in
/// source order and `StringConcat` converts only what cannot be. No source operator spells
/// it; it extends oxc's operator numbering.
pub(crate) const UNARY_TO_STRING_PRIMITIVE: u32 = oxc_ast::ast::UnaryOperator::Delete as u32 + 1;
/// The names `typeof` yields (ECMA-262 13.5.3.1), indexed as `typeof_name_index` (vm/coercion.rs) classifies
/// values.
pub(crate) const TYPEOF_NAMES: [&str; 8] =
    ["undefined", "object", "boolean", "number", "string", "bigint", "symbol", "function"];
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
pub(crate) const UNARY_TYPEOF_INDEX: u32 = UNARY_TYPEOF_IS + UNARY_TYPEOF_TESTS;

/// The index in `TYPEOF_NAMES` of `name`, if some value's `typeof` is `name`.
pub(crate) fn typeof_name_index(name: &str) -> Option<usize> {
    TYPEOF_NAMES.iter().position(|candidate| *candidate == name)
}

/// The name index and negation a `UNARY_TYPEOF_IS` operator tests.
#[inline(always)]
pub(crate) fn typeof_test(operator: u32) -> Option<(usize, bool)> {
    let offset = operator.checked_sub(UNARY_TYPEOF_IS).filter(|offset| *offset < UNARY_TYPEOF_TESTS)?;
    let names = TYPEOF_NAMES.len() as u32;
    Some(((offset % names) as usize, offset >= names))
}
