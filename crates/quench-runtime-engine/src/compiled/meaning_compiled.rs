//! What an instruction means to the compiled tiers, classified from the shared decode.
//!
//! A classification asks the operation the record performs (`performed`: the staged decode of the
//! record without its result flags, so a record that also returns or writes `this` is classified by
//! its operation). Each decoded operation belongs to one opcode family, so a decoded operation
//! outside the family is an exact no, and a record with no staged operation is no member of any
//! family. Nothing here asks the opcode's identity.

#[allow(unused_imports)]
use crate::ext_traits::*;
use quench_runtime::bytecode::{Op, Register, WideInstruction};
use quench_runtime::staged::{NumberOperation, Operation, Protocol};
use crate::compiled::performed;

pub(crate) use quench_runtime::staged::CallForm;

/// The call form `instruction` makes (`CallForm::of`, the staged module's form of every
/// call-family record, eval calls and array constructions included).
pub(crate) fn call_form(instruction: WideInstruction) -> Option<CallForm> {
    CallForm::of(instruction)
}

/// A plain call through a callee register and a window (`Call`).
pub(crate) fn is_call(instruction: WideInstruction) -> bool {
    call_form(instruction).is_some_and(|form| form == CallForm::Plain)
}

/// A call of a function the record names (`CallKnown`).
pub(crate) fn is_call_known(instruction: WideInstruction) -> bool {
    call_form(instruction).is_some_and(|form| form == CallForm::Known)
}

/// A `JumpBinaryFalse`: a branch where the comparison of its two operands fails.
#[inline(always)]
pub(crate) fn is_jump_binary_false(instruction: WideInstruction) -> bool {
    match performed(instruction) {
        Some(Operation::CompareJump { .. }) => true,
        _ => false,
    }
}

/// A read of a binding by name (`LoadName`, `LoadNameCall`, `LoadNameTypeof`): the catalog's
/// `loads_name`. The name reads are proved rows (`decode_load_name`), which take a proof, so
/// the name read is classified through that decode, and `typeof` through the global-typeof row.
pub(crate) fn loads_name(instruction: WideInstruction) -> bool {
    if Operation::<(), ()>::decode_load_name(instruction, ()).is_some() {
        return true;
    }
    match performed(instruction) {
        Some(Operation::GlobalTypeof { .. }) => true,
        _ => false,
    }
}

/// An ordinary call: a plain call or a call of a spread array (`Call`, `CallArray`).
#[inline(always)]
pub(crate) fn is_ordinary_call(instruction: WideInstruction) -> bool {
    call_form(instruction).is_some_and(|form| matches!(form, CallForm::Plain | CallForm::Array))
}

/// A call whose one argument register holds the array to apply.
pub(crate) fn is_call_array(instruction: WideInstruction) -> bool {
    call_form(instruction).is_some_and(|form| form == CallForm::Array)
}

/// A construction, plain or `super(...)`.
#[inline(always)]
pub(crate) fn is_construct(instruction: WideInstruction) -> bool {
    call_form(instruction).is_some_and(|form| matches!(form, CallForm::Construct | CallForm::Super))
}

/// A binary operation of two operands (`Binary`): its equality row or the VM's operator.
pub(crate) fn is_binary(instruction: WideInstruction) -> bool {
    match performed(instruction) {
        Some(Operation::Equality { .. } | Operation::Binary { .. }) => true,
        _ => false,
    }
}

/// A binary arithmetic operation whose operator `binary_operator` names: the equality row or the
/// VM's operator of a `Binary` record.
pub(crate) fn is_binary_arithmetic(instruction: WideInstruction) -> bool {
    match performed(instruction) {
        Some(Operation::Equality { .. } | Operation::Binary { .. }) => true,
        _ => false,
    }
}

/// A unary operation (`Unary`): a logical not, a `typeof` index, `void` or a bitwise not.
pub(crate) fn is_unary(instruction: WideInstruction) -> bool {
    match performed(instruction) {
        Some(Operation::LogicalNot { .. } | Operation::TypeofIndex { .. } | Operation::Void { .. } | Operation::Unary { .. }
            | Operation::Number(NumberOperation::BitwiseNot { .. })) => true,
        _ => false,
    }
}

