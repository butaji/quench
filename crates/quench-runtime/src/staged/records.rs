//! The record forms a tier writes when it rewrites code (an inlined unit, a fused read, a
//! cleared residual): each builder is the encoding its staged row decodes, so a tier makes a
//! record of a given meaning without naming its opcode. The rows (`Operation::decode`) stay the
//! one place that says what the record does.
use crate::bytecode::{Op, Register, WideInstruction};

/// A named field read into `a` through the base in `b`, the lookup in `c` and the atom in `imm`
/// (`Operation::GetField`).
#[inline(always)]
pub fn field_read(a: Register, b: u16, c: u16, imm: u32) -> WideInstruction {
    WideInstruction::new(Op::GetField, a, b, c, imm)
}

/// A store of `a` into the `this` field named by the site in `c` and atom `imm`: the sink of a
/// fused read.
#[inline(always)]
pub fn this_field_store(a: Register, b: u16, c: u16, imm: u32) -> WideInstruction {
    WideInstruction::new(Op::SetThisField, a, b, c, imm)
}

/// A store of `a` into a field of the object in `b` through the lookup in `c` and atom `imm`.
#[inline(always)]
pub fn field_store(a: u16, b: u16, c: u16, imm: u32) -> WideInstruction {
    WideInstruction::new(Op::SetField, a, b, c, imm)
}

/// A copy of register `b` into `a` (`Operation::Move`).
#[inline(always)]
pub fn register_move(a: Register, b: Register) -> WideInstruction {
    WideInstruction::new(Op::Move, a, b, 0, 0)
}

/// An unconditional jump to the PC `target` (`Operation::Jump`).
#[inline(always)]
pub fn jump(target: u32) -> WideInstruction {
    WideInstruction::new(Op::Jump, 0, 0, 0, target)
}

/// A capture read into `a` at the depth and slot its immediate `imm` names, through the operands
/// in `b` and `c`.
#[inline(always)]
pub fn capture_load(a: u16, b: u16, c: u16, imm: u32) -> WideInstruction {
    WideInstruction::new(Op::LoadCapture, a, b, c, imm)
}

/// A record that does nothing (`Operation::Nop`): a marker a rewrite leaves or a slot a residual clears.
#[inline(always)]
pub fn nop() -> WideInstruction {
    WideInstruction::new(Op::Nop, 0, 0, 0, 0)
}

/// A plain call: callee in `b`, result in `a`, receiver and arguments from `c` as `imm` lays them out.
#[inline(always)]
pub fn call(a: Register, b: u16, c: u16, imm: u32) -> WideInstruction {
    WideInstruction::new(Op::Call, a, b, c, imm)
}

/// A construction over the operands of the original construction.
#[inline(always)]
pub fn construct(a: Register, b: u16, c: u16, imm: u32) -> WideInstruction {
    WideInstruction::new(Op::Construct, a, b, c, imm)
}

/// Whether a region rejection's opcode index is the plain call `call` builds: the stop the inliner
/// takes over once the call site is hot.
#[inline(always)]
pub fn is_plain_call_index(opcode: u16) -> bool {
    opcode == Op::Call as u16
}

/// Whether a unit holds records of opcode `op` by its own rewrite or an exit rather than by
/// renaming (the catalog's `UNIT_REWRITTEN` trait, `Operation::is_unit_rewritten`).
#[inline(always)]
pub fn unit_rewritten(op: Op) -> bool {
    op.is_unit_rewritten()
}
