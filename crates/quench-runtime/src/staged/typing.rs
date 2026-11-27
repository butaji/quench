//! The one typing of an operation: the cell a staged `Operation<(), ()>` defines over the cells of its operands, and the
//! numeric rules those cells follow (an operator's result, an increment, a relation's edges, a counter's settled type).
//! The solve (`compile/aot_facts`) applies these rules to every instruction, the JIT lane's graph lowering reads the same
//! cells through `Stage::fact`, and check deletion relies on them, so the table lives here beside the rows that decode
//! the operations, not in one consumer.
//!
//! The rules take and return `typed_ir` values only. What a value is bound to (its origin), the keys a step writes and
//! the walk over the instructions stay with the caller, which reads a rule's cell and keeps its own bookkeeping.

use super::math::FloatUnary;
use super::{NumberOperation, Operation};
use crate::bytecode::{Operand, OperandKind, Register};
use crate::typed_ir::arithmetic::number_range;
use crate::typed_ir::{Cell, Elements, IntRange, Length, StringLength, Type, language_bit};
use crate::vm::operations::{binary_operator as op, NumberArithmetic, NumberBitwise, NumberRelation, NumberUnary};
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
        // The local-slot increment defines the value `step` gives its source, as the Number form does.
        Operation::IncDecLocal { source, decrement, .. } => Cell::of(step(env.register(source).ty, decrement)),
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
/// yield a string, so the operation then defines any value. An increment is the exception: `step` gives the type of
/// `++` over any operand (a Number or a BigInt), so it is defined over every operand.
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
        NumberOperation::Extreme { left, right, maximum, .. } if both(left, right) => Cell::of(extreme(maximum, ty(left), ty(right))),
        NumberOperation::Bitwise { left, right, operation, .. } if both(left, right) => Cell::of(bitwise(operation, ty(left), ty(right))),
        NumberOperation::Imul { left, right, .. } if both(left, right) => Cell::of(Type::INT32),
        NumberOperation::Unary { source, operation, .. } if one(source) => Cell::of(unary(operation, ty(source))),
        NumberOperation::Math { source, operation, .. } if one(source) => Cell::of(math(operation, ty(source))),
        NumberOperation::BitwiseNot { source, .. } if one(source) => Cell::of(Type::INT32),
        NumberOperation::IncDec { source, decrement, .. } => Cell::of(step(register(source).ty, decrement)),
        NumberOperation::ToNumeric { source, .. } if numeric(register(source)) => register(source),
        _ => Cell::ANY,
    }
}

