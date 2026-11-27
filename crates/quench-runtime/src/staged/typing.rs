//! The one typing of an operation: the cell a staged `Operation<(), ()>` defines over the cells of its operands, and the
//! numeric rules those cells follow (an operator's result, an increment, a relation's edges, a counter's settled type).
//! The solve (`compile/aot_facts`) applies these rules to every instruction, the JIT lane's graph lowering reads the same
//! cells through `Stage::fact`, and check deletion relies on them, so the table lives here beside the rows that decode
//! the operations, not in one consumer.
//!
//! The rules take and return `typed_ir` values only. What a value is bound to (its origin), the keys a step writes and
//! the walk over the instructions stay with the caller, which reads a rule's cell and keeps its own bookkeeping.

use super::{NumberOperation, Operation};
use crate::bytecode::{Operand, OperandKind, Register};
use crate::typed_ir::{Cell, Elements, IntRange, Length, Type};
use crate::vm::operations::{binary_operator as op, NumberArithmetic, NumberBitwise, NumberRelation};
use crate::ValueType;

/// The cells an operation's operands read: a register's, a program constant's, and the elements no field names.
/// The solve implements it over the registers before an instruction; a lowering implements it over its own values.
pub trait Abstract {
    /// The cell of `register`.
    fn register(&self, register: Register) -> Cell;
    /// The cell of the program constant at `index`: the type of its value.
    fn constant(&self, index: usize) -> Cell;
    /// The elements of the arrays no field names hold, or `None` where the types are not solved yet, so an element
    /// read is any value.
    fn unnamed_elements(&self) -> Option<Elements>;
}

/// The cell of an operand: its register's, the type of its constant, else any value.
pub fn operand_cell(env: &impl Abstract, operand: Operand) -> Cell {
    match (operand.register_index(), operand.kind()) {
        (Some(register), _) => env.register(register),
        (None, Some(OperandKind::Constant)) => env.constant(usize::from(operand.payload())),
        _ => Cell::ANY,
    }
}

/// The cell an operation defines, where its value is typed by its operands: a Number, a `Binary`, a predicate's Boolean,
/// `void`'s `undefined`, an indexed read. `None` for the operations whose value keeps an operand's origin or binds a key
/// (moves, loads, closures, constants), which the caller types itself.
pub fn value(op: &Operation<(), ()>, env: &impl Abstract) -> Option<Cell> {
    Some(match *op {
        Operation::Number(number) => number_cell(env, number),
        Operation::Binary { left, right, operator, .. } => binary(env, left, right, operator),
        Operation::Equality { .. } | Operation::LogicalNot { .. } | Operation::TypeofTest { .. } | Operation::InstanceOf { .. } =>
            Cell::of(Type::language(ValueType::Boolean)),
        Operation::Void { .. } => Cell::of(Type::language(ValueType::Undefined)),
        Operation::ElementRead { base, key, .. } => element(env, env.register(base), operand_cell(env, key)),
        _ => return None,
    })
}

/// The cell `left operator right` defines: a comparison is a Boolean whatever its operands; otherwise the operator's type
/// over the operands' types (`binary_type`), else any value. An operand no value reaches yet (`NEVER`, the solve's bottom)
/// makes the result `NEVER` too, except for `in` and `instanceof`, which stay any value.
pub fn binary(env: &impl Abstract, left: Operand, right: Operand, operator: u32) -> Cell {
    let (left, right) = (operand_cell(env, left).ty, operand_cell(env, right).ty);
    if left == Type::NEVER || right == Type::NEVER {
        return if matches!(operator, op::In | op::Instanceof) { Cell::ANY } else { Cell::of(Type::NEVER) };
    }
    if matches!(operator, op::Equality | op::Inequality | op::StrictEquality | op::StrictInequality
        | op::LessThan | op::LessEqualThan | op::GreaterThan | op::GreaterEqualThan) {
        return Cell::of(Type::language(ValueType::Boolean));
    }
    binary_type(operator, left, right).map_or(Cell::ANY, Cell::of)
}

