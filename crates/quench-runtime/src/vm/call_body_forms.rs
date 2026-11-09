//! The body forms a script call target's entry names (`CallEntry`), recognized once as the
//! target resolves (`Vm::resolve_script_call_target`): reads a call answers without an
//! activation, and leaves whose records run without frame state.
use super::*;
use crate::bytecode::{FieldBase, FieldLookup, Operand, OperandKind};

/// Whether `base` reads `this` or a parameter: no nested chain and no local.
fn reads_this_or_parameter(base: FieldBase, body: &Function) -> bool {
    base.0 != FieldBase::NESTED
        && base.local_index().is_none()
        && base.register_index().is_none_or(|register| register < body.params)
}

/// The entry a target's body takes before what its call needs (a frame, an environment,
/// kept arguments) decides further: the first form it has of those a call answers without
/// activating it, else `CallEntry::Activates`. A class constructor's calls throw on the
/// general path, so they take a frame; only its constructions activate frameless
/// (`lane_script_construct`).
pub(super) fn body_entry(body: &Function, constants: &[crate::bytecode::Constant]) -> CallEntry {
    if body.is_class_constructor {
        CallEntry::Frame
    } else if body.code.first().is_some_and(|packed| returning_kind(decoded_instruction(body, *packed)) == Some(RETURNING_FIELD)) {
        CallEntry::Reads
    } else if element_read_body(body) {
        CallEntry::ReadsElement
    } else if forwarding_read_body(body) {
        CallEntry::ForwardsRead
    } else if capture_read_body(body) {
        CallEntry::ReadsCapture
    } else if this_read_body(body) {
        CallEntry::ReadsThis
    } else if super::capture_append::recognizes(body, constants) {
        CallEntry::AppendsCapture
    } else {
        CallEntry::Activates
    }
}

/// Whether `body` is `GetField` of `this` or a parameter, `GetField` of a method of that,
/// moves of its parameters in order into a call window, and the call of that method on the
/// first field returning its completion (`CallEntry::ForwardsRead`), as `at(i) { return
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
    let [field, method, call] = [field, method, call].map(|packed| decoded_instruction(body, *packed));
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
    field.op().is_get_field()
        && field.result_flags() == 0
        && field.result_register() >= body.params
        && survives_copies(field.result_register())
        && reads_this_or_parameter(FieldBase(field.b()), body)
}

/// A `GetField` of `field`'s value, into another register past the parameters that the
/// call's copies leave.
fn reads_method(method: WideInstruction, field: WideInstruction, body: &Function, survives_copies: impl Fn(u16) -> bool) -> bool {
    method.op().is_get_field()
        && method.result_flags() == 0
        && method.result_register() >= body.params
        && method.result_register() != field.result_register()
        && survives_copies(method.result_register())
        && FieldBase(method.b()).register_index() == Some(field.a())
}

/// Each parameter, in order, moved to its place in the call window at `window`.
fn moves_parameters(body: &Function, moves: &[crate::bytecode::Instr], window: u16) -> bool {
    moves.iter().zip(0..body.params).all(|(&packed, parameter)| {
        let moved = decoded_instruction(body, packed);
        moved.op().is_move() && moved.b() == parameter && moved.a() == window + parameter
    })
}

/// The call of `method` on `field` with every parameter, returning its completion.
fn calls_method(call: WideInstruction, method: WideInstruction, field: WideInstruction, body: &Function) -> bool {
    call.op().is_call()
        && call.result_flags() == crate::bytecode::RETURN_REGISTER
        && call.b() == method.a()
        && call.c() == field.a()
        && ImmediateLayout::argument_count(call.imm()) == body.params
}

/// Whether `body` is `GetField` of `this` or a parameter, `GetIndex` of its result by a
/// parameter, and `Return` of that (`CallEntry::ReadsElement`), as `at(i) { return
/// this.items[i]; }` compiles.
fn element_read_body(body: &Function) -> bool {
    let [field, element, done, ..] = body.code.as_slice() else {
        return false;
    };
    let [field, element, done] = [field, element, done].map(|packed| decoded_instruction(body, *packed));
    field.op().is_get_field()
        && !field.returns_from_frame()
        && reads_this_or_parameter(FieldBase(field.b()), body)
        && element.op().is_get_index()
        && element.b() == field.a()
        && element.c() < body.params
        && done.op().is_return()
        && done.a() == element.a()
}

