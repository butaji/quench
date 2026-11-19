//! What an instruction means to the compiled tiers, classified from the shared decode.
//!
//! A classification asks the decoded operation first (`decoded`, `decoded_completed`). Each decoded
//! operation belongs to one opcode family, so a decoded operation outside the family is an exact
//! no. Where the decode returns `None` (an eval call, a record whose result flags or operands the
//! decode does not take, an opcode with no staged row such as `NumericAdd`), the catalog's own
//! trait for the opcode decides, as it did before. Nothing here matches on an opcode name.

use crate::bytecode::WideInstruction;
use crate::staged::{CallOperation, Callee, Invocation, NumberOperation, Operation, Passing, Protocol, Receiver};
use crate::vm::compiled::{decoded, decoded_completed};

/// The form a call-family record makes (`Operation::Call`, `SpreadCall`, `SuperCall`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum CallForm {
    /// A call through a callee register and a receiver register, over a window of arguments.
    Plain,
    /// A call of a function the record names.
    Known,
    /// A method call through a method site.
    Method,
    /// A call that passes the activation's `arguments` as its last argument.
    Forwarding,
    /// A construction over a window, allocating its receiver.
    Construct,
    /// A call whose one argument register holds the array to apply.
    Array,
    /// A call whose one argument register holds the iterable to spread.
    Spread,
    /// A call whose one argument register holds a rest view.
    Rest,
    /// `super(...)`.
    Super,
}

/// The call form `instruction` makes, where its record decodes (`decoded_completed`, so a return
/// completes the call without changing its form).
pub(crate) fn call_form(instruction: WideInstruction) -> Option<CallForm> {
    Some(match decoded_completed(instruction)? {
        Operation::Call(CallOperation::Site(Invocation { forwards: true, .. })) => CallForm::Forwarding,
        Operation::Call(CallOperation::Site(Invocation { callee: Callee::Known(_), .. })) => CallForm::Known,
        Operation::Call(CallOperation::Site(Invocation { callee: Callee::Method { .. }, .. })) => CallForm::Method,
        Operation::Call(CallOperation::Site(Invocation { receiver: Receiver::Allocated, .. })) => CallForm::Construct,
        Operation::Call(CallOperation::Site(Invocation { callee: Callee::Register(_), receiver: Receiver::Register(_), .. })) => CallForm::Plain,
        Operation::SpreadCall { passing: Passing::Array, .. } => CallForm::Array,
        Operation::SpreadCall { passing: Passing::Iterable, .. } => CallForm::Spread,
        Operation::SpreadCall { passing: Passing::Rest, .. } => CallForm::Rest,
        Operation::SuperCall { .. } => CallForm::Super,
        _ => return None,
    })
}

/// A plain call through a callee register and a window (`Call`).
pub(crate) fn is_call(instruction: WideInstruction) -> bool {
    match call_form(instruction) {
        Some(form) => form == CallForm::Plain,
        None => instruction.op().is_call(),
    }
}

/// A call of a function the record names (`CallKnown`).
pub(crate) fn is_call_known(instruction: WideInstruction) -> bool {
    match call_form(instruction) {
        Some(form) => form == CallForm::Known,
        None => instruction.op().is_call_known(),
    }
}

/// A `JumpBinaryFalse`: a branch where the comparison of its two operands fails.
pub(crate) fn is_jump_binary_false(instruction: WideInstruction) -> bool {
    match decoded(instruction) {
        Some(Operation::CompareJump { .. }) => true,
        Some(_) => false,
        None => instruction.op().is_jump_binary_false(),
    }
}

/// A read of a binding by name (`LoadName`, `LoadNameCall`, `LoadNameTypeof`): the catalog's
/// `loads_name`. The name reads are proved rows (`decode_load_name`), which take a proof, so
/// the name read is classified through that decode, and `typeof` through the global-typeof row.
pub(crate) fn loads_name(instruction: WideInstruction) -> bool {
    if Operation::<(), ()>::decode_load_name(instruction, ()).is_some() {
        return true;
    }
    match decoded(instruction) {
        Some(Operation::GlobalTypeof { .. }) => true,
        Some(_) => false,
        None => instruction.op().loads_name(),
    }
}

/// An ordinary call: a plain call or a call of a spread array (`Call`, `CallArray`).
pub(crate) fn is_ordinary_call(instruction: WideInstruction) -> bool {
    match call_form(instruction) {
        Some(form) => matches!(form, CallForm::Plain | CallForm::Array),
        None => instruction.op().is_ordinary_call(),
    }
}

