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
use crate::places::{DEST, LEFT, Places, RIGHT};
use crate::typed_ir::Representation;
use crate::staged::math::FloatUnary;
use crate::staged::{Comparison, Concrete, Domain, Frame, NegativeZero, NumberOperation, NumericFrame, Operation, Overflow, Step, numeric, step, step_arith, step_flow, step_generic};
use crate::vm::operations::{NumberArithmetic, NumberBitwise, NumberRelation, NumberUnary, number_bitwise_f64_result};
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

/// The outcome of a staged step that decides a branch: the taken successor where the frame took
/// the branch, the next row where it did not, and an exit where the step left.
#[inline(always)]
fn branched<const D: u8, const A: u8, const B: u8>(decided: Step<u64, ()>, frame: Places<D, A, B>) -> Out {
    match decided {
        Step::Control(()) if frame.taken => Out::Taken(frame.s),
        Step::Control(()) => Out::Next(frame.s),
        _ => Out::Exit(frame.s),
    }
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

/// A comparison row, which branches to the taken successor where the relation fails: the
/// interpreter's `CompareJump` of a Number relation, over the domain the row's places prove.
#[inline(always)]
fn compare_row<const K: u8, const D: u8, const A: u8, const B: u8, const SPEC: bool>(mut frame: Places<D, A, B>) -> Out {
    let domain = if is_double_row::<K, D, A, B>() {
        Domain::Float64
    } else if SPEC {
        Domain::Int32
    } else if frame.both_int() {
        frame.prove_ints();
        Domain::Int32
    } else {
        Domain::Float64
    };
    let comparison = Comparison::Relation { relation: relation_of(K), domain };
    let operation = Operation::CompareJump { comparison, left: Operand::register(LEFT), right: Operand::register(RIGHT), negate: true };
    branched(step_generic(&mut Concrete::from_hole(0), &mut frame, operation), frame)
}

/// A bitwise row: the interpreter's `NumberBitwise` of two operands' ToInt32, defining an int32
/// (`NumberOperation::Bitwise`). The one result no int32 holds is a zero-fill shift of a negative
/// dividend by a zero count, which the staged step leaves: a destination that holds no int32 takes
/// that Number here, its operands proved once more on this leave path only.
#[inline(always)]
fn bitwise_row<const K: u8, const D: u8, const A: u8, const B: u8>(mut frame: Places<D, A, B>) -> Out {
    let (left, right, operation) = (Operand::register(LEFT), Operand::register(RIGHT), bitwise_of(K));
    let done = staged(&mut frame, NumberOperation::Bitwise { destination: DEST, left, right, operation }).is_some();
    if done || holds_int(D) || operation != NumberBitwise::ShiftRightUnsigned {
        return finish(frame, done);
    }
    let mut stage = Concrete::from_hole(0);
    let (Some(x), Some(y)) = (frame.int32_operand(&mut stage, left, ()), frame.int32_operand(&mut stage, right, ())) else {
        return Out::Exit(frame.s);
    };
    // A Number past `i32::MAX` is no int word, so its bits are its tagged word.
    let widened = number_bitwise_f64_result(operation, x as u32 as i32, y as u32 as i32).to_bits();
    frame.write(&mut stage, DEST, widened, Rep::Tagged);
    finish(frame, true)
}

/// A move or a spill: the first operand into the destination, converted to its representation
/// (`Operation::Move`).
#[inline(always)]
fn copy_row<const D: u8, const A: u8, const B: u8>(mut frame: Places<D, A, B>) -> Out {
    let copied = step_flow(&mut Concrete::from_hole(0), &mut frame, Operation::Move { destination: DEST, source: LEFT });
    finish(frame, matches!(copied, Step::Value(_)))
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
/// The frame's `safepoint` answers it for the staged `Operation::Safepoint`.
#[inline(always)]
pub(crate) fn interrupted(s: &S) -> bool {
    unsafe { *(((s.ctx + hole::<0>()) as *const *const u8).read()) != 0 }
}

/// The staged `Operation::Safepoint` over the row's places: whether the row may continue.
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
    if WATCHED && COUNT {
        count_backedge(&frame.s);
    }
    match step_flow(&mut Concrete::from_hole(0), &mut frame, Operation::Jump) {
        Step::Control(()) => Out::Taken(frame.s),
        _ => Out::Exit(frame.s),
    }
}

/// Polls the interrupt flag (`Operation::Safepoint`) and continues.
#[inline(always)]
fn poll_row<const D: u8, const A: u8, const B: u8>(mut frame: Places<D, A, B>) -> Out {
    if poll_step(&mut frame) { Out::Next(frame.s) } else { Out::Exit(frame.s) }
}

/// The control rows: a jump (polling the interrupt flag, watching the loop's budget, or neither)
/// and the poll alone.
#[inline(always)]
fn control_row<const K: u8, const D: u8, const A: u8, const B: u8>(frame: Places<D, A, B>) -> Out {
    match K {
        JUMP => jump_row::<false, false, D, A, B>(frame),
        JUMP_POLL => jump_row::<true, true, D, A, B>(frame),
        JUMP_WATCH => jump_row::<true, false, D, A, B>(frame),
        _ => poll_row(frame),
    }
}

/// A conditional jump on the first operand's truthiness (`Operation::JumpIf`): to the taken
/// successor where it is `WHEN`; a value whose heap cell decides it leaves.
#[inline(always)]
fn truth_row<const WHEN: bool, const D: u8, const A: u8, const B: u8>(mut frame: Places<D, A, B>) -> Out {
    let jumped = step_flow(&mut Concrete::from_hole(0), &mut frame, Operation::JumpIf { register: LEFT, taken: WHEN });
    branched(jumped, frame)
}

/// The logical negation of the first operand's truthiness into the destination
/// (`Operation::LogicalNot`).
#[inline(always)]
fn not_row<const D: u8, const A: u8, const B: u8>(mut frame: Places<D, A, B>) -> Out {
    let negated = step_arith(&mut Concrete::from_hole(0), &mut frame, Operation::LogicalNot { destination: DEST, source: LEFT });
    finish(frame, matches!(negated, Step::Value(_)))
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

/// `Math.clz32` of the first operand, an int32 (0..=32): `staged::math::leading_zeros` of its ToUint32. An int32 is its own bits;
/// a Number whose truncation is exact (below 2^63 in magnitude, `value::number_to_u32`'s first step) takes the low 32 bits of
/// it. Any other word leaves with nothing written, for the interpreter's own conversion.
#[inline(always)]
fn clz32_row<const D: u8, const A: u8, const B: u8>(mut frame: Places<D, A, B>) -> Out {
    let (mut stage, operand) = (Concrete::from_hole(0), Operand::register(LEFT));
    let uint32 = match frame.int_operand(&mut stage, operand, ()) {
        Some(word) => Some(word as u32),
        None => frame.number_operand(&mut stage, operand, ()).and_then(|bits| {
            let wide = truncate64(f64::from_bits(bits));
            (wide != i64::MIN).then_some(wide as u32)
        }),
    };
    let Some(uint32) = uint32 else { return Out::Exit(frame.s) };
    frame.write(&mut stage, DEST, u64::from(crate::staged::math::leading_zeros(uint32)), Rep::Int32);
    finish(frame, true)
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
        AND..=USHR => bitwise_row::<K, D, A, B>(frame),
        LT..=NE => compare_row::<K, D, A, B, SPEC>(frame),
        JUMP | JUMP_POLL | JUMP_WATCH | POLL => control_row::<K, D, A, B>(frame),
        GUARD => guard_row(frame),
        JUMP_TRUE => truth_row::<true, D, A, B>(frame),
        JUMP_FALSE => truth_row::<false, D, A, B>(frame),
        NOT | NEG => unary_row::<K, D, A, B>(frame),
        REM => remainder_row::<D, A, B, SPEC>(frame),
        MATH_FIRST..=MATH_LAST => math_row::<K, D, A, B>(frame),
        MIN | MAX => extreme_row::<K, D, A, B>(frame),
        IMUL => imul_row::<D, A, B, SPEC>(frame),
        CLZ32 => clz32_row::<D, A, B>(frame),
        _ => tag_init(frame.s),
    }
}
