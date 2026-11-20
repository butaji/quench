//! The prologue of a constructor: the fields its straight-line code stores on the object it makes.

use super::effects::decoded;
use crate::bytecode::{ControlEdge, Function, Successors};
use crate::staged::Operation;

/// The fields a plain constructor stores on `this` before any code can observe the object, in
/// store order: a `new` of it allocates an object of that shape. `None` where `function` is not a
/// plain constructor.
pub(super) fn prologue_fields(function: &Function, successors: &Successors) -> Option<Vec<u32>> {
    let plain = function.constructible
        && !function.is_arrow
        && !function.is_async
        && !function.is_generator
        && !function.derived_constructor
        && !function.is_class_constructor;
    plain.then(|| fields(function, successors))
}

/// The fields of the straight-line prologue, each once. Only stores on `this` and operations
/// that reach no object of the function run in it: a call, a property read or a store of
/// anything else ends it.
fn fields(function: &Function, successors: &Successors) -> Vec<u32> {
    let mut atoms: Vec<u32> = Vec::new();
    for pc in 0..function.code.len() {
        if !successors.at(pc).eq([(pc + 1, ControlEdge::Normal)]) {
            break;
        }
        let instruction = function.instruction_at(pc);
        match decoded(instruction) {
            Some(Operation::SetField { base: None, atom, .. }) => {
                if !atoms.contains(&atom) {
                    atoms.push(atom);
                }
            }
            Some(
                Operation::Move { .. }
                | Operation::LoadThis { .. }
                | Operation::Nop
                | Operation::Number(_)
                | Operation::LoadEnvLocal { .. }
                | Operation::LoadLocalPlain { .. }
                | Operation::LoadCapture { .. },
            ) => {}
            _ if Operation::<(), ()>::decode_constant_load(instruction).is_some() => {}
            _ => break,
        }
    }
    atoms
}