/// The cell a Number operation defines, when its operands are Numbers: an operand that is not one may run guest code or
/// yield a string, so the operation then defines any value.
pub fn number_cell(env: &impl Abstract, number: NumberOperation) -> Cell {
    let fact = |operand: Operand| operand_cell(env, operand);
    // An operand no value reaches yet (`Type::NEVER`) is a Number among the others: what reaches it later is joined in
    // a later pass, and an operation over it defines none yet.
    let numeric = |cell: Cell| cell.ty.is_subtype(Type::NUMBER);
    let both = |left: Operand, right: Operand| numeric(fact(left)) && numeric(fact(right));
    let one = |source: Operand| numeric(fact(source));
    let register = |source: Register| env.register(source);
    let reached = |cells: &[Cell]| cells.iter().all(|cell| cell.ty != Type::NEVER);
    let operands: Vec<Cell> = match number {
        NumberOperation::Arithmetic { left, right, .. } | NumberOperation::Remainder { left, right, .. } | NumberOperation::Extreme { left, right, .. }
        | NumberOperation::Bitwise { left, right, .. } | NumberOperation::Imul { left, right, .. } => vec![fact(left), fact(right)],
        NumberOperation::Unary { source, .. } | NumberOperation::Math { source, .. } | NumberOperation::BitwiseNot { source, .. } => vec![fact(source)],
        NumberOperation::IncDec { source, .. } | NumberOperation::ToNumeric { source, .. } => vec![register(source)],
        _ => Vec::new(),
    };
    if !reached(&operands) && operands.iter().all(|cell| numeric(*cell)) { return Cell::of(Type::NEVER); }
    let ty = |operand: Operand| fact(operand).ty;
    match number {
        NumberOperation::Arithmetic { left, right, operation, .. } if both(left, right) => Cell::of(arithmetic(operation, ty(left), ty(right))),
        NumberOperation::Remainder { left, right, .. } if both(left, right) => Cell::of(remainder(ty(left), ty(right))),
        NumberOperation::Extreme { left, right, .. } if both(left, right) => Cell::of(Type::NUMBER),
        NumberOperation::Bitwise { left, right, operation, .. } if both(left, right) => Cell::of(bitwise(operation, ty(left), ty(right))),
        NumberOperation::Imul { left, right, .. } if both(left, right) => Cell::of(Type::INT32),
        NumberOperation::Unary { source, .. } | NumberOperation::Math { source, .. } if one(source) => Cell::of(Type::NUMBER),
        NumberOperation::BitwiseNot { source, .. } if one(source) => Cell::of(Type::INT32),
        NumberOperation::IncDec { source, decrement, .. } if numeric(register(source)) => Cell::of(step(register(source).ty, decrement)),
        NumberOperation::ToNumeric { source, .. } if numeric(register(source)) => register(source),
        _ => Cell::ANY,
    }
}

/// The cell of an indexed read of `base` at `index`: the elements `base` names (`Elements::element`), joined with those no
/// field names (`Abstract::unnamed_elements`), and with `undefined` unless the index is within a fixed length
/// (`within_length`), where a hole may not read. Any value where the types are not solved yet.
pub fn element(env: &impl Abstract, base: Cell, index: Cell) -> Cell {
    let Some(unnamed) = env.unnamed_elements() else { return Cell::ANY };
    let elements = base.elements.join(unnamed);
    let element = elements.element();
    if within_length(elements.length, index.ty) { element } else { element.join(Cell::of(Type::language(ValueType::Undefined))) }
}

/// Whether an index of type `index` stays within an array of length `length`: a fixed length, and an integer range inside
/// `[0, length)`.
pub fn within_length(length: Length, index: Type) -> bool {
    match (length, index.int_range()) {
        (Length::Fixed(count), Some(range)) => range.min >= 0 && range.max < i64::from(count),
        _ => false,
    }
}

/// Whether a cell is a Number the solve has reached: its type is a Number and not the solve's bottom. The `ToNumeric` of
/// such a cell defines that cell itself.
pub fn reached_number(cell: Cell) -> bool {
    cell.ty != Type::NEVER && cell.ty.is_subtype(Type::NUMBER)
}