/// A numeric increment or decrement (`IncDec`), into a register or a numeric local.
pub(crate) fn is_inc_dec(instruction: WideInstruction) -> bool {
    match performed(instruction) {
        Some(Operation::Number(NumberOperation::IncDec { .. }) | Operation::IncDecLocal { .. }) => true,
        _ => false,
    }
}

/// A conversion to a numeric value (`ToNumeric`).
pub(crate) fn is_to_numeric(instruction: WideInstruction) -> bool {
    match performed(instruction) {
        Some(Operation::Number(NumberOperation::ToNumeric { .. })) => true,
        _ => false,
    }
}

/// An indexed read of an array (`GetIndex`), of a register or of a local operand.
pub(crate) fn is_get_index(instruction: WideInstruction) -> bool {
    match performed(instruction) {
        Some(Operation::ElementRead { .. } | Operation::ElementReadOperand { .. }) => true,
        _ => false,
    }
}

/// An indexed store into an array (`SetIndex`).
pub(crate) fn is_set_index(instruction: WideInstruction) -> bool {
    match performed(instruction) {
        Some(Operation::ElementWrite { .. }) => true,
        _ => false,
    }
}

/// A named property read (`GetField`).
#[inline(always)]
pub(crate) fn is_get_field(instruction: WideInstruction) -> bool {
    match performed(instruction) {
        Some(Operation::GetField { .. }) => true,
        _ => false,
    }
}

/// A copy of a register into another (`Move`).
pub(crate) fn is_move(instruction: WideInstruction) -> bool {
    match performed(instruction) {
        Some(Operation::Move { .. }) => true,
        _ => false,
    }
}

/// A read of a capture (`LoadCapture`), whose result flags the decode takes as none.
pub(crate) fn is_load_capture(instruction: WideInstruction) -> bool {
    match performed(instruction) {
        Some(Operation::LoadCapture { .. }) => true,
        _ => false,
    }
}

/// A write of a capture (`StoreCapture`).
pub(crate) fn is_store_capture(instruction: WideInstruction) -> bool {
    match performed(instruction) {
        Some(Operation::StoreCapture { .. }) => true,
        _ => false,
    }
}

/// A read of an environment-resident local (`LoadEnvLocal`).
pub(crate) fn is_load_env_local(instruction: WideInstruction) -> bool {
    match performed(instruction) {
        Some(Operation::LoadEnvLocal { .. }) => true,
        _ => false,
    }
}

/// A read of `this` (`LoadThis`).
pub(crate) fn is_load_this(instruction: WideInstruction) -> bool {
    match performed(instruction) {
        Some(Operation::LoadThis { .. }) => true,
        _ => false,
    }
}

/// A `RequireObjectCoercible` check of a register.
pub(crate) fn is_require_object_coercible(instruction: WideInstruction) -> bool {
    match performed(instruction) {
        Some(Operation::RequireObjectCoercible { .. }) => true,
        _ => false,
    }
}

/// An unconditional jump (`Jump`).
pub(crate) fn is_jump(instruction: WideInstruction) -> bool {
    match performed(instruction) {
        Some(Operation::Jump) => true,
        _ => false,
    }
}

/// A jump on a register's truthiness, taken where the register is truthy (`JumpTrue`).
pub(crate) fn is_jump_true(instruction: WideInstruction) -> bool {
    match performed(instruction) {
        Some(Operation::JumpIf { taken, .. }) => taken,
        _ => false,
    }
}

/// A jump on a register's truthiness, taken where the register is falsy (`JumpFalse`).
pub(crate) fn is_jump_false(instruction: WideInstruction) -> bool {
    match performed(instruction) {
        Some(Operation::JumpIf { taken, .. }) => !taken,
        _ => false,
    }
}

/// A record that does nothing (`Nop`).
pub(crate) fn is_nop(instruction: WideInstruction) -> bool {
    match performed(instruction) {
        Some(Operation::Nop) => true,
        _ => false,
    }
}

