//! Which staged operation an instruction denotes, asked without its operand tables.
//!
//! `Operation::decode` needs the tier's constants for a `LoadConst` and its call records for a
//! call, and declines a few forms it leaves to the tier. A tier that only asks *which* operation
//! an instruction is (to pick a path, to scan a loop for effects) asks here instead of naming
//! opcodes: the semantics own the mapping from records to meanings, every tier reads it.

use crate::bytecode::{Class, WideInstruction};

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
}

/// Whether `instruction` denotes an operation of `meaning`.
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
    }
}