/// The cell of an indexed read of `base` at `index`: the elements `base` names (`Elements::element`), joined with those no
/// field names (`Abstract::unnamed_elements`). A read takes `undefined` where the array has holes or the index is not
/// within a fixed length (`within_length`). Any value where the types are not solved yet.
pub fn element(env: &impl Abstract, base: Cell, index: Cell) -> Cell {
    let Some(unnamed) = env.unnamed_elements() else { return Cell::ANY };
    let elements = base.elements.join(unnamed);
    elements.element(within_length(elements.length, index.ty))
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
        if left.is_subtype(string) || right.is_subtype(string) { return Some(concatenation(left, right)); }
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

/// The type of an arithmetic operator over two Numbers. Two integer ranges give the operator's integer range
/// (`typed_ir::arithmetic::number_range`) where it has one. Otherwise the result is an integer or a NaN-free Number
/// (`NOT_NAN_NUMBER`), except where NaN is possible:
/// - an operand that may be NaN, or an infinity of a NaN-free non-integer (`Infinity - Infinity`, `0 * Infinity`):
///   no float range is kept, so any operand that is not an integer may be infinite, and `+`, `-`, `*` of such operands
///   is `NUMBER`;
/// - `0 / 0`: a division whose dividend and divisor both may be zero. A dividend or a divisor that excludes zero gives
///   an infinity or a finite quotient, never NaN.
pub fn arithmetic(operation: NumberArithmetic, left: Type, right: Type) -> Type {
    match (left.int_range(), right.int_range()) {
        (Some(left), Some(right)) => {
            if operation == NumberArithmetic::Divide && left.contains(IntRange::ZERO) && right.contains(IntRange::ZERO) {
                return Type::NUMBER;
            }
            // An integer result is never NaN; `number_range` is `None` for a quotient (not an integer) and a product that
            // leaves the word span, which are NaN-free Numbers all the same.
            crate::typed_ir::arithmetic::number_range(operation, left, right).map_or(Type::NOT_NAN_NUMBER, Type::int)
        }
        _ => Type::NUMBER,
    }
}

/// The type of `Math.max` (`maximum`) or `Math.min` of two Numbers. Two integer ranges give the integer range of their
/// extremes. Otherwise the result is NaN wherever either operand is (`Math.max(NaN, 1)` is NaN), so a NaN-free Number
/// only where neither may be.
pub fn extreme(maximum: bool, left: Type, right: Type) -> Type {
    match (left.int_range(), right.int_range()) {
        (Some(left), Some(right)) => {
            let (min, max) = if maximum {
                (left.min.max(right.min), left.max.max(right.max))
            } else {
                (left.min.min(right.min), left.max.min(right.max))
            };
            IntRange::bounds(min, max).map_or(Type::NOT_NAN_NUMBER, Type::int)
        }
        _ if left.may_be_nan() || right.may_be_nan() => Type::NUMBER,
        _ => Type::NOT_NAN_NUMBER,
    }
}

/// The type of `+source` (`Plus`) or `-source` (`Negate`) over a Number of type `source`. A unary keeps a Number's NaN
/// bit, since NaN is its own negation. Negating an integer range is `0 - source` (`number_range`, as the arithmetic
/// rule computes it), which gives an integer range only where the range excludes zero: `-0` is a Number no integer
/// type holds.
pub fn unary(operation: NumberUnary, source: Type) -> Type {
    match (operation, source.int_range()) {
        (NumberUnary::Plus, _) => source,
        (NumberUnary::Negate, Some(range)) if !range.contains(IntRange::ZERO) =>
            number_range(NumberArithmetic::Subtract, IntRange::ZERO, range).map_or(Type::NOT_NAN_NUMBER, Type::int),
        (NumberUnary::Negate, _) if source.may_be_nan() => Type::NUMBER,
        (NumberUnary::Negate, _) => Type::NOT_NAN_NUMBER,
    }
}

/// The type of the Math function `operation` over a Number of type `source`. Every function returns NaN for a NaN
/// argument, so a source that may be NaN gives `NUMBER`. `Math.sqrt` of a negative is NaN, and a Number of unknown sign
/// may be negative (no float range is kept), so `Math.sqrt` is NaN-free only over an integer range with no negative
/// bound. Every other function of a NaN-free source (rounding, absolute value, sign, `fround`) is NaN-free.
pub fn math(operation: FloatUnary, source: Type) -> Type {
    if source.may_be_nan() { return Type::NUMBER; }
    match operation {
        FloatUnary::Sqrt if !source.int_range().is_some_and(|range| range.min >= 0) => Type::NUMBER,
        _ => Type::NOT_NAN_NUMBER,
    }
}

/// The type `ToNumber` of a value of type `ty` defines. A Number is its own `ToNumber`, a Boolean or `null` converts to
/// 0 or 1, and neither is NaN. `undefined` converts to NaN, a string may hold no numeral, and an object's `valueOf` may
/// return NaN, so any other type defines a Number that may be NaN.
pub fn to_number(ty: Type) -> Type {
    if ty == Type::NEVER || ty.is_subtype(Type::NUMBER) { return ty; }
    let nan_free = Type::languages(language_bit(ValueType::Boolean) | language_bit(ValueType::Null));
    if ty.is_subtype(nan_free) { Type::NOT_NAN_NUMBER } else { Type::NUMBER }
}


/// The type of a bitwise operator over Numbers. An operand reads as its int32 (`ToInt32` maps NaN to 0), so every
/// operator defines an integer: an int32 for all but `>>>`, which defines an int32 only by a shift of nonzero count and
/// otherwise an integer beyond int32's span (a NaN-free Number).
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
        NumberBitwise::ShiftRightUnsigned => left.shift_right_unsigned(right).map_or(Type::NOT_NAN_NUMBER, Type::int),
    }
}

/// The type of `left % right` over Numbers. A zero divisor gives NaN (`x % 0`), so a divisor that may be zero leaves
/// a Number that may be NaN. Otherwise the remainder's integer range (`IntRange::remainder`), or a NaN-free Number where
/// the dividend may be a negative zero.
pub fn remainder(left: Type, right: Type) -> Type {
    match (left.int_range(), right.int_range()) {
        (Some(left), Some(right)) if !right.contains(IntRange::ZERO) => left.remainder(right).map_or(Type::NOT_NAN_NUMBER, Type::int),
        _ => Type::NUMBER,
    }
}

