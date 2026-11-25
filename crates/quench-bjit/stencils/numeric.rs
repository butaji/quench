//! The Number rows over tagged `Value` words: one stencil per (kind, destination, operands,
//! speculate) combination, compiled by LLVM from the runtime's own rows. Every row that the
//! interpreter decodes to a staged `Operation` runs that operation's step over the stencil's
//! operand places (`places.rs`): arithmetic, relations, bitwise, negation and the Number-valued
//! moves, jumps, truthiness and polls. What this file adds is the dispatch from a row's codes to
//! those operations (which representation a place's proof asks for), and the rows that have no
//! staged operation: the guard that proves a representation and the tag of a fresh int slot.
//!
//! A row computes into `Out`: continue at the next row, branch to the taken successor, or leave
//! through exit 0 with the registers as they were at the row's start (every proof comes before
//! any write to a pinned register).

use crate::bytecode::Operand;
use crate::places::{DEST, LEFT, Places, RIGHT, finish};
use crate::typed_ir::Representation;
use crate::staged::math::FloatUnary;
use crate::staged::{Comparison, Concrete, Domain, Int32, Frame, NegativeZero, NumberOperation, NumericFrame, Operation, Overflow, Step, numeric, op_kind, step_arith_of, step_flow_of, step_generic_of, step_of};
use crate::vm::operations::{I32BitwiseStage, NumberArithmetic, NumberBitwise, NumberRelation, NumberUnary, number_bitwise_f64_result};
pub(crate) use crate::words::*;

pub(crate) use crate::codes::*;
pub(crate) use crate::places::{deliver, index_of, number_word, read_place, widen};

const fn arithmetic_of(kind: u8) -> NumberArithmetic {
    match kind {
        ADD | ADD_NO_OVERFLOW => NumberArithmetic::Add,
        SUB | SUB_NO_OVERFLOW => NumberArithmetic::Subtract,
        MUL => NumberArithmetic::Multiply,
        _ => NumberArithmetic::Divide,
    }
}

pub(crate) const fn relation_of(kind: u8) -> NumberRelation {
    match kind {
        LT => NumberRelation::LessThan,
        LE => NumberRelation::LessEqual,
        GT => NumberRelation::GreaterThan,
        GE => NumberRelation::GreaterEqual,
        EQ => NumberRelation::Equal,
        _ => NumberRelation::NotEqual,
    }
}

const fn bitwise_of(kind: u8) -> NumberBitwise {
    match kind {
        AND => NumberBitwise::And,
        OR => NumberBitwise::Or,
        XOR => NumberBitwise::Xor,
        SHL => NumberBitwise::ShiftLeft,
        SHR => NumberBitwise::ShiftRight,
        _ => NumberBitwise::ShiftRightUnsigned,
    }
}

/// A row over a pinned double, other than a bitwise row (which reads a double as its ToInt32).
/// Whether one of a row's places is a pinned double, so the row computes in binary64.
pub(crate) const fn any_double(d: u8, a: u8, b: u8) -> bool {
    d >= PINF || a >= PINF || b >= PINF
}

const fn is_double_row<const K: u8, const D: u8, const A: u8, const B: u8>() -> bool {
    any_double(D, A, B) && K <= NE && !is_bitwise(K)
}

/// The outcome of a staged step that decides a branch: the taken successor where the frame took
/// the branch, the next row where it did not, and an exit where the step left.
#[inline(always)]
fn branched(decided: Step<u64, ()>, frame: Places) -> Out {
    match decided {
        Step::Control(()) if frame.taken => Out::Taken(frame.s),
        Step::Control(()) => Out::Next(frame.s),
        _ => Out::Exit(frame.s),
    }
}

/// The variant `K` of the `NumberOperation` a row runs (`numeric::kind`), so `staged` instantiates only its arm.
pub(crate) struct Kind<const K: u8>;

/// The staged Number operation over the row's places: its word, `None` where it left.
#[inline(always)]
fn staged<const KIND: u8>(frame: &mut Places, operation: NumberOperation, _: Kind<KIND>) -> Option<u64> {
    match numeric::step_of::<KIND, _, _>(&mut Concrete::from_hole(0), frame, operation) {
        Step::Value(word) => Some(word),
        _ => None,
    }
}