/// The type `left operator right` defines, where a rule names the operator: a BigInt of two BigInts, a string of a `+`
/// with a string, else the Number of a numeric operator (a BigInt beside no Number). `None` for an operator no rule
/// names here: a comparison, `in`, `instanceof`, or a `+` whose operands may concatenate or be any value.
pub fn binary_type(operator: u32, left: Type, right: Type) -> Option<Type> {
    let bigint = Type::language(ValueType::BigInt);
    let string = Type::language(ValueType::String);
    let number = |ty: Type| ty.is_subtype(Type::NUMBER);
    if bigint_operator(operator) && left.is_subtype(bigint) && right.is_subtype(bigint) { return Some(bigint); }
    if operator == op::Addition {
        if left.is_subtype(string) || right.is_subtype(string) { return Some(string); }
        return (number(left) && number(right)).then(|| arithmetic(NumberArithmetic::Add, left, right));
    }
    // A Number operand makes the other's coercion a Number too: a BigInt beside it throws. With neither a Number, the
    // operator's Number may be a BigInt instead.
    let numeric = |ty: Type| if number(left) || number(right) { ty } else { ty.join(bigint) };
    if let Some(operation) = NumberArithmetic::from_operator(operator) {
        return Some(numeric(arithmetic(operation, left, right)));
    }
    if let Some(operation) = NumberBitwise::from_operator(operator) {
        return Some(numeric(bitwise(operation, left, right)));
    }
    (operator == op::Remainder).then(|| numeric(remainder(left, right)))
}

/// Whether two BigInts give a BigInt by `operator`: the arithmetic, bitwise and remainder operators but `>>>`.
fn bigint_operator(operator: u32) -> bool {
    (NumberArithmetic::from_operator(operator).is_some() || NumberBitwise::from_operator(operator).is_some() || operator == op::Remainder)
        && operator != op::ShiftRightZeroFill
}

/// The type of an arithmetic operator over two Numbers: the integer range of two integer ranges (`int_arithmetic`),
/// else a Number.
pub fn arithmetic(operation: NumberArithmetic, left: Type, right: Type) -> Type {
    match (left.int_range(), right.int_range()) {
        (Some(left), Some(right)) => int_arithmetic(operation, left, right).map_or(Type::NUMBER, Type::int),
        _ => Type::NUMBER,
    }
}

/// The exact range of an arithmetic operator over integer ranges, where its result is an integer Number a word holds.
/// A product that may be a negative zero is no integer.
fn int_arithmetic(operation: NumberArithmetic, left: IntRange, right: IntRange) -> Option<IntRange> {
    let range = match operation {
        NumberArithmetic::Add => left.add(right),
        NumberArithmetic::Subtract => left.subtract(right),
        NumberArithmetic::Multiply if !left.product_may_be_negative_zero(right) => left.multiply(right),
        NumberArithmetic::Multiply | NumberArithmetic::Divide => None,
    };
    range.and_then(IntRange::as_number)
}

/// The type of a bitwise operator over Numbers. An operand reads as its int32, so every operator but `>>>` defines an
/// int32 whatever its operands; `>>>` defines an int32 only by a shift of nonzero count.
pub fn bitwise(operation: NumberBitwise, left: Type, right: Type) -> Type {
    let operand = |ty: Type| ty.int_range().unwrap_or(IntRange::FULL).int32_operand();
    let (left, right) = (operand(left), operand(right));
    match operation {
        NumberBitwise::And => Type::int(left.and(right)),
        NumberBitwise::Or => Type::int(left.or(right)),
        NumberBitwise::Xor => Type::int(left.bitwise_hull(right)),
        // A left shift's word is ToInt32 of its result, which lies in int32 whatever its operands.
        NumberBitwise::ShiftLeft => Type::INT32,
        NumberBitwise::ShiftRight => Type::int(left.shift_right(right).unwrap_or(IntRange::FULL)),
        NumberBitwise::ShiftRightUnsigned => left.shift_right_unsigned(right).map_or(Type::NUMBER, Type::int),
    }
}

/// The type of `left % right` over Numbers: the remainder's range of two integer ranges (`IntRange::remainder`), else a
/// Number.
pub fn remainder(left: Type, right: Type) -> Type {
    match (left.int_range(), right.int_range()) {
        (Some(left), Some(right)) => left.remainder(right).map_or(Type::NUMBER, Type::int),
        _ => Type::NUMBER,
    }
}

/// The type `++` (or `--`, where `decrement`, and also its local-slot form, which defines the same value) defines over an
/// operand of type `source`: the stepped range of an integer where it stays in int32 (an int32 then), else a Number,
/// or a Number or BigInt where the operand may be a BigInt.
pub fn step(source: Type, decrement: bool) -> Type {
    if source == Type::NEVER { return Type::NEVER; }
    if let Some(range) = source.int_range() {
        return range.stepped(decrement).filter(IntRange::fits_int32).map_or(Type::NUMBER, Type::int);
    }
    if source.is_subtype(Type::NUMBER) { Type::NUMBER } else { Type::NUMBER.join(Type::language(ValueType::BigInt)) }
}

