//! The Number rows over tagged `Value` words: one stencil per (kind, destination, operands,
//! speculate) combination, compiled by LLVM from the runtime's own rows. Arithmetic and
//! comparisons are `staged::numeric::step`, the rows the interpreter and the Cranelift tiers
//! run, over the stencil's operand places (`places.rs`); a bitwise row is the interpreter's
//! `NumberBitwise` over `I32BitwiseStage`. What this file adds is the dispatch from a row's
//! codes to those rows (which representation a place's proof asks for) and the rows that
//! compute nothing: moves, guards, tagging and jumps.
//!
//! A row computes into `Out`: continue at the next row, branch to the taken successor, or leave
//! through exit 0 with the registers as they were at the row's start (every proof comes before
//! any write to a pinned register).

use crate::bytecode::Operand;
use crate::places::{DEST, LEFT, Places, RIGHT};
use crate::typed_ir::Representation;
use crate::staged::math::FloatUnary;
use crate::staged::{Concrete, Domain, Frame, NegativeZero, NumberOperation, NumericFrame, Overflow, Step, jump_if as branch_on_truth, logical_not, numeric};
use crate::vm::operations::{I32BitwiseStage, NumberArithmetic, NumberBitwise, NumberRelation, NumberUnary, number_bitwise_f64_result};
pub(crate) use crate::words::*;

pub(crate) use crate::codes::*;
pub(crate) use crate::places::{deliver, index_of, number_word};

const fn arithmetic_of(kind: u8) -> NumberArithmetic {
    match kind {
        ADD => NumberArithmetic::Add,
        SUB => NumberArithmetic::Subtract,
        MUL => NumberArithmetic::Multiply,
        _ => NumberArithmetic::Divide,
    }
}