/// The staged arithmetic of kind `K` in `domain`; whether it produced its result.
#[inline(always)]
fn arithmetic<const K: u8>(frame: &mut Places, domain: Domain) -> bool {
    let (left, right) = (Operand::register(LEFT), Operand::register(RIGHT));
    let operation = NumberOperation::Arithmetic { destination: DEST, left, right, operation: arithmetic_of(K), domain, overflow: Overflow::Exit, zero: NegativeZero::Exit };
    staged(frame, operation, Kind::<{ numeric::kind::ARITHMETIC }>).is_some()
}

/// An arithmetic row: doubles where a place holds one (and for a division), int32s where the
/// row speculates them, else ints while both operand words are ints and the result is exact,
/// any other Number through doubles.
#[inline(always)]
fn arith_row<const K: u8, const D: u8, const A: u8, const B: u8, const SPEC: bool>(mut frame: Places) -> Out {
    let done = if const { K == DIV || is_double_row::<K, D, A, B>() } {
        arithmetic::<K>(&mut frame, Domain::Float64)
    } else if SPEC {
        arithmetic::<K>(&mut frame, Domain::Int32)
    } else {
        let ints = frame.prove_ints();
        (ints && arithmetic::<K>(&mut frame, Domain::Int32)) || arithmetic::<K>(&mut frame, Domain::Float64)
    };
    finish(frame, done)
}

/// An int32 `+` or `-` the planner proved exact (`Overflow::Impossible`): the staged int32 step with no overflow
/// test, over int32 operands only (any other leaves through exit 0 before any write).
#[inline(always)]
fn exact_arith_row<const K: u8>(mut frame: Places) -> Out {
    let (left, right) = (Operand::register(LEFT), Operand::register(RIGHT));
    let operation = NumberOperation::Arithmetic { destination: DEST, left, right, operation: arithmetic_of(K), domain: Domain::Int32, overflow: Overflow::Impossible, zero: NegativeZero::Exit };
    let done = staged(&mut frame, operation, Kind::<{ numeric::kind::ARITHMETIC }>).is_some();
    finish(frame, done)
}

/// A comparison row, which branches to the taken successor where the relation fails: the
/// interpreter's `CompareJump` of a Number relation, over the domain the row's places prove.
#[inline(always)]
fn compare_row<const K: u8, const D: u8, const A: u8, const B: u8, const SPEC: bool>(mut frame: Places) -> Out {
    let domain = if const { is_double_row::<K, D, A, B>() } {
        Domain::Float64
    } else if SPEC {
        Domain::Int32
    } else if frame.prove_ints() {
        Domain::Int32
    } else {
        Domain::Float64
    };
    compare_jump(frame, Comparison::Relation { relation: relation_of(K), domain })
}

/// The staged `CompareJump` of `comparison` over the row's registers LEFT and RIGHT, negated: it branches to the taken
/// successor where the comparison fails. Every jump row runs it, so each test is the staged step's own.
#[inline(always)]
fn compare_jump(mut frame: Places, comparison: Comparison) -> Out {
    let operation = Operation::CompareJump { comparison, left: Operand::register(LEFT), right: Operand::register(RIGHT), negate: true };
    branched(step_generic_of::<{ op_kind::COMPARE_JUMP }, _, _>(&mut Concrete::from_hole(0), &mut frame, operation), frame)
}

/// `if (a & b)`: the staged bitwise test of the two operands' ToInt32 (`Comparison::Bitwise`).
#[inline(always)]
fn bit_test_row(frame: Places) -> Out {
    compare_jump(frame, Comparison::Bitwise(NumberBitwise::And))
}

/// `a == null` or `a == undefined` (`NEGATED`: `!=`), loosely: the tested operand's tag decides it, and the row reads
/// no second operand.
#[inline(always)]
fn nullish_row<const NEGATED: bool>(frame: Places) -> Out {
    compare_jump(frame, Comparison::Nullish { expected: crate::ValueType::Null, strict: false, negated: NEGATED })
}

/// `a === true` (`EXPECTED`) or `a === false`: the identity of the tested word.
#[inline(always)]
fn boolean_row<const EXPECTED: bool>(frame: Places) -> Out {
    compare_jump(frame, Comparison::Boolean { expected: EXPECTED, negated: false })
}

