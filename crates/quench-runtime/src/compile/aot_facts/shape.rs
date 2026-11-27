//! The shape changes the closed world sees: each store, define or delete of a named property, and each
//! call that hands an object to code the closed world does not follow, with the object's classes
//! (`Owner`) and what changes (`Change`). Only the class layouts (`class_layout`) read them: the facts
//! of the other analyses never name a shape, so a layout is one reading of the events, not a second record.

use super::effects::{Ctx, this_fact};
use super::flow::{Call, Target};
use super::value::{Origin, PrototypeBase, Word};
use crate::bytecode::WideInstruction;
use crate::staged::protocol_literals::LiteralStep;
use crate::staged::{Operation, Protocol};
use crate::typed_ir::Classes;

/// The object a shape changes: the function's own `this` (a store that names it directly), any other
/// object of the classes the solve names for it (`Classes::Many`: any object), or the prototype object a
/// `prototype` read off a base takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Owner {
    This(Classes),
    Object(Classes),
    Prototype(PrototypeBase),
}

impl Owner {
    /// The classes of the objects this owner may be. A prototype object is no instance, so none.
    pub(super) fn classes(self) -> Option<Classes> {
        match self {
            Self::This(classes) | Self::Object(classes) => Some(classes),
            Self::Prototype(_) => None,
        }
    }
}

/// What a shape does to its object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Change {
    /// A named property `atom` is stored, defined or added.
    Field(u32),
    /// A property whose key the analysis cannot name is stored or deleted, or a builtin that adds
    /// properties is given the object.
    Any,
    /// The object is handed to a call: code the closed world may run with it, and may store to it.
    Escape,
}

/// One change to an object's properties, made at instruction `pc`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Shape {
    pub(super) pc: u32,
    pub(super) owner: Owner,
    pub(super) change: Change,
}

/// The shape the instruction at `pc` makes, from the facts before it (`state`): a named store or define, a
/// delete, or a store through a key the analysis cannot name.
pub(super) fn of_instruction(ctx: &Ctx<'_>, pc: usize, instruction: WideInstruction, operation: Option<&Operation<(), ()>>, state: &[Word]) -> Option<Shape> {
    let pc = pc as u32;
    if instruction.op().is_field_store() {
        let owner = owner_of(ctx, instruction.field_store_register().map(usize::from), state);
        return Some(Shape { pc, owner, change: Change::Field(instruction.atom_index()) });
    }
    match operation? {
        Operation::Delete { base, .. } => Some(Shape { pc, owner: owner_of(ctx, Some(usize::from(*base)), state), change: Change::Any }),
        Operation::ElementWrite { base, key, .. } if !state[usize::from(*key)].is_number() =>
            Some(Shape { pc, owner: owner_of(ctx, Some(usize::from(*base)), state), change: Change::Any }),
        Operation::IndexSetHelper { .. } => Some(Shape { pc, owner: Owner::Object(Classes::Many), change: Change::Any }),
        Operation::Protocol { step: Protocol::Literal(LiteralStep::DefinePropertyRecord { .. }), .. } => {
            let key = state[usize::from(instruction.register_c())];
            let owner = owner_of(ctx, Some(usize::from(instruction.register_b())), state);
            Some(Shape { pc, owner, change: key.atom_key().map_or(Change::Any, Change::Field) })
        }
        _ => None,
    }
}

/// The shapes the call `call` at `pc` makes: the object it hands its callee (`Escape`), and, for a builtin
/// that adds properties to the objects it is given (`Scope::adds_properties`), an `Any` change of them.
pub(super) fn of_call(ctx: &Ctx<'_>, pc: u32, call: &Call) -> Vec<Shape> {
    let arguments = call.arguments.iter().fold(Classes::Never, |classes, argument| classes.join(argument.cell.classes));
    let handed = if call.construct { arguments } else { arguments.join(call.receiver.cell.classes) };
    let mut shapes = Vec::new();
    if handed != Classes::Never {
        shapes.push(Shape { pc, owner: Owner::Object(handed), change: Change::Escape });
    }
    if callee_name(call).is_some_and(|atom| ctx.scope.adds_properties(atom)) {
        shapes.push(Shape { pc, owner: Owner::Object(arguments), change: Change::Any });
    }
    shapes
}

/// The owner a store through register `base` (`None`: the function's `this`) writes to: a prototype object
/// where the register holds one (`Origin::Prototype`), else an object of the classes its cell names.
fn owner_of(ctx: &Ctx<'_>, base: Option<usize>, state: &[Word]) -> Owner {
    match base {
        None => Owner::This(if ctx.is_root { Classes::Many } else { this_fact(ctx).cell.classes }),
        Some(register) => match state[register].origin {
            Some(Origin::Prototype(prototype)) => Owner::Prototype(prototype),
            _ => Owner::Object(state[register].cell.classes),
        },
    }
}

/// The property name a call reads its callee from: a method call's, or a callee read off a named property.
fn callee_name(call: &Call) -> Option<u32> {
    match (call.target, call.callee.origin) {
        (Target::Method(atom), _) => Some(atom),
        (_, Some(Origin::Member(atom) | Origin::MemberOn { atom, .. })) => Some(atom),
        _ => None,
    }
}
