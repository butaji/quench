//! Canonical lowered operation shapes accepted by ordered reductions.

use crate::ir::Opcode;

pub(super) const REGION_END: usize = 27;
pub(super) const STATE_REGION_END: usize = 30;
pub(super) const LOOP_HEADER: usize = 6;
pub(super) const LOOP_BACKEDGE: usize = 24;
pub(super) const STATE_LOOP_BACKEDGE: usize = 25;
pub(super) const FOR_ARRAY_PC: usize = 13;
pub(super) const FOR_BOUND_PC: usize = 8;
pub(super) const WHILE_LOOP_HEADER: usize = 5;
pub(super) const WHILE_ARRAY_PC: usize = 12;
pub(super) const WHILE_BOUND_PC: usize = 7;
pub(super) const PREDICTABLE_REGION_END: usize = 39;
pub(super) const PREDICTABLE_ARRAY_PC: usize = 13;
pub(super) const PREDICTABLE_BOUND_PC: usize = 8;
pub(super) const PREDICTABLE_LOOP_BACKEDGE: usize = 34;

pub(super) const OPERATIONS: [Opcode; REGION_END] = [
    Opcode::LoadConst,
    Opcode::StoreLocal,
    Opcode::LoadConst,
    Opcode::LoadConst,
    Opcode::StoreLocal,
    Opcode::LoadConst,
    Opcode::LoadLocal,
    Opcode::LoadLocal,
    Opcode::GetN,
    Opcode::Binary,
    Opcode::JumpIfFalse,
    Opcode::LoadLocal,
    Opcode::LoadLocal,
    Opcode::Slow,
    Opcode::LoadLocal,
    Opcode::AGetI,
    Opcode::Add,
    Opcode::StoreLocal,
    Opcode::Move,
    Opcode::LoadLocal,
    Opcode::LoadConst,
    Opcode::Binary,
    Opcode::StoreLocal,
    Opcode::Unary,
    Opcode::Jump,
    Opcode::LoadLocal,
    Opcode::Return,
];

pub(super) const STATE_OPERATIONS: [Opcode; STATE_REGION_END] = [
    Opcode::LoadConst,
    Opcode::StoreLocal,
    Opcode::LoadConst,
    Opcode::LoadConst,
    Opcode::StoreLocal,
    Opcode::LoadConst,
    Opcode::LoadLocal,
    Opcode::LoadLocal,
    Opcode::GetN,
    Opcode::Binary,
    Opcode::JumpIfFalse,
    Opcode::LoadLocal,
    Opcode::LoadLocal,
    Opcode::GetN,
    Opcode::Slow,
    Opcode::LoadLocal,
    Opcode::AGetI,
    Opcode::Add,
    Opcode::StoreLocal,
    Opcode::Move,
    Opcode::LoadLocal,
    Opcode::LoadConst,
    Opcode::Binary,
    Opcode::StoreLocal,
    Opcode::Unary,
    Opcode::Jump,
    Opcode::LoadLocal,
    Opcode::Return,
    Opcode::LoadConst,
    Opcode::Return,
];

pub(super) const WHILE_OPERATIONS: [Opcode; STATE_REGION_END] = [
    Opcode::LoadConst,
    Opcode::StoreLocal,
    Opcode::LoadConst,
    Opcode::StoreLocal,
    Opcode::LoadConst,
    Opcode::LoadLocal,
    Opcode::LoadLocal,
    Opcode::GetN,
    Opcode::Binary,
    Opcode::JumpIfFalse,
    Opcode::LoadLocal,
    Opcode::LoadLocal,
    Opcode::GetN,
    Opcode::Slow,
    Opcode::LoadLocal,
    Opcode::AGetI,
    Opcode::Add,
    Opcode::StoreLocal,
    Opcode::Move,
    Opcode::LoadLocal,
    Opcode::LoadConst,
    Opcode::Binary,
    Opcode::StoreLocal,
    Opcode::Unary,
    Opcode::Move,
    Opcode::Jump,
    Opcode::LoadLocal,
    Opcode::Return,
    Opcode::LoadConst,
    Opcode::Return,
];