const fn relation_of(kind: u8) -> NumberRelation {
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
const fn is_double_row<const K: u8, const D: u8, const A: u8, const B: u8>() -> bool {
    (D >= PINF || A >= PINF || B >= PINF) && K <= NE && !is_bitwise(K)
}

/// The row's outcome: continue where the row finished and every result fit its place, else
/// leave with the state as it was.
#[inline(always)]
fn finish<const D: u8, const A: u8, const B: u8>(frame: Places<D, A, B>, done: bool) -> Out {
    if frame.writes_fit(done) { Out::Next(frame.s) } else { Out::Exit(frame.s) }
}

#[inline(always)]
fn jump_if(s: S, fails: bool) -> Out {
    if fails { Out::Taken(s) } else { Out::Next(s) }
}

/// The staged Number operation over the row's places: its word, `None` where it left.
#[inline(always)]
fn staged<const D: u8, const A: u8, const B: u8>(frame: &mut Places<D, A, B>, operation: NumberOperation) -> Option<u64> {
    match numeric::step(&mut Concrete::from_hole(0), frame, operation) {
        Step::Value(word) => Some(word),
        _ => None,
    }
}

/// The staged arithmetic of kind `K` in `domain`; whether it produced its result.
#[inline(always)]
fn arithmetic<const K: u8, const D: u8, const A: u8, const B: u8>(frame: &mut Places<D, A, B>, domain: Domain) -> bool {
    let (left, right) = (Operand::register(LEFT), Operand::register(RIGHT));
    let operation = NumberOperation::Arithmetic { destination: DEST, left, right, operation: arithmetic_of(K), domain, overflow: Overflow::Exit, zero: NegativeZero::Exit };
    staged(frame, operation).is_some()
}

/// The staged relation of kind `K` in `domain`: its predicate, `None` where an operand failed.
#[inline(always)]
fn relation<const K: u8, const D: u8, const A: u8, const B: u8>(frame: &mut Places<D, A, B>, domain: Domain) -> Option<bool> {
    let (left, right) = (Operand::register(LEFT), Operand::register(RIGHT));
    staged(frame, NumberOperation::Relation { left, right, relation: relation_of(K), domain }).map(|word| word != 0)
}

/// An arithmetic row: doubles where a place holds one (and for a division), int32s where the
/// row speculates them, else ints while both operand words are ints and the result is exact,
/// any other Number through doubles.
#[inline(always)]
fn arith_row<const K: u8, const D: u8, const A: u8, const B: u8, const SPEC: bool>(mut frame: Places<D, A, B>) -> Out {
    let done = if K == DIV || is_double_row::<K, D, A, B>() {
        arithmetic::<K, D, A, B>(&mut frame, Domain::Float64)
    } else if SPEC {
        arithmetic::<K, D, A, B>(&mut frame, Domain::Int32)
    } else {
        let ints = frame.both_int();
        frame.prove_ints();
        (ints && arithmetic::<K, D, A, B>(&mut frame, Domain::Int32)) || arithmetic::<K, D, A, B>(&mut frame, Domain::Float64)
    };
    finish(frame, done)
}

/// A comparison row, which branches to the taken successor where the relation fails.
#[inline(always)]
fn compare_row<const K: u8, const D: u8, const A: u8, const B: u8, const SPEC: bool>(mut frame: Places<D, A, B>) -> Out {
    let held = if is_double_row::<K, D, A, B>() {
        relation::<K, D, A, B>(&mut frame, Domain::Float64)
    } else if SPEC {
        relation::<K, D, A, B>(&mut frame, Domain::Int32)
    } else if frame.both_int() {
        frame.prove_ints();
        relation::<K, D, A, B>(&mut frame, Domain::Int32)
    } else {
        relation::<K, D, A, B>(&mut frame, Domain::Float64)
    };
    match held {
        Some(holds) => jump_if(frame.s, !holds),
        None => Out::Exit(frame.s),
    }
}

/// A bitwise row: int32 operands (a speculating row takes int words only, a general row any
/// Number that fits an int32) through the interpreter's `NumberBitwise`.
#[inline(always)]
fn bitwise_row<const K: u8, const D: u8, const A: u8, const B: u8, const SPEC: bool>(mut frame: Places<D, A, B>) -> Out {
    let (Some(x), Some(y)) = (frame.bitwise_operand(LEFT, !SPEC), frame.bitwise_operand(RIGHT, !SPEC)) else {
        return Out::Exit(frame.s);
    };
    let (x, y, operation) = (x as u32 as i32, y as u32 as i32, bitwise_of(K));
    let bits = operation.apply(&mut I32BitwiseStage, x, y);
    let result = if operation.keeps_int32(bits) {
        (u64::from(bits as u32), Representation::Int32)
    } else if holds_int(D) {
        return Out::Exit(frame.s);
    } else {
        // A Number past `i32::MAX` is no int word, so its bits are its tagged word.
        (number_bitwise_f64_result(operation, x, y).to_bits(), Representation::Tagged)
    };
    frame.write(&mut Concrete::from_hole(0), DEST, result.0, result.1);
    finish(frame, true)
}

/// A move or a spill: the first operand into the destination, converted to its representation.
#[inline(always)]
fn copy_row<const D: u8, const A: u8, const B: u8>(mut frame: Places<D, A, B>) -> Out {
    frame.move_register(&mut Concrete::from_hole(0), DEST, LEFT);
    finish(frame, true)
}

/// Branches to the taken successor unless the first operand is what the destination holds: a
/// pinned int32 register takes the int word, a pinned double a Number, an int slot is tested
/// for an int word, any other destination for a Number.
#[inline(always)]
fn guard_row<const D: u8, const A: u8, const B: u8>(mut frame: Places<D, A, B>) -> Out {
    let (mut stage, operand) = (Concrete::from_hole(0), Operand::register(LEFT));
    let proved = match D {
        PIN..=LAST_INT_PIN => frame.int_operand(&mut stage, operand, ()).map(|word| (word, Representation::Int32)),
        MEMINT => frame.int_operand(&mut stage, operand, ()).map(|_| (0, Representation::Tagged)),
        PINF.. => frame.number_operand(&mut stage, operand, ()).map(|bits| (bits, Representation::Float64)),
        _ => frame.prove_number(&mut stage, LEFT, ()).map(|()| (0, Representation::Tagged)),
    };
    let Some((value, rep)) = proved else { return Out::Taken(frame.s) };
    if D != MEMINT && D != MEM {
        frame.write(&mut stage, DEST, value, rep);
    }
    Out::Next(frame.s)
}

/// A taken backward edge counts in the run's counter, word 0 of the loop entry record: hole 1
/// and 2 are the offsets of the context's payload pointer and of the entry pointer in the payload.
/// Whether the count reached zero: a run given a budget starts the counter that far below zero
/// (`LoopEntry::set_backedge_budget`) and leaves there, to move to u-jit; one without starts at
/// zero and never wraps.
#[inline(always)]
fn count_backedge(s: &S) -> bool {
    unsafe {
        let payload = ((s.ctx + hole::<1>()) as *const usize).read();
        let entry = ((payload + hole::<HOLE_B>()) as *const usize).read();
        let count = (entry as *const u64).read().wrapping_add(1);
        (entry as *mut u64).write(count);
        count == 0
    }
}

/// Whether an interrupt was requested: the context's pointer (at hole 0) to a byte, nonzero.
#[inline(always)]
fn interrupted(s: &S) -> bool {
    unsafe { *(((s.ctx + hole::<0>()) as *const *const u8).read()) != 0 }
}

/// A jump that polls the interrupt flag first where `WATCHED`, and counts a taken edge where `COUNT`.
#[inline(always)]
fn jump<const WATCHED: bool, const COUNT: bool>(s: S) -> Out {
    if WATCHED {
        if interrupted(&s) {
            return Out::Exit(s);
        }
        if COUNT && count_backedge(&s) {
            // The run's budget is spent: it leaves at the jump, which the interpreter takes.
            return Out::Exit(s);
        }
    }
    Out::Taken(s)
}

/// Polls the interrupt flag and continues.
#[inline(always)]
fn poll(s: S) -> Out {
    if interrupted(&s) { Out::Exit(s) } else { Out::Next(s) }
}

/// A conditional jump on the first operand's truthiness (`staged::jump_if`): to the taken
/// successor where it is `WHEN`; a value whose heap cell decides it leaves.
#[inline(always)]
fn truth_row<const WHEN: bool, const D: u8, const A: u8, const B: u8>(mut frame: Places<D, A, B>) -> Out {
    match branch_on_truth(&mut Concrete::from_hole(0), &mut frame, LEFT, WHEN) {
        Step::Control(()) if frame.taken => Out::Taken(frame.s),
        Step::Control(()) => Out::Next(frame.s),
        _ => Out::Exit(frame.s),
    }
}

/// The logical negation of the first operand's truthiness into the destination
/// (`staged::logical_not`).
#[inline(always)]
fn not_row<const D: u8, const A: u8, const B: u8>(mut frame: Places<D, A, B>) -> Out {
    let done = matches!(logical_not(&mut Concrete::from_hole(0), &mut frame, DEST, LEFT), Step::Value(_));
    finish(frame, done)
}

/// The Number negation of the first operand, through doubles (`-0` is a double, which an int32
/// zero negates to): the staged `NumberOperation::Unary`.
#[inline(always)]
fn neg_row<const D: u8, const A: u8, const B: u8>(mut frame: Places<D, A, B>) -> Out {
    let operation = NumberOperation::Unary { destination: DEST, source: Operand::register(LEFT), operation: NumberUnary::Negate, domain: Domain::Float64 };
    let done = staged(&mut frame, operation).is_some();
    finish(frame, done)
}

/// The Math function `K` of the first operand, a double: the staged `NumberOperation::Math`.
#[inline(always)]
fn math_row<const K: u8, const D: u8, const A: u8, const B: u8>(mut frame: Places<D, A, B>) -> Out {
    let operation = match K {
        FLOOR => FloatUnary::Floor,
        CEIL => FloatUnary::Ceil,
        TRUNC => FloatUnary::Trunc,
        ABS => FloatUnary::Abs,
        ROUND => FloatUnary::Round,
        _ => FloatUnary::Sqrt,
    };
    let done = staged(&mut frame, NumberOperation::Math { destination: DEST, source: Operand::register(LEFT), operation }).is_some();
    finish(frame, done)
}

/// The tag of a fresh int slot: the upper half of its word.
#[inline(always)]
fn tag_init(s: S) -> Out {
    // SAFETY: the frame slot of hole 0 is writable; the stencil writes its tag half only.
    unsafe { (slot::<0>(&s).wrapping_add(HIGH_HALF_AT) as *mut u32).write(INT_HIGH as u32) };
    Out::Next(s)
}

/// The control rows: a jump (polling the interrupt flag, watching the loop's budget, or neither) and the poll alone.
#[inline(always)]
fn control_row<const K: u8>(s: S) -> Out {
    match K {
        JUMP => jump::<false, false>(s),
        JUMP_POLL => jump::<true, true>(s),
        JUMP_WATCH => jump::<true, false>(s),
        _ => poll(s),
    }
}

/// The staged remainder of the row's operands in `domain`; whether it produced its result.
#[inline(always)]
fn remainder<const D: u8, const A: u8, const B: u8>(frame: &mut Places<D, A, B>, domain: Domain) -> bool {
    let (left, right) = (Operand::register(LEFT), Operand::register(RIGHT));
    staged(frame, NumberOperation::Remainder { destination: DEST, left, right, domain }).is_some()
}

/// A remainder row over int places: the int32 remainder where the row speculates int32s, else
/// while both operand words are ints. Anything else (a zero divisor, a negative zero, a double)
/// leaves: the f64 remainder is libm's `fmod`, a call a stencil cannot hold.
#[inline(always)]
fn remainder_row<const D: u8, const A: u8, const B: u8, const SPEC: bool>(mut frame: Places<D, A, B>) -> Out {
    let done = if SPEC {
        remainder(&mut frame, Domain::Int32)
    } else {
        let ints = frame.both_int();
        frame.prove_ints();
        ints && remainder(&mut frame, Domain::Int32)
    };
    finish(frame, done)
}

/// `Math.min` (`MIN`) or `Math.max` of the two operands, a double.
#[inline(always)]
fn extreme_row<const K: u8, const D: u8, const A: u8, const B: u8>(mut frame: Places<D, A, B>) -> Out {
    let operation = NumberOperation::Extreme { destination: DEST, left: Operand::register(LEFT), right: Operand::register(RIGHT), maximum: K == MAX };
    let done = staged(&mut frame, operation).is_some();
    finish(frame, done)
}

/// `Math.imul` of the two operands, int32s; a general row proves them first, as the remainder does.
#[inline(always)]
fn imul_row<const D: u8, const A: u8, const B: u8, const SPEC: bool>(mut frame: Places<D, A, B>) -> Out {
    let product = |frame: &mut Places<D, A, B>| staged(frame, NumberOperation::Imul { destination: DEST, left: Operand::register(LEFT), right: Operand::register(RIGHT) }).is_some();
    let done = if SPEC {
        product(&mut frame)
    } else {
        let ints = frame.both_int();
        frame.prove_ints();
        ints && product(&mut frame)
    };
    finish(frame, done)
}

/// The unary rows: logical and Number negation.
#[inline(always)]
fn unary_row<const K: u8, const D: u8, const A: u8, const B: u8>(frame: Places<D, A, B>) -> Out {
    match K {
        NOT => not_row(frame),
        _ => neg_row(frame),
    }
}

/// One row, by kind and places. `SPEC` rows take int32 operands only.
#[inline(always)]
pub(crate) fn run<const K: u8, const D: u8, const A: u8, const B: u8, const SPEC: bool>(s: S) -> Out {
    let frame = Places::<D, A, B>::new(s);
    match K {
        MOVE | COPY | SPILL => copy_row(frame),
        ADD..=DIV => arith_row::<K, D, A, B, SPEC>(frame),
        AND..=USHR => bitwise_row::<K, D, A, B, SPEC>(frame),
        LT..=NE => compare_row::<K, D, A, B, SPEC>(frame),
        JUMP | JUMP_POLL | JUMP_WATCH | POLL => control_row::<K>(frame.s),
        GUARD => guard_row(frame),
        JUMP_TRUE => truth_row::<true, D, A, B>(frame),
        JUMP_FALSE => truth_row::<false, D, A, B>(frame),
        NOT | NEG => unary_row::<K, D, A, B>(frame),
        REM => remainder_row::<D, A, B, SPEC>(frame),
        MATH_FIRST..=MATH_LAST => math_row::<K, D, A, B>(frame),
        MIN | MAX => extreme_row::<K, D, A, B>(frame),
        IMUL => imul_row::<D, A, B, SPEC>(frame),
        _ => tag_init(frame.s),
    }
}
