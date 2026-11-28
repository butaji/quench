//! The body forms a script call target's entry names (`CallEntry`), recognized once as the
//! target resolves (`Vm::resolve_script_call_target`): reads a call answers without an
//! activation, and leaves whose records run without frame state.
#[allow(unused_imports)] use quench_runtime::staged::{OperationOperationExt, OperationOperationRecordsExt};
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_runtime::bytecode::{FieldBase, FieldLookup, Operand, OperandKind};
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

pub(crate) use quench_runtime::compile::body_forms::body_entry;

/// Whether `body` keeps frame state a leaf's records cannot: locals of a generator or async
/// function, a constructor's binding, rest parameters, `arguments`, handlers or dynamic scope.
fn keeps_frame_state(body: &Function) -> bool {
    body.is_generator
        || body.is_async
        || body.is_class_constructor
        || body.derived_constructor
        || body.rest
        || body.arguments_slot.is_some()
        || !body.handlers.is_empty()
        || body.has_dynamic_scope_access()
}

/// Whether `body` is a leaf (`CallEntry::frameless(Frameless::Leaf)`), and then what its records read of the
/// context (`LeafForm`): every instruction's lane record writes registers, captures or
/// nothing, reads no frame state and no activation, and leaves the lane for anything else
/// (its general path), or activates a getter, which makes the leaf an activation first; no
/// backward jump, so no safepoint runs in it. That is a register move or constant, a number
/// or string operation of registers and constants, a capture's load or store, `this`, a
/// field read or store of no private name through a register or `this`, a global name's
/// load, a forward jump, a construction and the return, of a register, a constant or an operator's value. A
/// body that keeps frame state (locals, handlers, `arguments`, dynamic scope) is none.
/// `private` says whether an atom is a private name, whose access the running function
/// decides (`LaneFrame::cached_miss`, `lane_private_atom`).
pub(crate) fn leaf_body(body: &Function, code: &ResidualProgram, private: impl Fn(quench_runtime::bytecode::Atom) -> bool) -> Option<LeafForm> {
    if keeps_frame_state(body) {
        return None;
    }
    // A record reads the receiver (`CONTEXT_LEAF`), the captures (`CAPTURE_LEAF`), or neither.
    let (mut this, mut captures) = (false, false);
    for (pc, instruction) in body.instructions().enumerate() {
        let reads = leaf_reads(instruction, body, code, &private)?;
        if instruction.op().immediate_role() == ImmediateRole::JumpTarget && instruction.imm() as usize <= pc {
            return None;
        }
        this |= reads == CONTEXT_LEAF;
        captures |= reads == CAPTURE_LEAF;
    }
    Some(match (this, captures) {
        (true, true) => CONTEXT_LEAF,
        (true, false) => THIS_LEAF,
        (false, true) => CAPTURE_LEAF,
        (false, false) => PLAIN_LEAF,
    })
}

