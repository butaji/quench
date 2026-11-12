//! A `throw` that a handler of the same function covers, lowered as its catch: exceptions do
//! not leave native code. The thrown value goes to the handler's register and control to the
//! handler's block, which is what `Vm::exception_handler_target` does for a thrown value, so
//! the interpreter never re-executes the `throw` to find the handler.
use super::*;
use crate::bytecode::{Function, ThrownValueSlot};
use super::super::lane_unit::{Origin, Unit};

/// Where a throw continues: the unit PC of its handler's block, and the register that
/// block's binding is promoted to (`ThrownValueSlot::Register`), none for a binding-less catch.
#[derive(Clone, Copy, Debug)]
pub(super) struct Catch {
    pub(super) target: usize,
    pub(super) slot: Option<Register>,
}

/// Whether `unit` runs `pc` as the function's own record `pc`: the only record whose handler
/// table is the caller's, with its PCs the unit's.
fn is_own_record(unit: &Unit, pc: usize) -> bool {
    unit.origins.get(pc) == Some(&Origin::Caller(pc as u32))
}

/// The register a handler's thrown value goes to, where native code can store it: `None`
/// for a handler it cannot serve (a `finally` return, a `with` scope to unwind, or a binding
/// that lives in a frame local), `Some(None)` for a catch that binds nothing.
fn handler_slot(unit: &Unit, handler: crate::bytecode::Handler) -> Option<Option<Register>> {
    if handler.return_target.is_some() || handler.with_depth != 0 { return None; }
    match handler.slot {
        None => Some(None),
        Some(ThrownValueSlot::Register(register)) if register < unit.registers => Some(Some(register)),
        Some(ThrownValueSlot::Register(_) | ThrownValueSlot::Local(_)) => None,
    }
}

/// The throws of `unit`'s function that native code catches itself, by unit PC. A throw
/// whose handler is not admitted (`handler_slot`), lies outside the lowered range, or is
/// not before its handler keeps leaving at the throw, which the interpreter raises exactly.
pub(super) fn catch_edges(function: &Function, unit: &Unit, coverage: Coverage, completion: Completion) -> BTreeMap<usize, Catch> {
    let mut catches = BTreeMap::new();
    let scopes_unwind = function.is_generator || function.is_async || function.has_dynamic_scope_access();
    if !completion.status_and_pc() || scopes_unwind || !unit.frames.is_empty() || function.handlers.is_empty() {
        return catches;
    }
    for (pc, instruction) in unit.instructions.iter().enumerate() {
        if !instruction.op().class().eq(Class::Throw) || instruction.result_flags() != 0
            || instruction.register_a() >= unit.registers
            || !coverage.lowered(pc) || !is_own_record(unit, pc) { continue; }
        let Some(handler) = function.handler_at(pc as u32) else { continue };
        let target = handler.target as usize;
        let Some(slot) = handler_slot(unit, handler) else { continue };
        if target > pc && target < unit.instructions.len() && coverage.lowered(target) && is_own_record(unit, target) {
            catches.insert(pc, Catch { target, slot });
        }
    }
    catches
}

impl Lowering<'_, '_> {
    /// The throw at `pc` as a jump to its handler: the thrown value, boxed once, stored to the
    /// handler's register, and the edge into the handler's block conformed as any edge is.
    /// `None` where the throw has no admitted handler (it leaves at the throw).
    pub(super) fn caught_throw(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> Option<bool> {
        let Catch { target, slot } = self.catches.get(&pc).copied()?;
        let value = self.tagged(state, instruction.register_a());
        if let Some(slot) = slot { self.set_tagged(state, slot, value); }
        self.continue_at(state, target);
        Some(false)
    }
}