/// The first instruction of `body` and the one after it, when it has two.
fn leading_pair(body: &Function) -> Option<[WideInstruction; 2]> {
    let [first, second, ..] = body.code.as_slice() else {
        return None;
    };
    Some([first, second].map(|packed| decoded_instruction(body, *packed)))
}

/// Whether `body` is `LoadCapture` and `Return` of it (`CallEntry::ReadsCapture`), as
/// `() => binding` compiles.
fn capture_read_body(body: &Function) -> bool {
    leading_pair(body).is_some_and(|[load, done]| {
        load.op().is_load_capture() && !load.returns_from_frame() && done.op().is_return() && done.a() == load.a()
    })
}

/// Whether `body` is `LoadThis` and `Return` of it (`CallEntry::ReadsThis`): a function's own
/// `this`, which neither an arrow's lexical one nor a constructor's binding complicates.
fn this_read_body(body: &Function) -> bool {
    !body.is_arrow
        && !body.is_class_constructor
        && leading_pair(body).is_some_and(|[load, done]| {
            load.op().is_load_this() && !load.returns_from_frame() && done.op().is_return() && done.a() == load.a()
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

/// Whether `body` is a leaf (`CallEntry::Leaf`), and then what its records read of the
/// context (`LeafForm`): every instruction's lane record writes registers, captures or
/// nothing, reads no frame state and no activation, and leaves the lane for anything else
/// (its general path), or activates a getter, which makes the leaf an activation first; no
/// backward jump, so no safepoint runs in it. That is a register move or constant, a number
/// or string operation of registers and constants, a capture's load or store, `this`, a
/// field read or store of no private name through a register or `this`, a global name's
/// load, a forward jump and the return, of a register, a constant or an operator's value. A
/// body that keeps frame state (locals, handlers, `arguments`, dynamic scope) is none.
/// `private` says whether an atom is a private name, whose access the running function
/// decides (`lane_cached_field_miss`, `lane_private_brand`).
pub(super) fn leaf_body(body: &Function, code: &ResidualProgram, private: impl Fn(crate::bytecode::Atom) -> bool) -> Option<LeafForm> {
    if keeps_frame_state(body) {
        return None;
    }
    let mut form = PLAIN_LEAF;
    for (pc, instruction) in body.instructions().enumerate() {
        let reads = leaf_reads(instruction, body, code, &private)?;
        if instruction.op().immediate_role() == ImmediateRole::JumpTarget && instruction.imm() as usize <= pc {
            return None;
        }
        // Each form makes its own what the forms before it do, and more.
        form = form.max(reads);
    }
    Some(form)
}

/// What `instruction`'s lane record reads of the context in a leaf (`leaf_body`), or `None`
/// when no leaf runs it.
fn leaf_reads(instruction: WideInstruction, body: &Function, code: &ResidualProgram,
    private: &impl Fn(crate::bytecode::Atom) -> bool) -> Option<LeafForm> {
    Some(match instruction.op() {
        op if op.is_leaf_value() || op.is_leaf_jump() => PLAIN_LEAF,
        op if op.is_leaf_binary() => return binary_leaf(instruction),
        op if op.traits().has(Traits::THIS_LEAF) => CONTEXT_LEAF,
        op if op.addresses_capture() => CAPTURE_LEAF,
        // A store past its caches (a setter, a proxy's trap, a frozen target) runs in
        // place (`lane_set_field_in_place`) or on the general path, either of which makes
        // the leaf an activation first.
        op if op.is_field_store() => return store_leaf(instruction, private),
        // A global name's first load finds the running function (`lane_load_name_first`),
        // which makes the leaf an activation first; its later loads read the realm alone.
        op if op.traits().has(Traits::NAME_LEAF) && body.names_resolve_statically(code) => PLAIN_LEAF,
        // A returning read requickens by its site's tier in its leaf form
        // (`lane_returning_field_first`, `LaneView::specialize_leaf_returns`).
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
fn store_leaf(instruction: WideInstruction, private: &impl Fn(crate::bytecode::Atom) -> bool) -> Option<LeafForm> {
    if private(instruction.imm()) {
        return None;
    }
    Some(if instruction.field_store_register().is_some() { PLAIN_LEAF } else { CONTEXT_LEAF })
}

/// A field read of no private name through a base a leaf reads; a sink stores the value
/// to a field of `this` (`lane_get_field_sink`).
fn field_leaf(instruction: WideInstruction, code: &ResidualProgram, private: &impl Fn(crate::bytecode::Atom) -> bool) -> Option<LeafForm> {
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
