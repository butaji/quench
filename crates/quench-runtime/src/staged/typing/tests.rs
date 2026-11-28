use super::*;
use crate::staged::Overflow;

#[test]
fn string_typing_tracks_length_and_atoms() {
    let text = |atom: u32, len: u32| Type::string_atoms(&[atom], len, len, true);
    let joined = concatenation(text(3, 2), text(4, 1));
    assert_eq!(joined.string_fact().unwrap().length(), StringLength { min: 3, max: 3 });
    assert!(joined.string_fact().unwrap().atoms().is_empty());
    assert!(joined.string_fact().unwrap().flat(), "two flat short operands concatenate flat");
    assert_eq!(to_string(text(3, 2)), text(3, 2));
    assert_eq!(to_string(Type::INT32), Type::language(ValueType::String));
    assert_eq!(char_at(text(3, 1), range(0, 0)), Type::string_length(1, 1, true));
    assert_eq!(char_at(text(3, 1), range(0, 1)).string_fact().unwrap().length(), StringLength { min: 0, max: 1 });
    assert_eq!(char_code_at(text(3, 1), range(0, 0)), range(0, 65_535));
    assert_eq!(char_code_at(text(3, 1), range(0, 1)), Type::NUMBER);
    let exact = substring_length(text(3, 4), range(1, 1), range(3, 3), false);
    assert_eq!(exact.string_fact().unwrap().length(), StringLength { min: 2, max: 2 });
    let slice = substring_length(text(3, 4), range(-3, -3), Type::NUMBER, true);
    assert_eq!(slice.string_fact().unwrap().length().max, 4);
}

#[test]
fn string_equality_is_decided_by_atoms_and_lengths() {
    let text = |atom: u32, len: u32| Type::string_atoms(&[atom], len, len, true);
    assert_eq!(string_equality(text(3, 2), text(3, 2)), Some(true));
    assert_eq!(string_equality(text(3, 2), text(4, 2)), Some(false));
    assert_eq!(string_equality(text(3, 2), text(4, 3)), Some(false));
    assert_eq!(string_equality(Type::language(ValueType::String), text(4, 2)), None);
    assert!(both_interned(text(3, 2), text(4, 2)));
    assert!(!both_interned(text(3, 2), Type::language(ValueType::String)));
    assert_eq!(string_equality(text(3, 2), Type::INT32), None);
}

fn span(min: i64, max: i64) -> IntRange { IntRange::bounds(min, max).unwrap() }

fn range(min: i64, max: i64) -> Type { Type::int(span(min, max)) }

/// Registers 0, 1, ... hold the given cells; no constants, and no elements no field names.
struct Registers(Vec<Cell>);

impl Abstract for Registers {
    fn register(&self, register: Register) -> Cell { self.0[usize::from(register)] }
    fn constant(&self, _index: usize) -> Cell { Cell::ANY }
    fn unnamed_elements(&self) -> Option<Elements> { None }
}

#[test]
fn int32_add_of_two_ranges_is_the_range_of_their_sums() {
    let (left, right) = (range(0, 10), range(5, 20));
    assert_eq!(binary_type(op::Addition, left, right), Some(range(5, 30)));
    assert!(range(5, 30).is_int32());
    let env = Registers(vec![Cell::of(left), Cell::of(right)]);
    let add = Operation::Binary { destination: 2, left: Operand::register(0), right: Operand::register(1), operator: op::Addition };
    assert_eq!(value(&add, &env), Some(Cell::of(range(5, 30))));
}

#[test]
fn increment_and_decrement_step_a_range_inside_int32() {
    assert_eq!(step(range(0, 10), false), range(1, 11));
    assert_eq!(step(range(0, 10), true), range(-1, 9));
}

#[test]
fn increment_past_int32_widens_to_an_integer_that_is_not_nan() {
    assert_eq!(step(Type::INT32, false), Type::NOT_NAN_NUMBER);
    assert_eq!(step(Type::INT32, true), Type::NOT_NAN_NUMBER);
    assert_eq!(step(range(i64::from(i32::MAX), i64::from(i32::MAX)), false), Type::NOT_NAN_NUMBER);
}

