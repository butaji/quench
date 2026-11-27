//! The arm the frame loop takes for an instruction, one const table over the opcode catalog.
//!
//! The loop matches `FrameArm` as a plain jump table: `arm` is one indexed load of `FRAME_ARMS`,
//! built once at compile time from `frame_arm`, so the loop never decodes a record to pick its
//! arm. Each variant names the arm's opcodes exactly as the loop's former opcode arms did.

use crate::bytecode::{Op, WideInstruction};

/// The arm of the frame loop an opcode takes; `Other` is every opcode no arm names.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum FrameArm {
    Other,
    /// `LoadLocalPlain`.
    LoadLocalPlain,
    /// `StoreLocalPlain`.
    StoreLocalPlain,
    /// `Move`.
    Move,
    /// `LoadConst`.
    ConstLoad,
    /// `LoadCapture`.
    LoadCapture,
    /// `JumpFalse`, `JumpTrue`.
    JumpIf,
    /// `JumpBinaryFalse`.
    CompareJump,
    /// `SetField`, `SetFieldStrict`, `SetThisField`, `SetThisFieldStrict`.
    FieldStore,
    /// `Binary`.
    Binary,
    /// `NumericAdd`, `NumericMultiply`.
    NumericArith,
    /// `GetField`.
    GetField,
    /// `GetIndex`.
    GetIndex,
    /// `SetIndex`.
    SetIndex,
    /// `IncDec`.
    IncDec,
    /// `ToNumeric`.
    ToNumeric,
    /// `Jump`.
    Jump,
    /// `Return`.
    Return,
    /// `Call`.
    Call,
}

/// The loop arm of `op`.
pub(crate) const fn frame_arm(op: Op) -> FrameArm {
    match op {
        Op::LoadLocalPlain => FrameArm::LoadLocalPlain,
        Op::StoreLocalPlain => FrameArm::StoreLocalPlain,
        Op::Move => FrameArm::Move,
        Op::LoadConst => FrameArm::ConstLoad,
        Op::LoadCapture => FrameArm::LoadCapture,
        Op::JumpFalse | Op::JumpTrue => FrameArm::JumpIf,
        Op::JumpBinaryFalse => FrameArm::CompareJump,
        Op::SetField | Op::SetFieldStrict | Op::SetThisField | Op::SetThisFieldStrict => FrameArm::FieldStore,
        Op::Binary => FrameArm::Binary,
        Op::NumericAdd | Op::NumericMultiply => FrameArm::NumericArith,
        Op::GetField => FrameArm::GetField,
        Op::GetIndex => FrameArm::GetIndex,
        Op::SetIndex => FrameArm::SetIndex,
        Op::IncDec => FrameArm::IncDec,
        Op::ToNumeric => FrameArm::ToNumeric,
        Op::Jump => FrameArm::Jump,
        Op::Return => FrameArm::Return,
        Op::Call => FrameArm::Call,
        _ => FrameArm::Other,
    }
}

/// `frame_arm` of every opcode, indexed by the opcode's catalog index.
pub(crate) const FRAME_ARMS: [FrameArm; Op::COUNT] = {
    let mut table = [FrameArm::Other; Op::COUNT];
    let mut index = 0;
    while index < Op::COUNT {
        if let Some(op) = Op::from_index(index) {
            table[index] = frame_arm(op);
        }
        index += 1;
    }
    table
};

/// The arm of the frame loop's general dispatch, once the fast arms declined: a return, a call, or
/// the general step. The step arm splits by the catalog's `Traits::MAY_ALLOCATE`, so only an op that
/// may allocate reaches the cell-debt service after it (`Vm::serviced`), and an op
/// that cannot pays nothing: the split is in this table, not a test in the loop.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum GeneralArm {
    /// The general step of an op that cannot allocate.
    Step,
    /// The general step of an op that may allocate, then the cell-debt service.
    Allocating,
    /// `Return`.
    Return,
    /// `Call`: an inline call where allowed, else the general step; then the cell-debt service.
    Call,
}

/// The general arm of `op`.
pub(crate) const fn general_arm(op: Op) -> GeneralArm {
    match frame_arm(op) {
        FrameArm::Return => GeneralArm::Return,
        FrameArm::Call => GeneralArm::Call,
        _ if op.traits().has(crate::bytecode::Traits::MAY_ALLOCATE) => GeneralArm::Allocating,
        _ => GeneralArm::Step,
    }
}

/// `general_arm` of every opcode, indexed by the opcode's catalog index.
pub(crate) const GENERAL_ARMS: [GeneralArm; Op::COUNT] = {
    let mut table = [GeneralArm::Allocating; Op::COUNT];
    let mut index = 0;
    while index < Op::COUNT {
        if let Some(op) = Op::from_index(index) {
            table[index] = general_arm(op);
        }
        index += 1;
    }
    table
};

/// The general arm of an instruction's opcode: one indexed load of `GENERAL_ARMS`.
#[inline(always)]
pub(crate) fn general(op: Op) -> GeneralArm {
    GENERAL_ARMS[op as usize]
}

/// The loop arm of an instruction's opcode: one indexed load of `FRAME_ARMS`.
#[inline(always)]
pub(crate) fn arm(op: Op) -> FrameArm {
    FRAME_ARMS[op as usize]
}

/// Whether a `JumpFalse`/`JumpTrue` record branches on a truthy operand (`JumpTrue`).
#[inline(always)]
pub(crate) fn jump_taken(instruction: WideInstruction) -> bool {
    instruction.op() == Op::JumpTrue
}