/// A return of a register (`Return`).
pub(crate) fn is_return(instruction: WideInstruction) -> bool {
    match performed(instruction) {
        Some(Operation::Return { .. }) => true,
        _ => false,
    }
}

/// A raise of its operand (`Throw`): the owned step that throws the register.
pub(crate) fn is_throw(instruction: WideInstruction) -> bool {
    match performed(instruction) {
        Some(Operation::Throw { .. }) => true,
        _ => false,
    }
}

/// A read of a frame-resident local (`LoadLocal`, `LoadLocalPlain`), the catalog's `loads_frame_local`.
pub(crate) fn loads_frame_local(instruction: WideInstruction) -> bool {
    match performed(instruction) {
        Some(Operation::LoadLocalPlain { .. }
            | Operation::Protocol { step: Protocol::Owned(quench_runtime::staged::protocol_owned::OwnedVarStep::LoadLocal { .. }), .. }) => true,
        _ => false,
    }
}

/// A record a stitched inline admits: a register move, a conversion to a number, a constant load,
/// a binary or unary operation, an increment or decrement, a jump, a return or a capture read.
pub(crate) fn stitchable(instruction: WideInstruction) -> bool {
    is_move(instruction) || is_to_numeric(instruction) || is_binary_arithmetic(instruction)
        || is_inc_dec(instruction) || is_unary(instruction) || is_jump_binary_false(instruction)
        || is_jump_true(instruction) || is_jump_false(instruction) || is_jump(instruction)
        || is_return(instruction) || is_load_capture(instruction)
        || quench_runtime::staged::Operation::<(), ()>::decode_constant_load(instruction).is_some()
}

/// A store of a binding by name (`StoreName`), which the global store row decodes.
pub(crate) fn stores_name(instruction: WideInstruction) -> bool {
    match performed(instruction) {
        Some(Operation::GlobalWrite { .. }) => true,
        _ => false,
    }
}

/// A binary operation of the numeric lowering (`JsDenotation::NumericBinary`): the catalog's property
/// of the opcode, which `numeric_binary` rows and the stage admit by their operator.
#[inline(always)]
pub(crate) fn is_numeric_binary(instruction: WideInstruction) -> bool {
    instruction.op().js_denotation() == quench_runtime::bytecode::JsDenotation::NumericBinary
}

/// A unary operation of the numeric lowering (`JsDenotation::NumericUnary`), as `is_numeric_binary`.
#[inline(always)]
pub(crate) fn is_numeric_unary(instruction: WideInstruction) -> bool {
    instruction.op().js_denotation() == quench_runtime::bytecode::JsDenotation::NumericUnary
}

/// Whether a region rejection's opcode is a plain call (`Op::Call`), the stop the inliner takes over
/// once the call site is hot (`RegionAnalysisFailure::is_call_boundary`).
#[inline(always)]
pub(crate) fn is_call_rejection(opcode: u16) -> bool {
    opcode == Op::Call as u16
}

/// The record that reads a named field into `a` through the base in `b`, the lookup in `c` and the
/// atom in `imm` (`GetField`). A rewrite builds its reads with this, not by naming the opcode.
#[inline(always)]
pub(crate) fn field_read_record(a: Register, b: u16, c: u16, imm: u32) -> WideInstruction {
    WideInstruction::new(Op::GetField, a, b, c, imm)
}

/// The record that stores `a` into the `this` field named by the site in `c` and atom `imm`
/// (`SetThisField`), the sink of a fused read.
#[inline(always)]
pub(crate) fn this_field_store_record(a: Register, b: u16, c: u16, imm: u32) -> WideInstruction {
    WideInstruction::new(Op::SetThisField, a, b, c, imm)
}

/// The record that stores `a` into a field of the object in `b` through the lookup in `c` and atom
/// `imm` (`SetField`).
#[inline(always)]
pub(crate) fn field_store_record(a: u16, b: u16, c: u16, imm: u32) -> WideInstruction {
    WideInstruction::new(Op::SetField, a, b, c, imm)
}