#[test]
fn decrement_past_int32_widens_and_the_range_steps_by_one_at_each_bound() {
    assert_eq!(step(range(i64::from(i32::MIN), i64::from(i32::MIN)), true), Type::NOT_NAN_NUMBER);
    assert_eq!(step(range(-5, -5), true), range(-6, -6));
    assert_eq!(step(range(-5, 5), false), range(-4, 6));
}

#[test]
fn increment_of_a_non_number_is_a_number_or_a_bigint_and_of_an_unreached_operand_is_unreached() {
    let number_or_bigint = Type::NUMBER.join(Type::language(ValueType::BigInt));
    assert_eq!(step(Type::language(ValueType::String), false), number_or_bigint);
    assert_eq!(step(Type::ANY, true), number_or_bigint);
    assert_eq!(step(Type::NEVER, true), Type::NEVER);
}

#[test]
fn negation_of_an_int32_minimum_widens_past_int32_by_number_range() {
    assert_eq!(unary(NumberUnary::Negate, range(i64::from(i32::MIN), i64::from(i32::MIN))), range(1 << 31, 1 << 31));
    assert_eq!(unary(NumberUnary::Negate, range(-5, -1)), range(1, 5));
    assert_eq!(unary(NumberUnary::Negate, range(-3, 4)), Type::NOT_NAN_NUMBER, "a range holding zero may negate to -0");
}

#[test]
fn the_number_and_local_slot_increments_are_the_one_rule() {
    let number = Operation::Number(NumberOperation::IncDec { destination: 1, source: 0, decrement: false, overflow: Overflow::Exit });
    let local = Operation::IncDecLocal { local: 1, source: 0, decrement: true };
    let env = Registers(vec![Cell::of(range(0, 10))]);
    assert_eq!(value(&number, &env), Some(Cell::of(range(1, 11))));
    assert_eq!(value(&local, &env), Some(Cell::of(range(-1, 9))));
    let unknown = Registers(vec![Cell::ANY]);
    assert_eq!(value(&number, &unknown), Some(Cell::of(Type::NUMBER.join(Type::language(ValueType::BigInt)))));
    let unreached = Registers(vec![Cell::of(Type::NEVER)]);
    assert_eq!(value(&local, &unreached), Some(Cell::of(Type::NEVER)));
}

#[test]
fn integer_arithmetic_is_never_nan_and_a_non_integer_operand_may_be() {
    let integers = range(0, 10);
    assert!(!arithmetic(NumberArithmetic::Add, integers, integers).may_be_nan());
    let huge = Type::int(IntRange::singleton(1_i64 << 40));
    assert_eq!(arithmetic(NumberArithmetic::Multiply, huge, huge), Type::NOT_NAN_NUMBER, "an integer product beyond the word");
    assert_eq!(arithmetic(NumberArithmetic::Add, Type::NUMBER, integers), Type::NUMBER);
    assert_eq!(arithmetic(NumberArithmetic::Add, Type::NOT_NAN_NUMBER, integers), Type::NUMBER, "an infinity may be a non-integer operand");
    assert_eq!(arithmetic(NumberArithmetic::Multiply, Type::NOT_NAN_NUMBER, Type::NOT_NAN_NUMBER), Type::NUMBER, "0 * Infinity");
    assert_eq!(arithmetic(NumberArithmetic::Subtract, Type::NOT_NAN_NUMBER, Type::NOT_NAN_NUMBER), Type::NUMBER, "Infinity - Infinity");
}