/// The one rule of `++` (or `--`, where `decrement`): the type it defines over an operand of type `source`, for the
/// Number instruction (`number_cell`) and the local-slot form (`value`), which defines the same value. An integer steps
/// by one through `number_range`, and the result is an int32 (the stepped range) only where it fits int32, else the
/// integer beyond int32 (a NaN-free Number). A Number keeps its NaN bit, and any other operand is coerced to a Number
/// or a BigInt.
pub fn step(source: Type, decrement: bool) -> Type {
    if source == Type::NEVER { return Type::NEVER; }
    if let Some(range) = source.int_range() {
        let operation = if decrement { NumberArithmetic::Subtract } else { NumberArithmetic::Add };
        return number_range(operation, range, IntRange::singleton(1)).filter(IntRange::fits_int32).map_or(Type::NOT_NAN_NUMBER, Type::int);
    }
    if source.is_subtype(Type::NUMBER) {
        if source.may_be_nan() { Type::NUMBER } else { Type::NOT_NAN_NUMBER }
    } else {
        Type::NUMBER.join(Type::language(ValueType::BigInt))
    }
}

/// The mask bound a `&` with a constant operand of range `constant` holds its result to: `x & C` is in [0, C] for any `x`
/// when C is nonnegative (`IntRange::and`), so the bound is C. A negative constant gives no bound of its own.
pub fn mask_bound(constant: IntRange) -> Option<i64> {
    (constant.min >= 0).then_some(constant.max)
}

/// The power-of-two bounds an element class's range widens to (`Types::widen`) beside its constant thresholds: the max is the
/// next power of two above `range`'s max, less one, and the min the negation of the next power of two below its min (0 where
/// it is not negative). Each is clamped to int32's span, so a bound past it falls to `IntRange::WORD` as before. A read-modify-write
/// store (`+=`, `|=`) doubles a class's range each pass, so a power-of-two bound keeps a digit sum or carry in int32 with a 2x
/// margin, where the nearest constant would be the word.
pub fn power_thresholds(range: IntRange) -> [i64; 2] {
    let up = i64::try_from((range.max.max(0) as u64 + 1).next_power_of_two() - 1).unwrap_or(i64::MAX).min(IntRange::FULL.max);
    let down = if range.min >= 0 { 0 } else { (-i64::try_from(range.min.unsigned_abs().next_power_of_two()).unwrap_or(i64::MAX)).max(IntRange::FULL.min) };
    [down, up]
}

/// The integer bounds a range widens to (`widen`): int32's two bounds, and each given bound one below and one above it.
/// A loop header is given the constants its relational jumps compare with; a key is given the constants it is stored,
/// compared or masked to (`mask_bound`). A strict and a non-strict comparison step a counter one past its constant, and
/// a masked value's bound is the mask's own. Sorted and distinct, so the nearest one above or below a bound is a search.
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

/// The type `left + right` defines where one operand is a string: a fresh string, never an atom (a concatenation is built
/// at run time), its length the sum of the operands' length ranges. An operand that is not a string converts to one whose
/// length no rule here bounds, so its length is the full range.
pub fn concatenation(left: Type, right: Type) -> Type {
    let length = |ty: Type| ty.string_fact().map_or(StringLength::FULL, |fact| fact.length());
    let (left, right) = (length(left), length(right));
    Type::string_length(left.min.saturating_add(right.min), left.max.saturating_add(right.max))
}

/// The type `String(value)` defines over a value of type `ty`: the value itself where it is a string (the conversion
/// returns it, so its atom survives), else a string of any length (a Number's or an object's string form is not bounded
/// here). `NEVER` stays `NEVER`.
pub fn to_string(ty: Type) -> Type {
    if ty == Type::NEVER || ty.string_fact().is_some() { return ty; }
    Type::language(ValueType::String)
}

/// The string `receiver.charAt(index)` defines: one code unit when `index` is proven within the receiver's length, else
/// the empty string or one unit. The result is never longer than one unit whatever the receiver.
pub fn char_at(receiver: Type, index: Type) -> Type {
    let length = receiver.string_fact().map_or(StringLength::FULL, |fact| fact.length());
    if within_length(Length::Fixed(length.min), index) { Type::string_length(1, 1) } else { Type::string_length(0, 1) }
}

