//! The body forms a script call target's entry names (`CallEntry`), recognized once as the
//! target resolves (`Vm::resolve_script_call_target`): reads a call answers without an
//! activation, and leaves whose records run without frame state.
#[allow(unused_imports)] use quench_runtime::staged::{OperationOperationExt, OperationOperationRecordsExt};
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_runtime::bytecode::{FieldBase, FieldLookup, Operand, OperandKind};

/// Whether `base` reads `this` or a parameter: no nested chain and no local.
fn reads_this_or_parameter(base: FieldBase, body: &Function) -> bool {
    base.0 != FieldBase::NESTED
        && base.local_index().is_none()
        && base.register_index().is_none_or(|register| register < body.params)
}

/// The entry a target's body takes in its call form (`frameless`, from the callee's composed signature,
/// `call_plan::entry_form`), before what its call needs (a frame, an environment, kept arguments) decides further.
/// The form is the signature's, never the body's: a framed call form takes `Framed::Activates`, the framed
/// shape that runs any body, so the body's frameless answers are recognized only for a frameless form. A
/// frameless form takes the first answer its body has of those a call answers without activating it; a body
/// with none takes `Framed::Activates` here, and `target_entry` makes it `CallEntry::leaf` where `leaf_body`
/// proves it a leaf; where it proves none, the call keeps a frame although the signature is frameless, since
/// no frameless shape runs such a body. A class constructor's calls throw on the general path, so they take a
/// frame; only its constructions activate frameless (`script_construct_miss`). A frameless form whose body
/// answers none of those takes `Frameless::Native` where `native` (the body is published with native code
/// that calls only natively), before `Framed::Activates`.
pub(crate) fn body_entry(body: &Function, constants: &[quench_runtime::bytecode::Constant], frameless: bool, native: bool) -> CallEntry {
    if body.is_class_constructor {
        CallEntry::framed(Framed::Frame)
    } else if !frameless {
        CallEntry::framed(Framed::Activates)
    } else if body.code.first().is_some_and(|packed| returning_kind(body.widened(*packed)) == Some(RETURNING_FIELD)) {
        CallEntry::frameless(Frameless::Reads)
    } else if element_read_body(body) {
        CallEntry::frameless(Frameless::ReadsElement)
    } else if forwarding_read_body(body) {
        CallEntry::frameless(Frameless::ForwardsRead)
    } else if capture_read_body(body) {
        CallEntry::frameless(Frameless::ReadsCapture)
    } else if this_read_body(body) {
        CallEntry::frameless(Frameless::ReadsThis)
    } else if super::capture_append::recognizes(body, constants) {
        CallEntry::frameless(Frameless::AppendsCapture)
    } else if native {
        CallEntry::frameless(Frameless::Native)
    } else {
        CallEntry::framed(Framed::Activates)
    }
}

/// Whether `body` is `GetField` of `this` or a parameter, `GetField` of a method of that,
/// moves of its parameters in order into a call window, and the call of that method on the
/// first field returning its completion (`CallEntry::frameless(Frameless::ForwardsRead)`), as `at(i) { return
/// this.items.at(i); }` compiles.
#[cold]
#[inline(never)]
fn forwarding_read_body(body: &Function) -> bool {
    if !body.handlers.is_empty() {
        return false;
    }
    let [field, method, rest @ ..] = body.code.as_slice() else {
        return false;
    };
    let Some((moves, [call, ..])) = rest.split_at_checked(usize::from(body.params)) else {
        return false;
    };
    let [field, method, call] = [field, method, call].map(|packed| body.widened(*packed));
    let window = ImmediateLayout::call_window_base(call.imm());
    let window_end = u32::from(window) + u32::from(body.params);
    let survives_copies =
        |register: u16| u32::from(register) < u32::from(window) || u32::from(register) >= window_end;
    reads_receiver_field(field, body, survives_copies)
        && reads_method(method, field, body, survives_copies)
        && (window == 0 || window >= body.params)
        && moves_parameters(body, moves, window)
        && calls_method(call, method, field, body)
}

