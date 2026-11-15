//! Int32 facts of a counted loop, shared by every tier that keeps its counter an int32: the
//! lane specializer's range analysis, the OSR entry observation and the interpreter's proved
//! regions (`vm::dispatch_fast::region_state`). One authority for what a loop test and an
//! entry observation prove.

use super::IntRange;
use crate::bytecode::Register;
use crate::value::ValueLayout;
use crate::vm::operations::NumberRelation;
use crate::Value;

/// The relation an edge proves of its operands: `relation` where `truth`, else its negation.
pub(crate) fn effective_relation(relation: NumberRelation, truth: bool) -> NumberRelation {
    use NumberRelation::*;
    if truth { return relation; }
    match relation {
        Equal => NotEqual, NotEqual => Equal, LessThan => GreaterEqual,
        LessEqual => GreaterThan, GreaterThan => LessEqual, GreaterEqual => LessThan,
    }
}

pub(crate) fn relation_ranges(relation: NumberRelation, truth: bool, left: IntRange, right: IntRange) -> Option<(IntRange, IntRange)> {
    use NumberRelation::*;
    let relation = effective_relation(relation, truth);
    match relation {
        Equal => { let common = left.intersect(right)?; Some((common, common)) }
        NotEqual => (left != right || left.min != left.max).then_some((left, right)),
        GreaterThan | GreaterEqual => relation_ranges(
            if relation == GreaterThan { LessThan } else { LessEqual }, true, right, left).map(|(right, left)| (left, right)),
        LessThan | LessEqual => {
            let strict = i64::from(relation == LessThan);
            Some((IntRange::bounds(i64::from(left.min), i64::from(left.max).min(i64::from(right.max) - strict))?,
                IntRange::bounds(i64::from(right.min).max(i64::from(left.min) + strict), i64::from(right.max))?))
        }
    }
}

/// The factors under which an int32 times a constant, and that product's sum with an int32,
/// are exact binary64 integers: the 53-bit mantissa less the int32's 32 bits.
pub(crate) const EXACT_FACTOR_BOUND: u32 = 1 << (f64::MANTISSA_DIGITS - i32::BITS);

/// Whether `constant` is a factor of the truncation island (`staged::numeric::scaled_sum`).
pub(crate) const fn exact_factor(constant: i32) -> bool { constant.unsigned_abs() < EXACT_FACTOR_BOUND }

/// Whether the counter step of a loop whose test is `counter relation limit` cannot overflow
/// where the test holds: the update `counter + 1` (`counter - 1` where `decrement`) of an int32
/// counter the test bounds stays an int32, so the step needs no overflow exit.
pub(crate) fn step_is_exact(relation: NumberRelation, decrement: bool) -> bool {
    let one = IntRange::singleton(1);
    relation_ranges(relation, true, IntRange::FULL, IntRange::FULL).is_some_and(|(counter, _)| {
        if decrement { counter.subtract(one) } else { counter.add(one) }.is_some()
    })
}

/// The registers among `registers` that hold int32s, each with the range its value gives an
/// entry (`IntRange::entering`): what a compiled region's entry contract checks.
pub(crate) fn entering(registers: impl Iterator<Item = Register>, read: impl Fn(Register) -> Value) -> Vec<(Register, IntRange)> {
    registers.filter_map(|register| read(register).as_int().map(|value| (register, IntRange::entering(value)))).collect()
}

/// Whether every one of `values` is an int32: the entry fact a region keeps its registers as
/// int32 words on, proved once at the region's entry, in one test of the tags' difference.
#[inline(always)]
pub(crate) fn all_int(values: impl Iterator<Item = Value>) -> bool {
    values.fold(0, |difference, value| difference | (value.to_bits() ^ ValueLayout::INT_TAG)) & ValueLayout::TAG_MASK == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Whether `relation` holds of the two ints.
    fn truth(relation: NumberRelation, x: i32, y: i32) -> bool {
        use NumberRelation::*;
        match relation { Equal => x == y, NotEqual => x != y, LessThan => x < y, LessEqual => x <= y, GreaterThan => x > y, GreaterEqual => x >= y }
    }

    /// Every pair of intervals within -3..=3.
    fn interval_pairs() -> impl Iterator<Item = (IntRange, IntRange)> {
        let intervals = || (-3..=3).flat_map(|min| (min..=3).map(move |max| IntRange { min, max }));
        intervals().flat_map(move |left| intervals().map(move |right| (left, right)))
    }

    #[test]
    fn relation_edges_overapproximate_all_signed_integer_outcomes() {
        use NumberRelation::*;
        for (left, right) in interval_pairs() {
            for relation in [Equal, NotEqual, LessThan, LessEqual, GreaterThan, GreaterEqual] {
                for edge in [false, true] {
                    let refined = relation_ranges(relation, edge, left, right);
                    for (x, y) in (left.min..=left.max).flat_map(|x| (right.min..=right.max).map(move |y| (x, y))) {
                        if truth(relation, x, y) == edge {
                            let (left, right) = refined.expect("reachable edge");
                            assert!(left.contains(IntRange::singleton(x)) && right.contains(IntRange::singleton(y)));
                        }
                    }
                }
            }
        }
        assert!(relation_ranges(LessThan, true, IntRange::singleton(i32::MIN), IntRange::singleton(i32::MIN)).is_none());
        assert!(relation_ranges(GreaterThan, true, IntRange::singleton(i32::MAX), IntRange::singleton(i32::MAX)).is_none());
    }

    #[test]
    fn counted_steps_are_exact_only_for_strict_relations_in_their_direction() {
        use NumberRelation::*;
        assert!(step_is_exact(LessThan, false));
        assert!(step_is_exact(GreaterThan, true));
        assert!(!step_is_exact(LessEqual, false));
        assert!(!step_is_exact(GreaterEqual, true));
        assert!(!step_is_exact(LessThan, true));
        assert!(!step_is_exact(GreaterThan, false));
    }

    #[test]
    fn entry_ints_are_proved_by_one_tag_difference() {
        let ints = [Value::integer(i32::MIN), Value::integer(-1), Value::integer(i32::MAX)];
        assert!(all_int(ints.into_iter()));
        for other in [Value::number(1.5), Value::UNDEFINED, Value::NULL, Value::boolean(true), Value::number(f64::NAN)] {
            assert!(!all_int(ints.into_iter().chain([other])), "{other:?}");
        }
        assert!(all_int(std::iter::empty()));
    }
}
