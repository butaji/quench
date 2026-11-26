//! What one instruction does to a function's register facts, and to the bindings it writes.

use super::scope::Scope;
use super::value::{Fact, Key, Origin};
use crate::bytecode::{Operand, OperandKind, Traits, WideInstruction};
use crate::typed_ir::Type;
use crate::staged::protocol_literals::LiteralStep;
use crate::staged::{Allocation, DecodeContext, NumberOperation, Operation, Protocol};

/// The function an instruction belongs to, and where its parameters end.
pub(super) struct Ctx<'a> {
    /// The function's id: the owner of its own environment and frame slots.
    pub(super) id: u32,
    /// The PC after the parameters' initialization: stores before it bind arguments.
    pub(super) parameter_end: usize,
    /// Whether the function is a root, whose `this` is the global object in a script.
    pub(super) is_root: bool,
    pub(super) scope: &'a Scope,
    /// Bindings the census proved hold one closure, so their loads carry its fact too.
    pub(super) resolved: &'a std::collections::BTreeMap<Key, u32>,
    pub(super) registers: usize,
}

/// What an instruction does beyond its registers: the bindings it writes, and whether it makes
/// writes the analysis cannot follow.
#[derive(Default)]
pub(super) struct Events {
    pub(super) writes: Vec<(Key, Fact)>,
    pub(super) wild_global: bool,
    pub(super) wild_owner: bool,
    /// Whether a store through a key the analysis cannot name may write a named property.
    pub(super) wild_member: bool,
}

/// The staged operation of `instruction`, with no constants or sites resolved.
pub(super) fn decoded(instruction: WideInstruction) -> Option<Operation<(), ()>> {
    Operation::<(), ()>::decode_completed(instruction, DecodeContext::EMPTY)
        .or_else(|| Operation::<(), ()>::decode_literal_completed_with(instruction, None, ()))
        .map(|(operation, _)| operation)
}

/// Applies `instruction` at `pc` to `state`, the facts before it, and returns its binding effects.
pub(super) fn step(ctx: &Ctx<'_>, pc: usize, instruction: WideInstruction, state: &mut [Fact]) -> Events {
    let mut events = Events::default();
    let operation = decoded(instruction);
    let value = value_of(ctx, instruction, operation, state, &mut events);
    writes(ctx, pc, instruction, operation, state, &mut events);
    let defined = instruction.defined_register();
    for register in instruction.written_registers() {
        state[usize::from(register)] = if Some(register) == defined { value } else { Fact::UNKNOWN };
    }
    events
}

/// Whether `instruction` reads a global name plainly (`LoadName`, `LoadNameCall`).
fn is_global_load(instruction: WideInstruction) -> bool {
    let traits = instruction.op().traits();
    traits.has(Traits::GLOBAL_NAME_ACCESS) && traits.has(Traits::NAME_LOAD)
}

/// The fact of a load from `key`: the closure the census proved there, else the binding.
fn resolved(ctx: &Ctx<'_>, key: Key) -> Fact {
    ctx.resolved.get(&key).copied().map_or_else(|| Fact::from(Origin::Binding(key)), Fact::closure)
}

