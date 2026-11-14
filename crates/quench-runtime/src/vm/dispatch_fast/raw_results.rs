//! Raw intermediate results: a Number result that only the next Number arithmetic reads, and
//! that dies there, keeps its double bits instead of being tested for an integral value.
//!
//! `Value::number` stores an integral double as an integer, which costs a truncation, a
//! conversion back and a compare on every double result (about 7 instructions). Number
//! arithmetic computes the same Number from either form, so where the only reader of a result
//! is the next arithmetic operation and the register is dead after it, the integer form buys
//! nothing. The raw form never reaches a store, a comparison, a key or a return: those read
//! registers this pass never lets a raw result live in.
//!
//! The one producer is the proved fused update, read and operation
//! (`lane_update_get_index_operator_proved`), whose sums V8-v7 NavierStokes' `lin_solve`
//! chains: three of its four element reads per element stored feed only the next sum.

use super::*;
use crate::bytecode::{Class, REGISTER_MASK};
use crate::compile::liveness::{RegisterMasks, register_dead};

/// The offset from a fused update, read and operation's first record to its operation.
const OPERATION: usize = 2;
/// The offsets from an operation to the update and the read of the next such triple.
const NEXT_UPDATE: usize = 1;
const NEXT_READ: usize = 2;
/// The offset from an operation to the next triple's operation.
const NEXT_OPERATION: usize = 3;

/// `number_value_operator` of an arithmetic `OP` (not bitwise) whose double result keeps its
/// double form, integral or not, NaN included (`raw_number`).
#[inline(always)]
pub(super) fn raw_number_value_operator<const OP: u32>(left: Value, right: Value) -> Option<Value> {
    let (left, right) = Value::number_pair(left, right)?;
    Some(raw_number(super::super::operations::number_operator::<OP>(left, right)?))
}

/// A Number in double form whatever its value, a NaN as the arithmetic left it. A raw result
/// only ever reaches the next arithmetic's operand (`raw_result_dies`), which reads it by its
/// tag as a double and computes the same Number from it; that arithmetic stores its own result
/// canonical, or raw under the same rule, so a NaN's bits are never compared, hashed or stored.
/// An arithmetic NaN is either an operand's canonical NaN or the machine's default NaN, and
/// neither has a tag's upper bits, so the bits always read as a double.
#[inline(always)]
pub(super) fn raw_number(value: f64) -> Value {
    Value::from_bits(value.to_bits())
}

/// Whether the arithmetic at `operation` may leave its result raw (`raw_result_dies`), in a
/// function with no handler (a catch may read any register).
pub(super) fn raw_result_allowed(
    decoded: &[WideInstruction],
    handlers: &[crate::bytecode::Handler],
    live: Option<&RegisterMasks>,
    operation: usize,
) -> bool {
    handlers.is_empty() && live.is_some_and(|live| raw_result_dies(decoded, live, operation))
}

/// The raw forms (`raw_results`) of the proved fused updates, reads and operations, each with
/// its generic record, as `proved_records` lists their canonical forms.
pub(super) fn raw_proved_records<H: Host>() -> impl Iterator<Item = (Handler, Handler, usize)> {
    use super::super::operations::{ADDITION, DIVISION, MULTIPLICATION, SUBTRACTION};
    macro_rules! raw_operators {
        ($($operator:ident),*) => {
            [$((lane_update_get_index_operator::<H, false, $operator> as Handler,
                lane_update_get_index_operator_proved::<H, false, $operator, true> as Handler, 1),
               (lane_update_get_index_operator::<H, true, $operator> as Handler,
                lane_update_get_index_operator_proved::<H, true, $operator, true> as Handler, 1)),*]
        };
    }
    fused_arithmetic!(raw_operators).into_iter()
}