/// A call whose one argument register holds the array to apply.
pub(crate) fn is_call_array(instruction: WideInstruction) -> bool {
    match call_form(instruction) {
        Some(form) => form == CallForm::Array,
        None => instruction.op().is_call_array(),
    }
}

/// A construction, plain or `super(...)`.
pub(crate) fn is_construct(instruction: WideInstruction) -> bool {
    match call_form(instruction) {
        Some(form) => matches!(form, CallForm::Construct | CallForm::Super),
        None => instruction.op().is_construct(),
    }
}

/// A binary operation of two operands (`Binary`): its equality row or the VM's operator.
pub(crate) fn is_binary(instruction: WideInstruction) -> bool {
    match decoded(instruction) {
        Some(Operation::Equality { .. } | Operation::Binary { .. }) => true,
        Some(_) => false,
        None => instruction.op().is_binary(),
    }
}

/// A binary arithmetic operation: `Binary`, `NumericAdd` or `NumericMultiply`, whose operator
/// `binary_operator` names. The numeric forms have no staged row, so they take the catalog's trait.
pub(crate) fn is_binary_arithmetic(instruction: WideInstruction) -> bool {
    match decoded(instruction) {
        Some(Operation::Equality { .. } | Operation::Binary { .. }) => true,
        Some(_) => false,
        None => instruction.op().is_binary_arithmetic(),
    }
}

/// A unary operation (`Unary`): a logical not, a `typeof` index, `void` or a bitwise not.
pub(crate) fn is_unary(instruction: WideInstruction) -> bool {
    match decoded(instruction) {
        Some(Operation::LogicalNot { .. } | Operation::TypeofIndex { .. } | Operation::Void { .. } | Operation::Unary { .. }
            | Operation::Number(NumberOperation::BitwiseNot { .. })) => true,
        Some(_) => false,
        None => instruction.op().is_unary(),
    }
}

/// A numeric increment or decrement (`IncDec`), into a register or a numeric local.
pub(crate) fn is_inc_dec(instruction: WideInstruction) -> bool {
    match decoded(instruction) {
        Some(Operation::Number(NumberOperation::IncDec { .. }) | Operation::IncDecLocal { .. }) => true,
        Some(_) => false,
        None => instruction.op().is_inc_dec(),
    }
}

/// A conversion to a numeric value (`ToNumeric`).
pub(crate) fn is_to_numeric(instruction: WideInstruction) -> bool {
    match decoded(instruction) {
        Some(Operation::Number(NumberOperation::ToNumeric { .. })) => true,
        Some(_) => false,
        None => instruction.op().is_to_numeric(),
    }
}

/// An indexed read of an array (`GetIndex`), of a register or of a local operand.
pub(crate) fn is_get_index(instruction: WideInstruction) -> bool {
    match decoded(instruction) {
        Some(Operation::ElementRead { .. } | Operation::ElementReadOperand { .. }) => true,
        Some(_) => false,
        None => instruction.op().is_get_index(),
    }
}

/// An indexed store into an array (`SetIndex`).
pub(crate) fn is_set_index(instruction: WideInstruction) -> bool {
    match decoded(instruction) {
        Some(Operation::ElementWrite { .. }) => true,
        Some(_) => false,
        None => instruction.op().is_set_index(),
    }
}

/// A named property read (`GetField`).
pub(crate) fn is_get_field(instruction: WideInstruction) -> bool {
    match decoded(instruction) {
        Some(Operation::GetField { .. }) => true,
        Some(_) => false,
        None => instruction.op().is_get_field(),
    }
}

/// A copy of a register into another (`Move`).
pub(crate) fn is_move(instruction: WideInstruction) -> bool {
    match decoded(instruction) {
        Some(Operation::Move { .. }) => true,
        Some(_) => false,
        None => instruction.op().is_move(),
    }
}

/// A read of a capture (`LoadCapture`), whose result flags the decode takes as none.
pub(crate) fn is_load_capture(instruction: WideInstruction) -> bool {
    match decoded(instruction) {
        Some(Operation::LoadCapture { .. }) => true,
        Some(_) => false,
        None => instruction.op().is_load_capture(),
    }
}

/// A write of a capture (`StoreCapture`).
pub(crate) fn is_store_capture(instruction: WideInstruction) -> bool {
    match decoded(instruction) {
        Some(Operation::StoreCapture { .. }) => true,
        Some(_) => false,
        None => instruction.op().is_store_capture(),
    }
}