/// The fact of the value `instruction` defines, read from the facts before it.
fn value_of(
    ctx: &Ctx<'_>,
    instruction: WideInstruction,
    operation: Option<Operation<(), ()>>,
    state: &[Fact],
    events: &mut Events,
) -> Fact {
    if let Some((_, index)) = Operation::<(), ()>::decode_constant_load(instruction) {
        if let Some(atom) = ctx.scope.constant_atoms.get(index).copied().flatten() { return Fact::atom(atom); }
        return ctx.scope.constants.get(index).copied().flatten().map_or(Fact::UNKNOWN, Fact::number);
    }
    if matches!(instruction.op().class(), crate::bytecode::Class::GetField) && instruction.result_flags() == 0 && !instruction.writes_current_this()
        && let Some(crate::bytecode::FieldLookup::Atom { atom, base, .. }) = instruction.field_lookup()
    {
        let owner = base.register_index().map(|register| state[usize::from(register)]);
        return match owner.and_then(|fact| fact.ty.body_index()) {
            // `Class.prototype`: the class's prototype object.
            Some(class) if ctx.scope.prototype == Some(atom) => Fact::prototype(class),
            _ => match owner.and_then(Fact::class_site) {
                Some((class, instance)) => Fact::member_on(class, instance, atom),
                None => Fact::from(Origin::Member(atom)),
            },
        };
    }
    if instruction.op() == crate::bytecode::Op::Construct {
        let callee = state.get(usize::from(instruction.register_b())).copied().unwrap_or(Fact::UNKNOWN);
        if let Some(class) = callee.ty.body_index() { return Fact::instance(class); }
    }
    if is_global_load(instruction) {
        let atom = instruction.atom_index();
        events.wild_global |= ctx.scope.is_global_object(atom);
        return resolved(ctx, Key::Global(atom));
    }
    // A closure record decodes under its allocation's proof (`Decoder::Literal`), not as an operation.
    if let Some((_, Allocation::Closure { function })) = Allocation::decode(instruction, &[]) {
        return Fact::closure(function);
    }
    match operation {
        Some(Operation::Move { source, .. } | Operation::StoreEnvLocal { source, .. } | Operation::StoreLocalPlain { source, .. }) => {
            state[usize::from(source)]
        }
        Some(Operation::LoadEnvLocal { slot, .. } | Operation::LoadLocalPlain { slot, .. }) => {
            resolved(ctx, Key::Slot { owner: ctx.id, slot })
        }
        Some(Operation::LoadCapture { depth, slot, .. }) => ctx
            .scope
            .owner(ctx.id, depth)
            .map_or(Fact::UNKNOWN, |owner| resolved(ctx, Key::Slot { owner, slot })),
        Some(Operation::Allocate { allocation: Allocation::Closure { function }, .. }) => Fact::closure(function),
        Some(Operation::Number(number)) => number_value(ctx.scope, state, number),
        _ => Fact::UNKNOWN,
    }
}

/// The fact of a numeric operand: its register's, or the Number of its constant.
fn operand_fact(scope: &Scope, state: &[Fact], operand: Operand) -> Fact {
    match (operand.register_index(), operand.kind()) {
        (Some(register), _) => state[usize::from(register)],
        (None, Some(OperandKind::Constant)) => {
            scope.constants.get(usize::from(operand.payload())).copied().flatten().map_or(Fact::UNKNOWN, Fact::number)
        }
        _ => Fact::UNKNOWN,
    }
}

/// The fact of the Number a numeric operation defines, when its operands are Numbers: the
/// operation of an operand that is not one may run guest code or yield a string.
fn number_value(scope: &Scope, state: &[Fact], number: NumberOperation) -> Fact {
    let fact = |operand: Operand| operand_fact(scope, state, operand);
    let both = |left: Operand, right: Operand| fact(left).is_number() && fact(right).is_number();
    let one = |source: Operand| fact(source).is_number();
    let register = |source: u16| state[usize::from(source)];
    match number {
        NumberOperation::Arithmetic { left, right, .. }
        | NumberOperation::Remainder { left, right, .. }
        | NumberOperation::Extreme { left, right, .. }
            if both(left, right) =>
        {
            Fact::of(Type::NUMBER)
        }
        NumberOperation::Bitwise { left, right, .. } | NumberOperation::Imul { left, right, .. }
            if both(left, right) =>
        {
            Fact::of(Type::INT32)
        }
        NumberOperation::Unary { source, .. } | NumberOperation::Math { source, .. } if one(source) => Fact::of(Type::NUMBER),
        NumberOperation::BitwiseNot { source, .. } if one(source) => Fact::of(Type::INT32),
        NumberOperation::IncDec { source, .. } if register(source).is_number() => Fact::of(Type::NUMBER),
        NumberOperation::ToNumeric { source, .. } if register(source).is_number() => register(source),
        _ => Fact::UNKNOWN,
    }
}

/// Records the bindings `instruction` writes, from the facts before it, and the writes it makes
/// that the analysis cannot follow.
fn writes(
    ctx: &Ctx<'_>,
    pc: usize,
    instruction: WideInstruction,
    operation: Option<Operation<(), ()>>,
    state: &[Fact],
    events: &mut Events,
) {
    unfollowed_writes(ctx, instruction, operation, events);
    binding_stores(ctx, pc, operation, state, events);
    member_stores(instruction, operation, state, events);
}