/// The largest UTF-16 code unit: the upper bound of a `charCodeAt` result.
const UTF16_UNIT_MAX: i64 = 0xFFFF;

/// The Number `receiver.charCodeAt(index)` defines. An index proven within the receiver's length reads one code unit, an
/// integer in `[0, 65535]`. Any other index may read NaN (`charCodeAt` past the end), and a NaN-free Number range is not
/// representable beside it, so the Number may be NaN.
pub fn char_code_at(receiver: Type, index: Type) -> Type {
    match receiver.string_fact() {
        Some(fact) if within_length(Length::Fixed(fact.length().min), index) => Type::int(IntRange { min: 0, max: UTF16_UNIT_MAX }),
        _ => Type::NUMBER,
    }
}

/// The clamped position hull `index` (an integer range) takes over a string of length `top`. `relative` is `slice`'s
/// mapping, where a negative index counts from the end; otherwise `substring` clamps every index to `[0, top]`. Piecewise
/// monotone, so the extremes are at the endpoints and at `-1` and `0`.
fn position_hull(index: IntRange, top: i64, relative: bool) -> IntRange {
    let map = |i: i64| if !relative { i.clamp(0, top) } else if i < 0 { (top + i).max(0) } else { i.min(top) };
    let mut points = vec![index.min, index.max];
    points.extend([-1, 0].into_iter().filter(|point| index.min <= *point && *point <= index.max));
    let values: Vec<i64> = points.into_iter().map(map).collect();
    IntRange { min: values.iter().copied().min().unwrap_or(0), max: values.iter().copied().max().unwrap_or(0) }
}

/// The string `receiver.substring(start, end)` (`relative` false) or `receiver.slice(start, end)` (`relative` true) defines:
/// a fresh string whose length is at most the clamped end's (the clamped span, for `slice`, never negative). Exact where
/// the receiver's length and both indices are exact; the full range of the receiver's length, else, where an index is not
/// an integer range.
pub fn substring_length(receiver: Type, start: Type, end: Type, relative: bool) -> Type {
    let length = receiver.string_fact().map_or(StringLength::FULL, |fact| fact.length());
    let (Some(start), Some(end)) = (start.int_range(), end.int_range()) else {
        return Type::string_length(0, length.max);
    };
    let top = i64::from(length.max);
    let (position_start, position_end) = (position_hull(start, top, relative), position_hull(end, top, relative));
    let clamp = |value: i64| u32::try_from(value.clamp(0, i64::from(u32::MAX))).unwrap_or(u32::MAX);
    if length.min == length.max && start.min == start.max && end.min == end.max {
        let (from, to) = (position_hull(start, i64::from(length.min), relative).min, position_hull(end, i64::from(length.min), relative).min);
        let exact = if relative { (to - from).max(0) } else { (to - from).abs() };
        return Type::string_length(clamp(exact), clamp(exact));
    }
    let max = if relative { position_end.max.max(0) } else { position_end.max.max(position_start.max) };
    Type::string_length(0, clamp(max))
}

/// Whether `left === right` between two strings is decided by their facts: `Some(true)` where both are one same atom,
/// `Some(false)` where both name atoms and no atom is common (distinct interned atoms are distinct strings) or their
/// length ranges are disjoint, and `None` where the facts leave it open or an operand is not a string.
pub fn string_equality(left: Type, right: Type) -> Option<bool> {
    let (left, right) = (left.string_fact()?, right.string_fact()?);
    let (a, b) = (left.length(), right.length());
    if a.max < b.min || b.max < a.min { return Some(false); }
    if left.atoms().is_empty() || right.atoms().is_empty() { return None; }
    if left.atoms().iter().any(|atom| right.atoms().contains(atom)) {
        return (left.atom().is_some() && left.atom() == right.atom()).then_some(true);
    }
    Some(false)
}

/// Whether both operands are interned atoms: their equality is a pointer compare (`icmp`), since equal atoms are one
/// string object and different atoms are different strings.
pub fn both_interned(left: Type, right: Type) -> bool {
    [left, right].into_iter().all(|ty| ty.string_fact().is_some_and(|fact| !fact.atoms().is_empty()))
}

#[cfg(test)]
mod tests;