/// The first records of the fused update, read and operation triples whose operation's result
/// may stay raw (`raw_result_dies`). A function with a handler has none: a catch may read any
/// register. Liveness is asked for only where a triple's shape occurs.
pub(super) fn raw_result_triples<'l>(
    decoded: &[WideInstruction],
    handlers: &[crate::bytecode::Handler],
    live: impl FnOnce() -> Option<&'l RegisterMasks>,
) -> crate::bitset::BitSet {
    let mut triples = crate::bitset::BitSet::new(decoded.len());
    let candidates = (0..decoded.len().saturating_sub(OPERATION)).filter(|&pc| is_triple(decoded, pc));
    let mut candidates = candidates.peekable();
    if !handlers.is_empty() || candidates.peek().is_none() {
        return triples;
    }
    let Some(live) = live() else {
        return triples;
    };
    for pc in candidates {
        if raw_result_dies(decoded, live, pc + OPERATION) {
            triples.insert(pc);
        }
    }
    triples
}

/// Whether the arithmetic at `operation` writes a plain register that only the next
/// arithmetic reads before it dies: the arithmetic right after it, or the operation of the
/// next triple, whose update and read do not name the register.
fn raw_result_dies(decoded: &[WideInstruction], live: &RegisterMasks, operation: usize) -> bool {
    let producer = decoded[operation];
    if !is_arithmetic(producer) || producer.a() & !REGISTER_MASK != 0 {
        return false;
    }
    let register = producer.a();
    let reads = |pc: usize| decoded.get(pc).is_some_and(|&instruction| is_arithmetic(instruction) && reads_operand(instruction, register));
    let consumer = if reads(operation + 1) {
        operation + 1
    } else if passes_over(decoded, operation, register) && reads(operation + NEXT_OPERATION) {
        operation + NEXT_OPERATION
    } else {
        return false;
    };
    decoded[consumer].a() == register
        || live.get(consumer + 1).is_some_and(|after| register_dead(after, usize::from(register)))
}

/// Whether the two instructions after `operation` are the update and the read of a next
/// triple, neither naming `register` in any field.
fn passes_over(decoded: &[WideInstruction], operation: usize, register: u16) -> bool {
    let (Some(&update), Some(&read)) = (decoded.get(operation + NEXT_UPDATE), decoded.get(operation + NEXT_READ)) else {
        return false;
    };
    update.op().class() == Class::IncDec
        && read.op().is_get_index()
        && !names_register(update, register)
        && !names_register(read, register)
}

/// Whether an update, a read and an arithmetic operation start at `pc`: the shape of a fused
/// update, read and operation (`specialize_updated_index_operators`).
fn is_triple(decoded: &[WideInstruction], pc: usize) -> bool {
    decoded[pc].op().class() == Class::IncDec
        && decoded[pc + NEXT_READ - NEXT_UPDATE].op().is_get_index()
        && is_arithmetic(decoded[pc + OPERATION])
}

/// Number arithmetic the lane fuses (`fused_arithmetic`): the same Number from any operand form.
fn is_arithmetic(instruction: WideInstruction) -> bool {
    use super::super::operations::{ADDITION, DIVISION, MULTIPLICATION, SUBTRACTION};
    instruction.op().js_denotation() == crate::bytecode::JsDenotation::NumericBinary
        && matches!(instruction.binary_operator(), ADDITION | SUBTRACTION | MULTIPLICATION | DIVISION)
}

/// Whether an operand field of arithmetic `instruction` reads `register`.
fn reads_operand(instruction: WideInstruction, register: u16) -> bool {
    Operand(instruction.b()).register_index() == Some(register) || Operand(instruction.c()).register_index() == Some(register)
}

/// Whether any field of `instruction` could name `register`, read as a register or an operand.
fn names_register(instruction: WideInstruction, register: u16) -> bool {
    let names = |field: u16| field & REGISTER_MASK == register || Operand(field).register_index() == Some(register);
    names(instruction.a()) || names(instruction.b()) || names(instruction.c())
}