/// A read of an environment-resident local (`LoadEnvLocal`).
pub(crate) fn is_load_env_local(instruction: WideInstruction) -> bool {
    match decoded(instruction) {
        Some(Operation::LoadEnvLocal { .. }) => true,
        Some(_) => false,
        None => instruction.op().is_load_env_local(),
    }
}

/// A read of `this` (`LoadThis`).
pub(crate) fn is_load_this(instruction: WideInstruction) -> bool {
    match decoded(instruction) {
        Some(Operation::LoadThis { .. }) => true,
        Some(_) => false,
        None => instruction.op().is_load_this(),
    }
}

/// A `RequireObjectCoercible` check of a register.
pub(crate) fn is_require_object_coercible(instruction: WideInstruction) -> bool {
    match decoded(instruction) {
        Some(Operation::RequireObjectCoercible { .. }) => true,
        Some(_) => false,
        None => instruction.op().is_require_object_coercible(),
    }
}

/// An unconditional jump (`Jump`).
pub(crate) fn is_jump(instruction: WideInstruction) -> bool {
    match decoded(instruction) {
        Some(Operation::Jump) => true,
        Some(_) => false,
        None => instruction.op().is_jump(),
    }
}

/// A jump on a register's truthiness, taken where the register is truthy (`JumpTrue`).
pub(crate) fn is_jump_true(instruction: WideInstruction) -> bool {
    match decoded(instruction) {
        Some(Operation::JumpIf { taken, .. }) => taken,
        Some(_) => false,
        None => instruction.op().is_jump_true(),
    }
}

/// A jump on a register's truthiness, taken where the register is falsy (`JumpFalse`).
pub(crate) fn is_jump_false(instruction: WideInstruction) -> bool {
    match decoded(instruction) {
        Some(Operation::JumpIf { taken, .. }) => !taken,
        Some(_) => false,
        None => instruction.op().is_jump_false(),
    }
}

/// A record that does nothing (`Nop`).
pub(crate) fn is_nop(instruction: WideInstruction) -> bool {
    match decoded(instruction) {
        Some(Operation::Nop) => true,
        Some(_) => false,
        None => instruction.op().is_nop(),
    }
}

/// A return of a register (`Return`).
pub(crate) fn is_return(instruction: WideInstruction) -> bool {
    match decoded(instruction) {
        Some(Operation::Return { .. }) => true,
        Some(_) => false,
        None => instruction.op().is_return(),
    }
}

/// A raise of its operand (`Throw`): the owned step that throws the register.
pub(crate) fn is_throw(instruction: WideInstruction) -> bool {
    match decoded(instruction) {
        Some(Operation::Protocol { step: Protocol::Owned(crate::staged::protocol_owned::OwnedVarStep::ThrowValue), .. }) => true,
        Some(_) => false,
        None => instruction.op().is_throw(),
    }
}

/// A read of a frame-resident local (`LoadLocal`, `LoadLocalPlain`), the catalog's `loads_frame_local`.
pub(crate) fn loads_frame_local(instruction: WideInstruction) -> bool {
    match decoded(instruction) {
        Some(Operation::LoadLocalPlain { .. }
            | Operation::Protocol { step: Protocol::Owned(crate::staged::protocol_owned::OwnedVarStep::LoadLocal { .. }), .. }) => true,
        Some(_) => false,
        None => instruction.op().loads_frame_local(),
    }
}

/// A record a stitched inline admits: a register move, a conversion to a number, a constant load,
/// a binary or unary operation, an increment or decrement, a jump, a return or a capture read.
pub(crate) fn stitchable(instruction: WideInstruction) -> bool {
    is_move(instruction) || is_to_numeric(instruction) || is_binary_arithmetic(instruction)
        || is_inc_dec(instruction) || is_unary(instruction) || is_jump_binary_false(instruction)
        || is_jump_true(instruction) || is_jump_false(instruction) || is_jump(instruction)
        || is_return(instruction) || is_load_capture(instruction)
        || crate::staged::Operation::<(), ()>::decode_constant_load(instruction).is_some()
}

/// A store of a binding by name (`StoreName`), which the global store row decodes.
pub(crate) fn stores_name(instruction: WideInstruction) -> bool {
    match decoded(instruction) {
        Some(Operation::GlobalWrite { .. }) => true,
        Some(_) => false,
        None => instruction.op().stores_name(),
    }
}