/// A bitwise row: the interpreter's `NumberBitwise` of two operands' ToInt32, defining an int32
/// (`NumberOperation::Bitwise`). The one result no int32 holds is a zero-fill shift of a negative
/// dividend by a zero count, which the staged step leaves: a destination that holds no int32 takes
/// that Number here, its operands proved once more on this leave path only.
#[inline(always)]
fn bitwise_row<const K: u8, const D: u8, const A: u8>(mut frame: Places) -> Out {
    let (left, right, operation) = (Operand::register(LEFT), Operand::register(RIGHT), bitwise_of(K));
    let done = staged(&mut frame, NumberOperation::Bitwise { destination: DEST, left, right, operation }, Kind::<{ numeric::kind::BITWISE }>).is_some();
    if done || holds_int(D) || operation != NumberBitwise::ShiftRightUnsigned {
        return finish(frame, done);
    }
    let mut stage = Concrete::from_hole(0);
    let (Some(x), Some(y)) = (frame.int32_operand(&mut stage, left, ()).map(Int32::word), frame.int32_operand(&mut stage, right, ()).map(Int32::word)) else {
        return Out::Exit(frame.s);
    };
    let (x, y) = (x as u32 as i32, y as u32 as i32);
    let bits = operation.apply(&mut I32BitwiseStage, x, y);
    let result = if operation.keeps_int32(bits) {
        (u64::from(bits as u32), Representation::Int32)
    } else {
        // A Number past `i32::MAX` is no int word, so its bits are its tagged word.
        (number_bitwise_f64_result(operation, x, y).to_bits(), Representation::Tagged)
    };
    frame.write(&mut stage, DEST, result.0, result.1);
    finish(frame, true)
}

/// A move or a spill: the first operand into the destination, converted to its representation
/// (`Operation::Move`).
#[inline(always)]
fn copy_row(mut frame: Places) -> Out {
    let copied = step_flow_of::<{ op_kind::MOVE }, _, _>(&mut Concrete::from_hole(0), &mut frame, Operation::Move { destination: DEST, source: LEFT });
    finish(frame, matches!(copied, Step::Value(_)))
}

/// Branches to the taken successor unless the first operand is what the destination holds: a
/// pinned int32 register takes the int word, a pinned double a Number, an int slot is tested
/// for an int word, any other destination for a Number.
#[inline(always)]
fn guard_row<const D: u8>(mut frame: Places) -> Out {
    let (mut stage, operand) = (Concrete::from_hole(0), Operand::register(LEFT));
    let proved = match D {
        PIN..=LAST_INT_PIN => frame.int_operand(&mut stage, operand, ()).map(|word| (word.word(), Representation::Int32)),
        MEMINT => frame.int_operand(&mut stage, operand, ()).map(|_| (0, Representation::Tagged)),
        PINF.. => frame.number_operand(&mut stage, operand, ()).map(|bits| (bits.word(), Representation::Float64)),
        _ => frame.prove_number(&mut stage, LEFT, ()).map(|()| (0, Representation::Tagged)),
    };
    let Some((value, rep)) = proved else { return Out::Taken(frame.s) };
    if D != MEMINT && D != MEM {
        frame.write(&mut stage, DEST, value, rep);
    }
    Out::Next(frame.s)
}

/// `guard_row` with its tag test left out (`UNBOX`): a pinned double takes the frame slot's word as a binary64 (an int
/// word widened). Only where the caller's argument proof (`CallRecord::proven_args`) holds the slot a Number, which is
/// what makes the test redundant.
#[inline(always)]
fn unbox_row(mut frame: Places) -> Out {
    let word = read_place::<MEM, { LEFT as u8 }>(&frame.s);
    let value = if is_int(word) { widen(word) } else { word };
    frame.write(&mut Concrete::from_hole(0), DEST, value, Representation::Float64);
    Out::Next(frame.s)
}

/// `guard_row` with its tag test left out (`UNBOX_INT`): a pinned int32 takes the frame slot's low word. Only where the
/// caller's argument proof holds the slot an int32 (`CallRecord::proven_args`'s int32 bits).
#[inline(always)]
fn unbox_int_row(mut frame: Places) -> Out {
    let word = read_place::<MEM, { LEFT as u8 }>(&frame.s);
    frame.write(&mut Concrete::from_hole(0), DEST, word as u32 as u64, Representation::Int32);
    Out::Next(frame.s)
}