/// What `instruction`'s lane record reads of the context in a leaf (`leaf_body`), or `None`
/// when no leaf runs it.
fn leaf_reads(instruction: WideInstruction, body: &Function, code: &ResidualProgram,
    private: &impl Fn(quench_runtime::bytecode::Atom) -> bool) -> Option<LeafForm> {
    use quench_runtime::staged::{NumberOperation, Operation};
    // The decoded records of the staged table decide their own leaf form first.
    match crate::compiled::decoded_completed(instruction) {
        // A register move, a jump, a return, an increment or a nop of registers and constants.
        Some(Operation::Move { .. } | Operation::Jump | Operation::JumpIf { .. } | Operation::Return { .. } | Operation::Nop
            | Operation::Number(NumberOperation::IncDec { .. }) | Operation::IncDecLocal { .. }) => return Some(PLAIN_LEAF),
        Some(Operation::LoadThis { .. }) => return Some(CONTEXT_LEAF),
        Some(Operation::LoadCapture { .. } | Operation::StoreCapture { .. }) => return Some(CAPTURE_LEAF),
        // A store past its caches (a setter, a proxy's trap, a frozen target) runs in
        // place (`lane_set_field_in_place`) or on the general path, either of which makes
        // the leaf an activation first.
        Some(Operation::SetField { .. }) => return store_leaf(instruction, private),
        // A returning read takes its row at load, in its leaf form (`LaneView::specialize_leaf_returns`).
        Some(Operation::GetField { .. }) => return field_leaf(instruction, code, private),
        _ => {}
    }
    // A constant load (the decode of an empty context has none) is a plain leaf record.
    if Operation::<(), ()>::decode_constant_load(instruction).is_some() {
        return Some(PLAIN_LEAF);
    }
    // Records the decode leaves to the opcode: the other threads' opcodes (a number operation
    // with a binary form, a unary, a name, a call) and a record the decode rejects.
    Some(match instruction.op() {
        // A construction the lane plans completes in place, or materializes the leaf first
        // (`script_construct_miss`), so its record runs in a leaf.
        op if quench_runtime::staged::construction_family(op) => PLAIN_LEAF,
        op if quench_runtime::staged::leaf_value_or_jump(op) => PLAIN_LEAF,
        op if quench_runtime::staged::leaf_binary_operation(op) => return binary_leaf(instruction),
        op if op.traits().has(Traits::THIS_LEAF) => CONTEXT_LEAF,
        op if op.addresses_capture() => CAPTURE_LEAF,
        // merge: the fallback store arm is kept for stores the decode leaves to the opcode
        // (the decoded `SetField` arm above covers the rest).
        _ if quench_runtime::staged::Operation::<u64, ()>::is_field_store(instruction) => return store_leaf(instruction, private),
        // A global name's first load finds the running function (`name_resolution`),
        // which makes the leaf an activation first; its later loads read the realm alone.
        op if op.traits().has(Traits::NAME_LEAF) && body.names_resolve_statically(code) => PLAIN_LEAF,
        op if op.traits().has(Traits::FIELD_LEAF) => field_leaf(instruction, code, private)?,
        _ => return None,
    })
}

/// A number or string operation runs in a leaf of register and constant operands alone.
fn binary_leaf(instruction: WideInstruction) -> Option<LeafForm> {
    let value_operand = |encoded: u16| matches!(Operand(encoded).kind(), Some(OperandKind::Register | OperandKind::Constant));
    (value_operand(instruction.b()) && value_operand(instruction.c())).then_some(PLAIN_LEAF)
}

/// A field store of no private name, through a register (`PLAIN_LEAF`) or `this`.
fn store_leaf(instruction: WideInstruction, private: &impl Fn(quench_runtime::bytecode::Atom) -> bool) -> Option<LeafForm> {
    if private(instruction.imm()) {
        return None;
    }
    Some(if instruction.field_store_register().is_some() { PLAIN_LEAF } else { CONTEXT_LEAF })
}

/// A field read of no private name through a base a leaf reads; a sink stores the value
/// to a field of `this` (`row_get_field_sink`).
fn field_leaf(instruction: WideInstruction, code: &ResidualProgram, private: &impl Fn(quench_runtime::bytecode::Atom) -> bool) -> Option<LeafForm> {
    match instruction.field_lookup()? {
        FieldLookup::Atom { atom, base, .. } => if private(atom) { None } else { field_base_leaf(base) },
        FieldLookup::Site(index) => {
            let site = code.field_sites.get(index)?;
            if [Some(site.first), site.second, site.sink].into_iter().flatten().any(|(atom, _)| private(atom)) {
                return None;
            }
            let base = field_base_leaf(site.base)?;
            Some(if site.sink.is_some() { CONTEXT_LEAF } else { base })
        }
    }
}

/// A field base reads `this` (`CONTEXT_LEAF`), a register (`PLAIN_LEAF`), or no base a
/// leaf reads.
fn field_base_leaf(base: FieldBase) -> Option<LeafForm> {
    match base {
        FieldBase::THIS => Some(CONTEXT_LEAF),
        base => base.register_index().map(|_| PLAIN_LEAF),
    }
}