/// A `GetField` of `this` or a parameter, into a register past the parameters that the
/// call's copies leave.
fn reads_receiver_field(field: WideInstruction, body: &Function, survives_copies: impl Fn(u16) -> bool) -> bool {
    quench_runtime::staged::reads_named_field(field.op())
        && field.result_flags() == 0
        && field.result_register() >= body.params
        && survives_copies(field.result_register())
        && reads_this_or_parameter(FieldBase(field.b()), body)
}

/// A `GetField` of `field`'s value, into another register past the parameters that the
/// call's copies leave.
fn reads_method(method: WideInstruction, field: WideInstruction, body: &Function, survives_copies: impl Fn(u16) -> bool) -> bool {
    quench_runtime::staged::reads_named_field(method.op())
        && method.result_flags() == 0
        && method.result_register() >= body.params
        && method.result_register() != field.result_register()
        && survives_copies(method.result_register())
        && FieldBase(method.b()).register_index() == Some(field.a())
}

/// Each parameter, in order, moved to its place in the call window at `window`.
fn moves_parameters(body: &Function, moves: &[quench_runtime::bytecode::Instr], window: u16) -> bool {
    moves.iter().zip(0..body.params).all(|(&packed, parameter)| {
        let moved = body.widened(packed);
        quench_runtime::staged::copies_register(moved.op()) && moved.b() == parameter && moved.a() == window + parameter
    })
}

/// The call of `method` on `field` with every parameter, returning its completion.
fn calls_method(call: WideInstruction, method: WideInstruction, field: WideInstruction, body: &Function) -> bool {
    quench_runtime::staged::calls(call.op())
        && call.result_flags() == quench_runtime::bytecode::RETURN_REGISTER
        && call.b() == method.a()
        && call.c() == field.a()
        && ImmediateLayout::argument_count(call.imm()) == body.params
}

/// Whether `body` is `GetField` of `this` or a parameter, `GetIndex` of its result by a
/// parameter, and `Return` of that (`CallEntry::frameless(Frameless::ReadsElement)`), as `at(i) { return
/// this.items[i]; }` compiles.
fn element_read_body(body: &Function) -> bool {
    let [field, element, done, ..] = body.code.as_slice() else {
        return false;
    };
    let [field, element, done] = [field, element, done].map(|packed| body.widened(*packed));
    quench_runtime::staged::reads_named_field(field.op())
        && !field.returns_from_frame()
        && reads_this_or_parameter(FieldBase(field.b()), body)
        && quench_runtime::staged::reads_element(element.op())
        && element.b() == field.a()
        && element.c() < body.params
        && quench_runtime::staged::returns(done.op())
        && done.a() == element.a()
}

/// The first instruction of `body` and the one after it, when it has two.
fn leading_pair(body: &Function) -> Option<[WideInstruction; 2]> {
    let [first, second, ..] = body.code.as_slice() else {
        return None;
    };
    Some([first, second].map(|packed| body.widened(*packed)))
}

/// Whether `body` is `LoadCapture` and `Return` of it (`CallEntry::frameless(Frameless::ReadsCapture)`), as
/// `() => binding` compiles.
fn capture_read_body(body: &Function) -> bool {
    leading_pair(body).is_some_and(|[load, done]| {
        quench_runtime::staged::reads_capture(load.op()) && !load.returns_from_frame() && quench_runtime::staged::returns(done.op()) && done.a() == load.a()
    })
}

/// Whether `body` is `LoadThis` and `Return` of it (`CallEntry::frameless(Frameless::ReadsThis)`): a function's own
/// `this`, which neither an arrow's lexical one nor a constructor's binding complicates.
fn this_read_body(body: &Function) -> bool {
    !body.is_arrow
        && !body.is_class_constructor
        && leading_pair(body).is_some_and(|[load, done]| {
            quench_runtime::staged::loads_this(load.op()) && !load.returns_from_frame() && quench_runtime::staged::returns(done.op()) && done.a() == load.a()
        })
}

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
        // A returning read requickens by its site's tier in its leaf form
        // (`row_returning_field_first`, `LaneView::specialize_leaf_returns`).
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