/// The record that copies register `b` into `a` (`Move`).
#[inline(always)]
pub(crate) fn move_record(a: Register, b: Register) -> WideInstruction {
    WideInstruction::new(Op::Move, a, b, 0, 0)
}

/// The record that jumps unconditionally to the PC `target` (`Jump`).
#[inline(always)]
pub(crate) fn jump_record(target: u32) -> WideInstruction {
    WideInstruction::new(Op::Jump, 0, 0, 0, target)
}

/// The record that reads a capture into `a` at the depth and slot its immediate `imm` names
/// (`LoadCapture`), through the operands in `b` and `c`.
#[inline(always)]
pub(crate) fn capture_load_record(a: u16, b: u16, c: u16, imm: u32) -> WideInstruction {
    WideInstruction::new(Op::LoadCapture, a, b, c, imm)
}

/// The record that does nothing (`Nop`): a marker a rewrite leaves or a slot a residual clears.
#[inline(always)]
pub(crate) fn nop_record() -> WideInstruction {
    WideInstruction::new(Op::Nop, 0, 0, 0, 0)
}

/// The plain call record: callee in `b`, result in `a`, receiver and arguments from `c` as the
/// immediate `imm` lays them out (`Call`).
#[inline(always)]
pub(crate) fn call_record(a: Register, b: u16, c: u16, imm: u32) -> WideInstruction {
    WideInstruction::new(Op::Call, a, b, c, imm)
}

/// The construct record over the same operands as the original construction (`Construct`).
#[inline(always)]
pub(crate) fn construct_record(a: Register, b: u16, c: u16, imm: u32) -> WideInstruction {
    WideInstruction::new(Op::Construct, a, b, c, imm)
}

/// The site-lookup field read record (`GetField`) with result flags folded into `a`.
#[inline(always)]
pub(crate) fn get_field_record(a: Register, b: u16, c: u16, imm: u32) -> WideInstruction {
    field_read_record(a, b, c, imm)
}

/// Whether the record is a `super(...)` construction (the catalog's own trait for it).
#[inline(always)]
pub(crate) fn is_super_construct(instruction: WideInstruction) -> bool {
    instruction.is_super_construct()
}

/// Whether the record's `a` operand names a register it writes (the catalog's field layout).
#[inline(always)]
pub(crate) fn writes_register_a(instruction: WideInstruction) -> bool {
    instruction.op().field_layout(quench_runtime::bytecode::InstructionField::A).is_register_field()
}

/// Whether the inline-unit rewriter replaces this opcode's records (the catalog's trait).
#[inline(always)]
pub(crate) fn is_unit_rewritten(op: Op) -> bool {
    op.is_unit_rewritten()
}

/// The number of catalog opcodes: one past the last opcode index, the index a table keeps for a
/// dynamic (non-catalog) operation.
pub(crate) const OPCODE_COUNT: usize = Op::COUNT;

const WASM_WORD: usize = u64::BITS as usize;

/// The Wasm opcodes (catalog names that begin `Wasm`), by opcode index. A Wasm function has its
/// own compiler, which owns the linear memory, tables and typed registers these opcodes read.
const WASM_OPCODES: [u64; Op::COUNT.div_ceil(WASM_WORD)] = {
    const PREFIX: &[u8] = b"Wasm";
    let mut table = [0; Op::COUNT.div_ceil(WASM_WORD)];
    let mut index = 0;
    while index < Op::COUNT {
        let name = Op::NAMES[index].as_bytes();
        let mut at = 0;
        let mut named = name.len() >= PREFIX.len();
        while named && at < PREFIX.len() {
            named = name[at] == PREFIX[at];
            at += 1;
        }
        table[index / WASM_WORD] |= (named as u64) << (index % WASM_WORD);
        index += 1;
    }
    table
};

/// Whether the record is a Wasm opcode, which no JS tier compiles.
#[inline(always)]
pub(crate) fn is_wasm(instruction: WideInstruction) -> bool {
    let index = instruction.op() as usize;
    WASM_OPCODES[index / WASM_WORD] >> (index % WASM_WORD) & 1 != 0
}