/// Whether an interrupt was requested: the staged `gated_poll` row over the process-wide attention
/// byte (hole 3, an absolute word the planner patches with `execution_interrupt::attention_address`)
/// and the run's own byte (the context's pointer at hole 0). The frame's `safepoint` answers it for
/// the staged `Operation::Safepoint`.
#[inline(always)]
pub(crate) fn interrupted(s: &S) -> bool {
    crate::staged::poll::gated_poll(&mut Concrete::from_hole(0), hole64::<HOLE_FOURTH>(), (s.ctx + hole::<0>()) as u64, ()).is_none()
}

/// The staged `Operation::Safepoint` over the row's places: whether the row may continue.
#[inline(always)]
fn poll_step(frame: &mut Places) -> bool {
    matches!(step_of::<{ op_kind::SAFEPOINT }, _, _>(&mut Concrete::from_hole(0), frame, Operation::Safepoint { proof: () }), Step::Value(_))
}

/// The staged `Operation::BackEdge` over the row's places: polls the interrupt flag, then counts the taken edge.
#[inline(always)]
fn back_edge_step(frame: &mut Places) -> bool {
    matches!(step_of::<{ op_kind::BACK_EDGE }, _, _>(&mut Concrete::from_hole(0), frame, Operation::BackEdge { proof: () }), Step::Value(_))
}

/// A jump (`Operation::Jump`) that polls the interrupt flag first where `WATCHED`, and counts a
/// taken edge where `COUNT` (`Operation::BackEdge`).
#[inline(always)]
fn jump_row<const WATCHED: bool, const COUNT: bool>(mut frame: Places) -> Out {
    let proceed = match (WATCHED, COUNT) {
        (true, true) => back_edge_step(&mut frame),
        (true, false) => poll_step(&mut frame),
        (false, _) => true,
    };
    if !proceed {
        return Out::Exit(frame.s);
    }
    match step_flow_of::<{ op_kind::JUMP }, _, _>(&mut Concrete::from_hole(0), &mut frame, Operation::Jump) {
        Step::Control(()) => Out::Taken(frame.s),
        _ => Out::Exit(frame.s),
    }
}

/// Polls the interrupt flag (`Operation::Safepoint`) and continues.
#[inline(always)]
fn poll_row(mut frame: Places) -> Out {
    if poll_step(&mut frame) { Out::Next(frame.s) } else { Out::Exit(frame.s) }
}

/// The control rows: a jump (polling the interrupt flag, watching the loop's budget, or neither)
/// and the poll alone.
#[inline(always)]
fn control_row<const K: u8, const D: u8, const A: u8, const B: u8>(frame: Places) -> Out {
    match K {
        JUMP => jump_row::<false, false>(frame),
        JUMP_POLL => jump_row::<true, true>(frame),
        JUMP_WATCH => jump_row::<true, false>(frame),
        _ => poll_row(frame),
    }
}

/// A conditional jump on the first operand's truthiness (`Operation::JumpIf`): to the taken
/// successor where it is `WHEN`; a value whose heap cell decides it leaves.
#[inline(always)]
fn truth_row<const WHEN: bool>(mut frame: Places) -> Out {
    let jumped = step_flow_of::<{ op_kind::JUMP_IF }, _, _>(&mut Concrete::from_hole(0), &mut frame, Operation::JumpIf { register: LEFT, taken: WHEN });
    branched(jumped, frame)
}

/// The logical negation of the first operand's truthiness into the destination
/// (`Operation::LogicalNot`).
#[inline(always)]
fn not_row(mut frame: Places) -> Out {
    let negated = step_arith_of::<{ op_kind::LOGICAL_NOT }, _, _>(&mut Concrete::from_hole(0), &mut frame, Operation::LogicalNot { destination: DEST, source: LEFT });
    finish(frame, matches!(negated, Step::Value(_)))
}

/// The Number negation of the first operand, through doubles (`-0` is a double, which an int32
/// zero negates to): the staged `NumberOperation::Unary`.
#[inline(always)]
fn neg_row(mut frame: Places) -> Out {
    let operation = NumberOperation::Unary { destination: DEST, source: Operand::register(LEFT), operation: NumberUnary::Negate, domain: Domain::Float64 };
    let done = staged(&mut frame, operation, Kind::<{ numeric::kind::UNARY }>).is_some();
    finish(frame, done)
}