#[test]
fn division_is_nan_only_where_zero_over_zero_is_possible() {
    assert_eq!(arithmetic(NumberArithmetic::Divide, range(0, 0), range(0, 4)), Type::NUMBER, "0 / 0");
    assert_eq!(arithmetic(NumberArithmetic::Divide, range(1, 9), range(0, 4)), Type::NOT_NAN_NUMBER, "x / 0 is an infinity");
    assert_eq!(arithmetic(NumberArithmetic::Divide, range(0, 9), range(1, 4)), Type::NOT_NAN_NUMBER, "0 / x is zero");
    assert_eq!(arithmetic(NumberArithmetic::Divide, Type::NUMBER, range(1, 4)), Type::NUMBER);
}

#[test]
fn remainder_by_a_divisor_that_may_be_zero_may_be_nan() {
    assert_eq!(remainder(range(0, 9), range(0, 4)), Type::NUMBER, "x % 0 is NaN");
    assert_eq!(remainder(range(0, 9), range(3, 4)), range(0, 3));
    assert_eq!(remainder(range(-9, 9), range(3, 4)), Type::NOT_NAN_NUMBER, "a negative dividend may be -0");
}

#[test]
fn math_functions_are_nan_for_a_nan_argument_and_sqrt_for_a_negative_one() {
    assert_eq!(math(FloatUnary::Sqrt, range(0, 16)), Type::NOT_NAN_NUMBER);
    assert_eq!(math(FloatUnary::Sqrt, range(-1, 4)), Type::NUMBER);
    assert_eq!(math(FloatUnary::Sqrt, Type::NOT_NAN_NUMBER), Type::NUMBER, "a non-integer of unknown sign");
    assert_eq!(math(FloatUnary::Floor, Type::NOT_NAN_NUMBER), Type::NOT_NAN_NUMBER);
    assert_eq!(math(FloatUnary::Floor, Type::NUMBER), Type::NUMBER);
    assert_eq!(math(FloatUnary::Sqrt, Type::NUMBER), Type::NUMBER);
}

#[test]
fn rounding_an_integer_range_keeps_the_range() {
    for operation in [FloatUnary::Floor, FloatUnary::Ceil, FloatUnary::Round, FloatUnary::Trunc] {
        assert_eq!(math(operation, range(-3, 7)), range(-3, 7));
        assert_eq!(math(operation, Type::NOT_NAN_NUMBER), Type::NOT_NAN_NUMBER, "a non-integer rounds to an integer, unbounded here");
    }
}

#[test]
fn max_and_min_are_nan_only_where_an_operand_is() {
    assert_eq!(extreme(true, Type::NOT_NAN_NUMBER, range(0, 3)), Type::NOT_NAN_NUMBER);
    assert_eq!(extreme(true, Type::NUMBER, range(0, 3)), Type::NUMBER, "Math.max(NaN, 1)");
    assert_eq!(extreme(false, range(0, 3), range(5, 9)), range(0, 3));
    assert_eq!(extreme(true, range(0, 3), range(5, 9)), range(5, 9));
}

#[test]
fn unary_negation_keeps_the_nan_bit_and_negates_only_ranges_without_zero() {
    assert_eq!(unary(NumberUnary::Negate, range(1, 5)), range(-5, -1));
    assert_eq!(unary(NumberUnary::Negate, range(0, 5)), Type::NOT_NAN_NUMBER, "-0");
    assert_eq!(unary(NumberUnary::Negate, Type::NUMBER), Type::NUMBER);
    assert_eq!(unary(NumberUnary::Plus, Type::NUMBER), Type::NUMBER);
    assert_eq!(step(Type::NOT_NAN_NUMBER, false), Type::NOT_NAN_NUMBER);
    assert_eq!(step(Type::NUMBER, false), Type::NUMBER);
}

#[test]
fn to_number_is_nan_free_for_numbers_booleans_and_null_only() {
    assert_eq!(to_number(Type::NUMBER), Type::NUMBER);
    assert_eq!(to_number(range(0, 3)), range(0, 3));
    assert_eq!(to_number(Type::language(ValueType::Boolean)), Type::NOT_NAN_NUMBER);
    assert_eq!(to_number(Type::language(ValueType::Null)), Type::NOT_NAN_NUMBER);
    assert_eq!(to_number(Type::language(ValueType::Undefined)), Type::NUMBER, "undefined converts to NaN");
    assert_eq!(to_number(Type::language(ValueType::String)), Type::NUMBER);
}

