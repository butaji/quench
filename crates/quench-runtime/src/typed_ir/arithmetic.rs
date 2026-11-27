//! The integer-range rules of an arithmetic operator over its operands' ranges: the one home of what range a Number
//! operation defines, read by the facts solver (`compile::aot_facts::numeric`) and by the lane's consumer queries.
//! The operator is decoded by the caller; this file maps the operator to `IntRange`'s rules. It sits beside `Type`
//! and outside the stencil's include list, since it names the operator.

use super::IntRange;
use crate::vm::operations::NumberArithmetic;

/// The exact range of `operation` over the integer ranges `left` and `right`: `add`, `subtract` or `multiply`, which
/// may leave int32. `None` for another operator or where the range leaves `i64`.
pub fn exact(operation: NumberArithmetic, left: IntRange, right: IntRange) -> Option<IntRange> {
    match operation {
        NumberArithmetic::Add => left.add(right),
        NumberArithmetic::Subtract => left.subtract(right),
        NumberArithmetic::Multiply => left.multiply(right),
        _ => None,
    }
}

/// Whether a product of `left` and `right` may be a negative zero, which no integer type holds.
fn may_be_negative_zero(operation: NumberArithmetic, left: IntRange, right: IntRange) -> bool {
    operation == NumberArithmetic::Multiply && left.product_may_be_negative_zero(right)
}

/// The integer range of the Number `operation` defines over `left` and `right`, where a word holds it and it is no
/// negative zero (`IntRange::as_number`). `None` where it has no integer range.
pub fn number_range(operation: NumberArithmetic, left: IntRange, right: IntRange) -> Option<IntRange> {
    if may_be_negative_zero(operation, left, right) { return None; }
    exact(operation, left, right)?.as_number()
}

/// The integer range of a result that leaves int32 and is an integer a word holds, which an index is computed in. A
/// `signless` result (no consumer reads the sign of its zero) may be a negative zero, which the word holds as zero.
pub fn index_range(operation: NumberArithmetic, left: IntRange, right: IntRange, signless: bool) -> Option<IntRange> {
    let range = if signless { signless_range(operation, left, right) } else { number_range(operation, left, right) };
    range.filter(|range| !range.fits_int32())
}

/// The range of a product whose sign of zero no consumer reads: the product's word range, however it may be a
/// negative zero. `None` for any other operator.
pub fn signless_range(operation: NumberArithmetic, left: IntRange, right: IntRange) -> Option<IntRange> {
    (operation == NumberArithmetic::Multiply).then(|| exact(operation, left, right)?.as_number()).flatten()
}

/// Whether the result of `operation` over `left` and `right` is no negative zero.
pub fn zero_free(operation: NumberArithmetic, left: IntRange, right: IntRange) -> bool {
    !may_be_negative_zero(operation, left, right)
}