/// Records the named property stores `instruction` makes (`Key::Member`), with the facts they store; a store through a
/// key that is no Number may name any property (`Events::wild_member`).
fn member_stores(instruction: WideInstruction, operation: Option<Operation<(), ()>>, state: &[Fact], events: &mut Events) {
    if let Some(Operation::Protocol { step: Protocol::Literal(LiteralStep::DefinePropertyRecord { value, .. }), .. }) = operation {
        let target = state[usize::from(instruction.register_b())];
        let key = state[usize::from(instruction.register_c())];
        if let (Some(atom), Some((class, instance))) = (key.atom_key(), target.class_site()) {
            events.writes.push((Key::MemberOn { class, instance, atom }, state[usize::from(value)]));
        }
    }
    match operation {
        Some(Operation::SetField { value, atom, .. }) => events.writes.push((Key::Member(atom), state[usize::from(value)])),
        Some(Operation::StoreField { atom, .. } | Operation::FieldStoreMiss { atom, .. } | Operation::FieldAdd { atom, .. }) => {
            events.writes.push((Key::Member(atom), Fact::UNKNOWN));
        }
        Some(Operation::ElementWrite { key, .. }) if !state[usize::from(key)].is_number() => events.wild_member = true,
        Some(Operation::IndexWrite { .. } | Operation::IndexSetHelper { .. } | Operation::Delete { .. }) => events.wild_member = true,
        _ if instruction.op().is_field_store() && !matches!(operation, Some(Operation::SetField { .. })) => {
            events.writes.push((Key::Member(instruction.atom_index()), Fact::UNKNOWN));
        }
        _ => {}
    }
}

/// Marks the writes the analysis cannot follow: name operations other than plain loads and
/// global stores (`with`, `delete`, sloppy declarations), writes to a numeric local, and writes
/// through a computed key, which may name a global or an element of an arguments object.
fn unfollowed_writes(ctx: &Ctx<'_>, instruction: WideInstruction, operation: Option<Operation<(), ()>>, events: &mut Events) {
    let traits = instruction.op().traits();
    let global_store = matches!(operation, Some(Operation::GlobalWrite { .. }));
    if traits.has(Traits::NAME_OPERATION)
        && !traits.has(Traits::NAME_LOAD)
        && !traits.has(Traits::ENVIRONMENT_CLONE)
        && !global_store
    {
        events.wild_global = true;
        events.wild_owner = true;
    }
    if instruction.writes_numeric_local() {
        events.wild_owner = true;
    }
    match operation {
        Some(
            Operation::ElementWrite { .. }
            | Operation::IndexWrite { .. }
            | Operation::IndexSetHelper { .. }
            | Operation::Delete { .. },
        ) => events.wild_global = true,
        Some(Operation::LoadThis { .. }) if ctx.is_root => events.wild_global = true,
        _ => {}
    }
}

/// Records the binding stores `operation` makes, with the facts they store.
fn binding_stores(ctx: &Ctx<'_>, pc: usize, operation: Option<Operation<(), ()>>, state: &[Fact], events: &mut Events) {
    match operation {
        // A captured or global binding is no parameter: a store of it stays as the flow proves
        // it even in the prologue, where a register store may be overwritten by an argument.
        Some(Operation::StoreEnvLocal { source, slot, .. }) => {
            write(ctx, pc, true, events, Key::Slot { owner: ctx.id, slot }, state[usize::from(source)]);
        }
        Some(Operation::StoreLocalPlain { source, slot, .. }) => {
            write(ctx, pc, false, events, Key::Slot { owner: ctx.id, slot }, state[usize::from(source)]);
        }
        Some(Operation::StoreCapture { source, depth, slot }) => match ctx.scope.owner(ctx.id, depth) {
            Some(owner) => write(ctx, pc, true, events, Key::Slot { owner, slot }, state[usize::from(source)]),
            None => {
                events.wild_global = true;
                events.wild_owner = true;
            }
        },
        Some(Operation::IncDecLocal { local, .. }) => {
            write(ctx, pc, true, events, Key::Slot { owner: ctx.id, slot: local }, Fact::UNKNOWN);
        }
        Some(Operation::GlobalWrite { source, atom, .. }) => {
            write(ctx, pc, true, events, Key::Global(atom), state[usize::from(source)]);
        }
        Some(
            Operation::SetField { atom, .. }
            | Operation::StoreField { atom, .. }
            | Operation::FieldStoreMiss { atom, .. }
            | Operation::FieldAdd { atom, .. },
        ) => write(ctx, pc, true, events, Key::Global(atom), Fact::UNKNOWN),
        _ => {}
    }
}

/// Records a write of `fact` to `key` at `pc`; a store before the parameters end binds an argument.
fn write(ctx: &Ctx<'_>, pc: usize, stable: bool, events: &mut Events, key: Key, fact: Fact) {
    let fact = if !stable && pc < ctx.parameter_end { Fact::UNKNOWN } else { fact };
    events.writes.push((key, fact));
}