#[test]
fn the_nan_bit_joins_by_or_and_integers_are_nan_free() {
    assert_eq!(range(0, 3).join(Type::NOT_NAN_NUMBER), Type::NOT_NAN_NUMBER);
    assert_eq!(Type::NOT_NAN_NUMBER.join(Type::NUMBER), Type::NUMBER);
    assert!(range(0, 3).is_subtype(Type::NOT_NAN_NUMBER) && Type::NOT_NAN_NUMBER.is_subtype(Type::NUMBER));
    assert!(!Type::NUMBER.is_subtype(Type::NOT_NAN_NUMBER));
    assert!(!Type::NOT_NAN_NUMBER.may_be_nan() && Type::NUMBER.may_be_nan());
    assert!(!Type::NEVER.may_be_nan() && !Type::language(ValueType::Boolean).may_be_nan());
    assert_eq!(Type::NOT_NAN_NUMBER.machine(), Type::NUMBER);
}

#[test]
fn a_relation_edge_refines_a_counter() {
    let (counter, limit) = (span(0, 100), span(10, 10));
    // `counter < 10` holds on the taken edge: the counter keeps [0, 9] and the limit is still 10.
    assert_eq!(relation_edge(NumberRelation::LessThan, true, counter, limit), Some([span(0, 9), span(10, 10)]));
    // Where it fails the counter is at least the limit.
    assert_eq!(relation_edge(NumberRelation::LessThan, false, counter, limit), Some([span(10, 100), span(10, 10)]));
}

/// One loop's solve over a single register: the header joins its backedge for `WIDENING_ROUNDS` rounds, then widens
/// (as `flow::Solver::join` does), and one descending pass recomputes the header from its backedge without widening.
fn solve_header(entry: Type, step_body: impl FnMut(Type) -> Type) -> Type {
    solve_with(&thresholds([]), entry, step_body)
}

/// `solve_header` widening to the given thresholds: a loop header's own (`thresholds([])`), or a field's, which the
/// solve widens to the constants stored to and compared against it (`aot_facts::types::Types::widen`).
fn solve_with(thresholds: &[i64], entry: Type, mut step_body: impl FnMut(Type) -> Type) -> Type {
    let mut header = entry;
    for round in 0..=u32::from(crate::typed_ir::WIDENING_ROUNDS) {
        let joined = header.join(step_body(header));
        header = if round >= u32::from(crate::typed_ir::WIDENING_ROUNDS) { widen(thresholds, header, joined) } else { joined };
    }
    entry.join(step_body(header))
}

#[test]
fn a_field_counter_incremented_under_a_bound_stays_within_its_constants() {
    // `this.count` is stored 0 at construction and `this.count = this.count + 1` runs only where `this.count < 1000`:
    // the field's cell is the join of the stores, so the solve's thresholds are the constants it stores and compares.
    let bounds = thresholds([0, 1000]);
    let field = solve_with(&bounds, Type::int(IntRange::singleton(0)), |field| {
        let [counter, _] = relation_edge(NumberRelation::LessThan, true, field.int_range().unwrap(), span(1000, 1000)).unwrap();
        step(Type::int(counter), false)
    });
    assert_eq!(field, range(0, 1000));
}

#[test]
fn a_field_compared_with_a_constant_settles_at_that_constant() {
    // The same counter with the constant only as a compared bound (`this.count < 1000`, no store of 1000): the
    // relational jump's bound alone is the threshold the field widens to.
    let bounds = thresholds([1000]);
    let field = solve_with(&bounds, Type::int(IntRange::singleton(0)), |field| {
        let [counter, _] = relation_edge(NumberRelation::LessThan, true, field.int_range().unwrap(), span(1000, 1000)).unwrap();
        step(Type::int(counter), false)
    });
    assert_eq!(field, range(0, 1000));
}

