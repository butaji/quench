//! The integer bounds a function's replay puts on the keys it stores to and compares against (`FunctionFlow::bounds`),
//! and the constants a relational jump compares a key with. A bound widens a key's range, so a counter compared with a constant
//! settles at it.

use super::super::effects::Ctx;
use super::super::record::Record;
use super::super::scope::Scope;
use super::super::value::{Key, Origin, Word};
use crate::bytecode::{Function, Operand, OperandKind, WideInstruction, used_registers};
use crate::staged::{Comparison, NumberOperation, Operation};
use crate::staged::typing;
use crate::vm::operations::NumberBitwise;
#[cfg(not(stencil_build))]
#[allow(unused_imports)]
use crate::staged::glue_prelude::*;

/// The integer constants the function's relational jumps compare a value against (`typing::thresholds`): the ones
/// whose operand is a constant. A register operand's bound is the state's, which the narrowing pass tightens to.
pub(super) fn relational_bounds(function: &Function, scope: &Scope) -> Vec<i64> {
    (0..function.code.len())
        .filter_map(|pc| match Operation::<u64, ()>::of(function.instruction_at(pc))? {
            Operation::CompareJump { comparison: Comparison::Relation { .. }, left, right, .. } => Some([left, right]),
            _ => None,
        })
        .flatten()
        .filter(|operand| matches!(operand.kind(), Some(crate::bytecode::OperandKind::Constant)))
        .filter_map(|operand| scope.constant_type(usize::from(operand.payload())).int_range())
        .flat_map(|range| [range.min, range.max])
        .collect()
}

/// The integer range of a program constant operand (`Scope::constant_type`); `None` for a register.
fn constant_range(ctx: &Ctx<'_>, operand: Operand) -> Option<IntRange> {
    if matches!(operand.kind(), Some(OperandKind::Constant)) { ctx.scope.constant_type(usize::from(operand.payload())).int_range() } else { None }
}

/// The key a value read from `origin` is the binding or named property of (`Record::commit` reads keys the same way).
fn origin_key(origin: Option<Origin>) -> Option<Key> {
    match origin? {
        Origin::Binding(key) => Some(key),
        Origin::Member(atom) => Some(Key::Member(atom)),
        Origin::MemberOn { class, instance, atom, .. } => Some(Key::MemberOn { class, instance, atom }),
        _ => None,
    }
}

/// The bounds a relational jump puts on the key of its register operand, where its other operand is a constant: each
/// end of the constant's range (`Record::bounds`). A field compared with `this.count < 1000` gets 1000 under its key.
pub(super) fn compared_bounds(ctx: &Ctx<'_>, instruction: WideInstruction, state: &[Word], record: &mut Record) {
    let Some(Operation::CompareJump { comparison: Comparison::Relation { .. }, left, right, .. }) = Operation::<u64, ()>::of(instruction) else { return };
    for (operand, other) in [(left, right), (right, left)] {
        let Some(register) = operand.register_index() else { continue };
        let Some(range) = constant_range(ctx, other) else { continue };
        let Some(key) = origin_key(state[usize::from(register)].origin) else { continue };
        record.bounds.extend([range.min, range.max].map(|bound| (key, bound)));
    }
}

/// Marks the registers `instruction` writes: a `&` with a nonnegative constant operand holds that constant's mask bound
/// (`typing::mask_bound`), and any other write clears the mark. Only the bytecode decides the marks, so they agree in every pass.
pub(super) fn mask_bounds(ctx: &Ctx<'_>, instruction: WideInstruction, masks: &mut [Option<i64>]) {
    for register in instruction.written_registers() { masks[usize::from(register)] = None; }
    if let Some(Operation::Number(NumberOperation::Bitwise { destination, left, right, operation: NumberBitwise::And })) = Operation::<u64, ()>::of(instruction) {
        masks[usize::from(destination)] = [left, right].into_iter().find_map(|operand| typing::mask_bound(constant_range(ctx, operand)?));
    }
}

/// The mask bound a store writes to the keys it stores to: a value the instruction reads that holds one (`mask_bounds`).
pub(super) fn stored_bounds(instruction: WideInstruction, writes: &[(Key, Word)], masks: &[Option<i64>], record: &mut Record) {
    let Some(bound) = used_registers(instruction).into_iter().find_map(|register| masks[usize::from(register)]) else { return };
    record.bounds.extend(writes.iter().map(|&(key, _)| (key, bound)));
}
