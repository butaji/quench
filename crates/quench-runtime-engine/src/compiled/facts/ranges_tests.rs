#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

fn holds(relation: NumberRelation, x: i64, y: i64) -> bool {
    use NumberRelation::*;
    match relation {
        Equal => x == y, NotEqual => x != y, LessThan => x < y,
        LessEqual => x <= y, GreaterThan => x > y, GreaterEqual => x >= y,
    }
}

/// Every pair of values of `left` and `right` that makes `relation` `truth` lies in the ranges the edge gives.
fn edge_encloses(relation: NumberRelation, truth: bool, left: IntRange, right: IntRange) {
    let refined = relation_ranges(relation, truth, left, right);
    for x in left.min..=left.max { for y in right.min..=right.max {
        if holds(relation, x, y) == truth {
            let (left, right) = refined.expect("reachable edge");
            assert!(left.contains(IntRange::singleton(x)) && right.contains(IntRange::singleton(y)));
        }
    }}
}

#[test]
fn relation_edges_overapproximate_all_signed_integer_outcomes() {
    use NumberRelation::*;
    let ranges: Vec<IntRange> = (-3..=3).flat_map(|min| (min..=3).map(move |max| IntRange { min, max })).collect();
    for left in &ranges { for right in &ranges {
        for relation in [Equal, NotEqual, LessThan, LessEqual, GreaterThan, GreaterEqual] {
            for truth in [false, true] { edge_encloses(relation, truth, *left, *right); }
        }
    }}
    assert!(relation_ranges(LessThan, true, IntRange::singleton(i32::MIN), IntRange::singleton(i32::MIN)).is_none());
    assert!(relation_ranges(GreaterThan, true, IntRange::singleton(i32::MAX), IntRange::singleton(i32::MAX)).is_none());
}

#[test]
fn only_a_product_that_may_be_negative_zero_loses_its_integer_type() {
    let range = |min, max| IntRange { min, max };
    let multiply = NumberArithmetic::Multiply;
    assert!(negative_zero(multiply, range(-3, 2), range(0, 5)));
    assert!(negative_zero(multiply, range(0, 5), range(-3, 2)));
    assert!(!negative_zero(multiply, range(1, 5), range(-3, -1)));
    assert!(!negative_zero(multiply, range(0, 5), range(0, 5)));
    assert!(!negative_zero(NumberArithmetic::Add, range(-3, 2), range(0, 5)));
}

#[test]
fn bitwise_operands_outside_int32_or_unranged_are_any_int32() {
    let outside = |operand: Operand| (operand.register_index() == Some(0)).then(|| IntRange { min: 0, max: i64::from(u32::MAX) });
    let operands = [Operand::register(0), Operand::register(1)];
    assert_eq!(bitwise(&outside, NumberBitwise::Or, operands), IntRange::FULL);
    let small = |operand: Operand| (operand.register_index() == Some(0)).then(|| IntRange { min: 0, max: 255 });
    assert_eq!(bitwise(&small, NumberBitwise::And, operands), IntRange { min: 0, max: 255 });
    let zero = |operand: Operand| Some(if operand.register_index() == Some(0) { IntRange { min: 3, max: 9 } } else { IntRange::singleton(0) });
    assert_eq!(bitwise(&zero, NumberBitwise::Or, operands), IntRange { min: 3, max: 9 });
}

#[test]
fn a_sum_or_product_of_int32s_keeps_an_integer_range_within_the_word_span() {
    let range = |min: i64, max: i64| IntRange { min, max };
    let positive = range(0, i64::from(i32::MAX));
    let product = number_range(NumberArithmetic::Multiply, positive, range(1, i64::from(i32::MAX))).expect("a product of int32s");
    assert!(!product.fits_int32() && IntRange::WORD.contains(product));
    let sum = number_range(NumberArithmetic::Add, product, positive).expect("an index: a product and a counter");
    assert!(!sum.fits_int32() && sum.min == 0);
    let negative = range(i64::from(i32::MIN), -1);
    let beyond = number_range(NumberArithmetic::Multiply, negative, negative).expect("the span's own bound is inside it");
    assert_eq!(beyond.max, IntRange::WORD.max);
    assert_eq!(number_range(NumberArithmetic::Add, beyond, IntRange::singleton(1)), Some(IntRange { min: 2, max: IntRange::WORD.max }), "2^62 + 1 rounds to 2^62, inside the span");
    assert_eq!(number_range(NumberArithmetic::Add, beyond, IntRange::singleton(1 << 11)), None, "past the word span there is no range");
    assert_eq!(number_range(NumberArithmetic::Multiply, range(-3, 2), range(0, 5)), None, "a possible negative zero has none");
}