#[test]
fn a_mask_bounds_its_result_to_a_nonnegative_constant() {
    assert_eq!(mask_bound(IntRange::singleton(0xfffffff)), Some(0xfffffff));
    assert_eq!(mask_bound(IntRange::singleton(-1)), None, "a negative mask keeps no bound of its own");
    assert_eq!(bitwise(NumberBitwise::And, Type::NUMBER, Type::int(IntRange::singleton(0xfffffff))), range(0, 0xfffffff));
}

#[test]
fn a_field_stored_a_mask_stays_within_the_mask() {
    // `this.digit = (this.digit + x) & 0xfffffff` with x in [0, 2^30]: the mask's bound is one of the field's thresholds,
    // so the widened field is the mask's [0, 0xfffffff] and not int32's span, where a store would reach past the mask.
    const MASK: i64 = 0xfffffff;
    let bounds = thresholds([mask_bound(IntRange::singleton(MASK)).unwrap()]);
    let x = span(0, 1 << 30);
    let field = solve_with(&bounds, Type::int(IntRange::singleton(0)), |field| {
        bitwise(NumberBitwise::And, arithmetic(NumberArithmetic::Add, field, Type::int(x)), Type::int(IntRange::singleton(MASK)))
    });
    assert_eq!(field, range(0, MASK));
}

#[test]
fn a_grown_field_range_widens_to_the_nearest_constant_not_to_int32() {
    // A counter stored 1000 and grown past 6 by one per round widens to 999 (the constant beside it), not int32's span.
    let bounds = thresholds([1000]);
    assert_eq!(widen(&bounds, range(0, 6), range(0, 7)), range(0, 999));
    // Past every constant, the widening settles at int32's span (`thresholds` always holds it), not `WORD`.
    assert_eq!(widen(&bounds, range(0, 6), range(0, 2000)), range(0, i64::from(i32::MAX)));
}

#[test]
fn a_counted_loop_counter_stays_within_its_bound() {
    // `for (i = 0; i < n; i++)` with n in [0, 100]: the body sees i below n, so the header's back edge carries [1, n].
    let (entry, n) = (span(0, 0), span(0, 100));
    let header = solve_header(Type::int(entry), |header| {
        let [counter, _] = relation_edge(NumberRelation::LessThan, true, header.int_range().unwrap(), n).unwrap();
        step(Type::int(counter), false)
    });
    assert_eq!(header, range(0, 100));
}

#[test]
fn a_carry_recurrence_reaches_a_bounded_fixed_point_inside_int32() {
    // `c = (c + x) >>> 15` with x in [0, 2^30): the carry is at most 2^15 once the sum is shifted, and it never leaves int32.
    let (x, shift) = (span(0, (1 << 30) - 1), Type::int(IntRange::singleton(15)));
    let carry = solve_header(Type::int(IntRange::singleton(0)), |carry| {
        bitwise(NumberBitwise::ShiftRightUnsigned, arithmetic(NumberArithmetic::Add, carry, Type::int(x)), shift)
    });
    assert!(carry.is_int32(), "the carry stays an int32: {carry:?}");
    assert!(carry.int_range().unwrap().max <= 1 << 15 || carry == Type::INT32, "{carry:?}");
}

#[test]
fn power_thresholds_cover_a_growing_range_with_a_power_of_two_margin() {
    assert_eq!(power_thresholds(span(0, 5)), [0, 7], "the next power of two above 5, less one");
    assert_eq!(power_thresholds(span(0, 7)), [0, 7], "7 is itself one less than a power of two");
    assert_eq!(power_thresholds(span(-5, 3)), [-8, 3], "the next power of two of -5 is 8");
    assert_eq!(power_thresholds(span(0, 1 << 28)), [0, (1 << 29) - 1], "a sum past 2^28 doubles to 2^29 - 1");
    assert_eq!(power_thresholds(span(0, i64::from(i32::MAX) + 1)), [0, i64::from(i32::MAX)], "clamped to int32's span");
}