/// The Math function `K` of the first operand, a double: the staged `NumberOperation::Math`.
#[inline(always)]
fn math_row<const K: u8>(mut frame: Places) -> Out {
    let operation = match K {
        FLOOR => FloatUnary::Floor,
        CEIL => FloatUnary::Ceil,
        TRUNC => FloatUnary::Trunc,
        ABS => FloatUnary::Abs,
        ROUND => FloatUnary::Round,
        FROUND => FloatUnary::Fround,
        SIGN => FloatUnary::Sign,
        _ => FloatUnary::Sqrt,
    };
    let done = staged(&mut frame, NumberOperation::Math { destination: DEST, source: Operand::register(LEFT), operation }, Kind::<{ numeric::kind::MATH }>).is_some();
    finish(frame, done)
}

/// The tag of a fresh int slot: the upper half of its word. No staged operation writes a slot's
/// tag half alone; the stencil's own representation write.
#[inline(always)]
fn tag_init(s: S) -> Out {
    // SAFETY: the frame slot of hole 0 is writable; the stencil writes its tag half only.
    unsafe { (slot::<0>(&s).wrapping_add(HIGH_HALF_AT) as *mut u32).write(INT_HIGH as u32) };
    Out::Next(s)
}

/// The staged remainder of the row's operands in `domain`; whether it produced its result.
#[inline(always)]
fn remainder(frame: &mut Places, domain: Domain) -> bool {
    let (left, right) = (Operand::register(LEFT), Operand::register(RIGHT));
    staged(frame, NumberOperation::Remainder { destination: DEST, left, right, domain, zero: NegativeZero::Exit,
        divisor_nonzero: false }, Kind::<{ numeric::kind::REMAINDER }>).is_some()
}

/// A remainder row: the int32 remainder where the row speculates int32s or both operands are ints, else the staged
/// Float64 remainder, whose exact integer path (`integral_remainder`) answers integral operands and misses on the rest, so
/// the f64 `fmod` (a call a stencil cannot hold) is never reached from a stencil.
#[inline(always)]
fn remainder_row<const SPEC: bool>(mut frame: Places) -> Out {
    let done = if SPEC {
        remainder(&mut frame, Domain::Int32)
    } else {
        let ints = frame.prove_ints();
        if ints { remainder(&mut frame, Domain::Int32) } else { remainder(&mut frame, Domain::Float64) }
    };
    finish(frame, done)
}

/// `Math.min` (`MIN`) or `Math.max` of the two operands, a double.
#[inline(always)]
fn extreme_row<const K: u8>(mut frame: Places) -> Out {
    let operation = NumberOperation::Extreme { destination: DEST, left: Operand::register(LEFT), right: Operand::register(RIGHT), maximum: K == MAX };
    let done = staged(&mut frame, operation, Kind::<{ numeric::kind::EXTREME }>).is_some();
    finish(frame, done)
}

/// `Math.imul` of the two operands, int32s; a general row proves them first, as the remainder does.
#[inline(always)]
fn imul_row<const SPEC: bool>(mut frame: Places) -> Out {
    // A function, not a closure: a closure can't be marked always-inline, and since the frame's places are
    // run-time fields every row of the shard shares it, so LLVM kept it as a call.
    #[inline(always)]
    fn product(frame: &mut Places) -> bool {
        staged(frame, NumberOperation::Imul { destination: DEST, left: Operand::register(LEFT), right: Operand::register(RIGHT) }, Kind::<{ numeric::kind::IMUL }>).is_some()
    }
    let done = if SPEC {
        product(&mut frame)
    } else {
        let ints = frame.prove_ints();
        ints && product(&mut frame)
    };
    finish(frame, done)
}

