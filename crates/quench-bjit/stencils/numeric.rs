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
use crate::places::{DEST, LEFT, Places, RIGHT, Rep};
use crate::staged::{Concrete, Domain, Frame, NumberOperation, NumericFrame, Overflow, Step, numeric};
use crate::vm::operations::{I32BitwiseStage, NumberArithmetic, NumberBitwise, NumberRelation, number_bitwise_f64_result};
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
    let operation = NumberOperation::Arithmetic { destination: DEST, left, right, operation: arithmetic_of(K), domain, overflow: Overflow::Exit };
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
        (u64::from(bits as u32), Rep::Int32)
    } else if holds_int(D) {
        return Out::Exit(frame.s);
    } else {
        // A Number past `i32::MAX` is no int word, so its bits are its tagged word.
        (number_bitwise_f64_result(operation, x, y).to_bits(), Rep::Tagged)
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
        PIN..=LAST_INT_PIN => frame.int_operand(&mut stage, operand, ()).map(|word| (word, Rep::Int32)),
        MEMINT => frame.int_operand(&mut stage, operand, ()).map(|_| (0, Rep::Tagged)),
        PINF.. => frame.number_operand(&mut stage, operand, ()).map(|bits| (bits, Rep::Float64)),
        _ => frame.prove_number(&mut stage, LEFT, ()).map(|()| (0, Rep::Tagged)),
    };
    let Some((value, rep)) = proved else { return Out::Taken(frame.s) };
    if D != MEMINT && D != MEM {
        frame.write(&mut stage, DEST, value, rep);
    }
    Out::Next(frame.s)
}

/// A taken backward edge counts in the run's counter, word 0 of the loop entry record: hole 1
/// and 2 are the offsets of the context's payload pointer and of the entry pointer in the payload.
#[inline(always)]
fn count_backedge(s: &S) {
    unsafe {
        let payload = ((s.ctx + hole::<1>()) as *const usize).read();
        let entry = ((payload + hole::<HOLE_B>()) as *const usize).read();
        let count = (entry as *const u64).read();
        (entry as *mut u64).write(count.wrapping_add(1));
    }
}

#[inline(always)]
fn jump<const POLL: bool>(s: S) -> Out {
    if POLL {
        // The interrupt flag: the context's pointer to a byte, nonzero once requested.
        let byte = unsafe { *(((s.ctx + hole::<0>()) as *const *const u8).read()) };
        if byte != 0 {
            return Out::Exit(s);
        }
        count_backedge(&s);
    }
    Out::Taken(s)
}

/// The tag of a fresh int slot: the upper half of its word.
#[inline(always)]
fn tag_init(s: S) -> Out {
    // SAFETY: the frame slot of hole 0 is writable; the stencil writes its tag half only.
    unsafe { (slot::<0>(&s).wrapping_add(HIGH_HALF_AT) as *mut u32).write(INT_HIGH as u32) };
    Out::Next(s)
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
        JUMP => jump::<false>(frame.s),
        JUMP_POLL => jump::<true>(frame.s),
        GUARD => guard_row(frame),
        _ => tag_init(frame.s),
    }
}
