//! A named read of an own member of a constant-marked object (`Vm::own_constant_of`): the read
//! folds to the value the object held. The receiver is guarded by identity, and the code depends
//! on the object's exact shape as a prototype's values are held (`hold_holder`), so the object's
//! next write retires the body (`Object::note_constant_write`).
use super::*;

impl Lowering<'_, '_> {
    /// Folds the read at `pc` of `instruction`, a plain single-name read of a register holding the
    /// own-constant object the VM holds for that name (`VmFacts::online`'s `own_constant`), to the
    /// value that object held: a receiver that is not the object leaves through the guard to the
    /// shared body. `None`, emitting nothing, where the read is not one or nothing holds it.
    pub(crate) fn fold_own_constant(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> Option<bool> {
        let online = self.vm.online?;
        if !self.holds_holders() || instruction.returns_from_frame() || instruction.writes_current_this() { return None; }
        let base = self.field_base(pc, instruction)?;
        let register = base.register_index().filter(|register| usize::from(*register) < state.values.len())?;
        let (parts, sink) = self.field_chain(instruction)?;
        let &[(atom, _)] = parts.as_slice() else { return None };
        if sink.is_some() { return None; }
        let own = (online.own_constant)(atom)?;
        let exit = self.guard_exit(state, pc as u32);
        let bits = self.tagged(state, register);
        let is_object = self.is_identity(bits, own.object);
        emit_js_guard(&mut self.builder, is_object, exit);
        self.hold_holder_any(state, exit, own.shape);
        let constant = self.identity_constant(own.value);
        self.define(state, instruction.result_register(), constant, RegisterFact::definition(Type::immediate(own.value), Representation::Tagged));
        Some(true)
    }
}