/// `Math.clz32` of the first operand, an int32 (0..=32): `staged::math::leading_zeros` of its ToUint32. An int32 is its own bits;
/// a Number whose ToUint32 is exact (`value::exact_uint32`, the one conversion the interpreter's `number_to_u32` also
/// starts from) takes that. Any other word leaves with nothing written, for the interpreter's own conversion.
#[inline(always)]
fn clz32_row(mut frame: Places) -> Out {
    let (mut stage, operand) = (Concrete::from_hole(0), Operand::register(LEFT));
    let uint32 = match frame.int_operand(&mut stage, operand, ()).map(Int32::word) {
        Some(word) => Some(word as u32),
        None => frame.number_operand(&mut stage, operand, ()).and_then(|bits| crate::value::exact_uint32(f64::from_bits(bits.word()))),
    };
    let Some(uint32) = uint32 else { return Out::Exit(frame.s) };
    frame.write(&mut stage, DEST, u64::from(crate::staged::math::leading_zeros(uint32)), Representation::Int32);
    finish(frame, true)
}

/// `switch`'s case slot (`NumberOperation::IntCase`): the minimum is the second operand's immediate (hole 2), the maximum hole 3
/// (the planner's `Decoded::hole`). The slot is written to the destination; every value is decided, so the row never leaves.
#[inline(always)]
fn int_case_row(mut frame: Places) -> Out {
    let minimum = hole::<HOLE_B>() as u32 as i32;
    let maximum = hole64::<HOLE_FOURTH>() as u32 as i32;
    let count = (i64::from(maximum) - i64::from(minimum) + 1) as u16;
    let operation = NumberOperation::IntCase { destination: DEST, source: Operand::register(LEFT), minimum, maximum, count };
    let done = staged(&mut frame, operation, Kind::<{ numeric::kind::INT_CASE }>).is_some();
    finish(frame, done)
}

/// The unary rows: logical and Number negation.
#[inline(always)]
fn unary_row<const K: u8>(frame: Places) -> Out {
    match K {
        NOT => not_row(frame),
        _ => neg_row(frame),
    }
}

/// One row, by kind and places. `SPEC` rows take int32 operands only.
#[inline(always)]
pub(crate) fn run<const K: u8, const D: u8, const A: u8, const B: u8, const SPEC: bool>(s: S) -> Out {
    let frame = Places::new::<D, A, B>(s);
    // `if const`: rustc drops every other arm before code generation, so the row instantiates its own arm only.
    if const { K == MOVE || K == COPY || K == SPILL } {
        copy_row(frame)
    } else if const { (ADD <= K && K <= DIV) } {
        arith_row::<K, D, A, B, SPEC>(frame)
    } else if const { K == ADD_NO_OVERFLOW || K == SUB_NO_OVERFLOW } {
        exact_arith_row::<K>(frame)
    } else if const { (AND <= K && K <= USHR) } {
        bitwise_row::<K, D, A>(frame)
    } else if const { (LT <= K && K <= NE) } {
        compare_row::<K, D, A, B, SPEC>(frame)
    } else if const { K == JUMP || K == JUMP_POLL || K == JUMP_WATCH || K == POLL } {
        control_row::<K, D, A, B>(frame)
    } else if const { K == GUARD } {
        guard_row::<D>(frame)
    } else if const { K == UNBOX } {
        unbox_row(frame)
    } else if const { K == UNBOX_INT } {
        unbox_int_row(frame)
    } else if const { K == JUMP_TRUE } {
        truth_row::<true>(frame)
    } else if const { K == JUMP_FALSE } {
        truth_row::<false>(frame)
    } else if const { K == NOT || K == NEG } {
        unary_row::<K>(frame)
    } else if const { K == REM } {
        remainder_row::<SPEC>(frame)
    } else if const { (MATH_FIRST <= K && K <= MATH_LAST) } {
        math_row::<K>(frame)
    } else if const { K == MIN || K == MAX } {
        extreme_row::<K>(frame)
    } else if const { K == IMUL } {
        imul_row::<SPEC>(frame)
    } else if const { K == CLZ32 } {
        clz32_row(frame)
    } else if const { K == INT_CASE } {
        int_case_row(frame)
    } else if const { K == BIT_TEST } {
        bit_test_row(frame)
    } else if const { K == NULLISH_EQ } {
        nullish_row::<false>(frame)
    } else if const { K == NULLISH_NE } {
        nullish_row::<true>(frame)
    } else if const { K == BOOL_TRUE } {
        boolean_row::<true>(frame)
    } else if const { K == BOOL_FALSE } {
        boolean_row::<false>(frame)
    } else {
        tag_init(frame.s)
    }
}
