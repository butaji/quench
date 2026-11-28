//! Which staged operation an instruction denotes, asked without its operand tables.
//!
//! `Operation::decode` needs the tier's constants for a `LoadConst` and its call records for a
//! call, and declines a few forms it leaves to the tier. A tier that only asks *which* operation
//! an instruction is (to pick a path, to scan a loop for effects) asks here instead of naming
//! opcodes: the semantics own the mapping from records to meanings, every tier reads it.

#[cfg(not(stencil_build))]
#[allow(unused_imports)] use crate::staged::{OperationOperationExt};
use super::frame_arm::{frame_arm, FrameArm};
use crate::bytecode::{Class, Op, WideInstruction};
#[cfg(not(stencil_build))]
#[allow(unused_imports)]
use crate::staged::glue_prelude::*;

/// A family of staged operations. One record can mean more than one (a compare-jump is also a
/// conditional jump), so a tier asks `means(instruction, meaning)` for each it cares about.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Meaning {
    /// `Operation::LoadConst`, whatever the constant.
    LoadConst,
    /// A call through its forms (`Operation::Call`), not a construction.
    Call,
    /// A construction (`new`, `super(...)`).
    Construct,
    /// `NumberOperation::IncDec` and its local-slot form.
    IncDec,
    /// Any unary operator.
    Unary,
    /// Any binary operator record (arithmetic, bitwise, relational, equality).
    Binary,
    /// `throw`.
    Throw,
    /// A name read that throws where the name is unbound (`LoadName` outside `typeof`).
    ThrowingNameLoad,
    /// A jump taken on a value's truth or a comparison (`JumpIf`, `CompareJump`).
    ConditionalJump,
    /// A named field read or store.
    NamedFieldAccess,
    /// A local-slot read.
    LoadLocal,
    /// A local-slot store.
    StoreLocal,
    /// A store to a `var` binding.
    StoreVarBinding,
    /// A binding's temporal-dead-zone initialization.
    InitializeTdz,
    /// An `arguments[i]` read.
    ArgumentGet,
    /// `SetFunctionName`.
    SetFunctionName,
    /// A register-to-register move.
    Move,
    /// An instruction that does nothing.
    Nop,
    /// An unconditional jump.
    Jump,
    /// A jump taken on one value's truth (`JumpIf`).
    TruthJump,
    /// A jump taken on a binary comparison (`CompareJump`).
    CompareJump,
    /// A binary operator of the numeric lowering: `Binary` and its Number forms (`NumericAdd`,
    /// `NumericMultiply`), whose operator `binary_operator` names.
    NumericBinary,
    /// A unary operator of the numeric lowering (`Unary`), whose operator `unary_operator` names.
    NumericUnary,
    /// A Wasm branch table (`WasmBranchTable`), which `Wasm` jump decoding reads as a `BranchTable` (`wasm_flow`).
    BranchTable,
    /// A method call through its receiver register (`CallMethod`), whose method site `Invocation::method` reads.
    MethodCall,
    /// `NumericAdd` and `NumericMultiply`: the Number arithmetic of their own operator, which `decode_generic` reads
    /// with `Binary`'s operation.
    NumericArithmetic,
    /// A string-case index over a constant table (`StringCaseIndex`), which `decode_lane` reads.
    StringCaseIndex,
}

/// Whether `instruction` denotes an operation of `meaning`.
#[inline(always)]
pub fn means(instruction: impl std::borrow::Borrow<WideInstruction>, meaning: Meaning) -> bool {
    let op = instruction.borrow().op();
    match meaning {
        Meaning::LoadConst => op.is_load_const(),
        Meaning::Call => op.is_call(),
        Meaning::Construct => op.class() == Class::Construct,
        Meaning::IncDec => op.is_inc_dec(),
        Meaning::Unary => op.is_unary(),
        Meaning::Binary => op.is_binary(),
        Meaning::Throw => op.is_throw(),
        Meaning::ThrowingNameLoad => op.is_throwing_name_load(),
        Meaning::ConditionalJump => op.is_js_conditional_jump(),
        Meaning::NamedFieldAccess => op.is_named_field_access(),
        Meaning::LoadLocal => op.class() == Class::LoadLocal,
        Meaning::StoreLocal => op.class() == Class::StoreLocal,
        Meaning::StoreVarBinding => op.class() == Class::StoreVarBinding,
        Meaning::InitializeTdz => op.class() == Class::InitializeTdz,
        Meaning::ArgumentGet => op.class() == Class::ArgumentGet,
        Meaning::SetFunctionName => op.class() == Class::SetFunctionName,
        Meaning::Move => op.class() == Class::Move,
        Meaning::Nop => op.class() == Class::Nop,
        Meaning::Jump => op.class() == Class::Jump,
        Meaning::TruthJump => matches!(op.class(), Class::JumpTrue | Class::JumpFalse),
        Meaning::CompareJump => op.class() == Class::JumpBinaryFalse,
        Meaning::NumericBinary => op.js_denotation() == crate::bytecode::JsDenotation::NumericBinary,
        Meaning::NumericUnary => op.js_denotation() == crate::bytecode::JsDenotation::NumericUnary,
        Meaning::BranchTable => op.class() == Class::WasmBranchTable,
        Meaning::MethodCall => op.class() == Class::CallMethod,
        Meaning::NumericArithmetic => matches!(frame_arm(op), FrameArm::NumericArith),
        Meaning::StringCaseIndex => op.is_string_case_index(),
    }
}

/// Whether the record of `op` carries an operand feedback byte (`operand_feedback`): the loop arms that read
/// one (`frame_arm`, the one opcode-to-arm map): the generic and numeric arithmetic and the element accesses.
#[inline(always)]
pub const fn records_operand_feedback(op: Op) -> bool {
    matches!(frame_arm(op), FrameArm::Binary | FrameArm::NumericArith | FrameArm::GetIndex | FrameArm::SetIndex)
}