/// The integer bounds a loop header widens its ranges to (`widen`): int32's two bounds, and each constant a relational
/// jump compares a value against, one below and one above it, since a strict and a non-strict comparison step a counter
/// one past the constant. Sorted and distinct, so the nearest one above or below a bound is a search.
pub fn thresholds(bounds: impl IntoIterator<Item = i64>) -> Vec<i64> {
    let mut all: Vec<i64> = bounds.into_iter().flat_map(|bound| [bound - 1, bound, bound + 1])
        .chain([IntRange::FULL.min, IntRange::FULL.max]).collect();
    all.sort_unstable();
    all.dedup();
    all
}

/// The type a loop header holds once its join has changed `rounds` times past `WIDENING_ROUNDS` (`flow::Solver::join`):
/// `joined` (the join of `known` and the incoming type) with each integer bound that grew moved to its nearest threshold
/// (`thresholds`), so the header's ranges reach a fixed bound in finitely many joins and stay within the thresholds a
/// relational edge compares against. Types that are not both integer ranges keep their join.
pub fn widen(thresholds: &[i64], known: Type, joined: Type) -> Type {
    match (known.int_range(), joined.int_range()) {
        (Some(known), Some(joined)) => Type::int(widen_range(thresholds, known, joined)),
        _ => joined,
    }
}

/// `joined`, a hull of `known` and an incoming range, with a bound that `known` did not reach moved to the nearest
/// threshold beyond it, or to `IntRange::WORD`'s bound where no threshold lies beyond (a value a word holds exactly).
fn widen_range(thresholds: &[i64], known: IntRange, joined: IntRange) -> IntRange {
    let up = |value: i64| thresholds.iter().copied().find(|&bound| bound >= value).unwrap_or(IntRange::WORD.max);
    let down = |value: i64| thresholds.iter().rev().copied().find(|&bound| bound <= value).unwrap_or(IntRange::WORD.min);
    IntRange {
        min: if joined.min < known.min { down(joined.min) } else { joined.min },
        max: if joined.max > known.max { up(joined.max) } else { joined.max },
    }
}

/// The ranges a relational jump proves of its operands `left` and `right` on the edge taken where the relation holds
/// (`truth`), or where it fails. `None` where no pair of values reaches the edge.
pub fn relation_edge(relation: NumberRelation, truth: bool, left: IntRange, right: IntRange) -> Option<[IntRange; 2]> {
    let relation = if truth { relation } else { negated(relation) };
    match relation {
        NumberRelation::Equal => {
            let common = left.intersect(right)?;
            Some([common, common])
        }
        NumberRelation::NotEqual => Some([left.without(right)?, right.without(left)?]),
        NumberRelation::LessThan => left.ordered_edge(right, true),
        NumberRelation::LessEqual => left.ordered_edge(right, false),
        NumberRelation::GreaterThan => right.ordered_edge(left, true).map(|[right, left]| [left, right]),
        NumberRelation::GreaterEqual => right.ordered_edge(left, false).map(|[right, left]| [left, right]),
    }
}

/// The relation that holds where `relation` does not.
fn negated(relation: NumberRelation) -> NumberRelation {
    match relation {
        NumberRelation::Equal => NumberRelation::NotEqual,
        NumberRelation::NotEqual => NumberRelation::Equal,
        NumberRelation::LessThan => NumberRelation::GreaterEqual,
        NumberRelation::LessEqual => NumberRelation::GreaterThan,
        NumberRelation::GreaterThan => NumberRelation::LessEqual,
        NumberRelation::GreaterEqual => NumberRelation::LessThan,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn increment_past_int32_widens_to_a_number() {
        assert_eq!(step(Type::INT32, false), Type::NUMBER);
        assert_eq!(step(Type::INT32, true), Type::NUMBER);
        assert_eq!(step(range(i64::from(i32::MAX), i64::from(i32::MAX)), false), Type::NUMBER);
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
    fn solve_header(entry: Type, mut step_body: impl FnMut(Type) -> Type) -> Type {
        let thresholds = thresholds([]);
        let mut header = entry;
        for round in 0..=u32::from(crate::typed_ir::WIDENING_ROUNDS) {
            let joined = header.join(step_body(header));
            header = if round >= u32::from(crate::typed_ir::WIDENING_ROUNDS) { widen(&thresholds, header, joined) } else { joined };
        }
        entry.join(step_body(header))
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
}
