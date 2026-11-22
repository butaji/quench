//! The exact shape a register's object had before a call, held again past it.
//!
//! A call may move any object out of its shape, so `State::call_effect` retires every register's
//! shape fact, and the next access proves the shape anew. Where the VM holds the shape stable
//! (`Vm::shape_is_stable`: no object has left it yet) the code depends on its departures instead
//! (`StableUses::shapes`): the first object to leave it retires the code, which sets the
//! invalidation word, so a use after a call reads that word once (`StableHold::hold_shape`) and
//! needs no shape test. What a call does not keep is the layout: an object's storage form and
//! arena address move without a change of shape, so they are proved again (`kept_layout`).

use super::*;

impl LocalProofs {
    /// Notes the exact shape each of `values` has, which a call retires.
    pub(crate) fn keep_shapes(&mut self, values: &[RegisterFact]) {
        for (register, fact) in values.iter().enumerate() {
            if let Some(shape) = fact.value.ty.object_shape() {
                self.kept_shapes.insert(register as Register, shape);
            }
        }
    }
}

#[quench_macros::ext(StateOps2)]
impl State {
    /// The exact shape `register`'s object has: its type's, or the one it had before a call.
    fn shape_of(&self, register: Register) -> Option<u32> {
        let ty = self.values[usize::from(register)].value.ty;
        ty.object_shape().or_else(|| ty.is_direct_object_family()
            .then(|| self.proofs.kept_shapes.get(&register).copied()).flatten())
    }

    /// The kept shapes of this state past a join with `incoming`, before their registers' facts
    /// are joined: a register keeps a shape both arrivals gave its object, which needs holding
    /// where either gave it as kept; any other register keeps none.
    pub(crate) fn join_kept_shapes(&mut self, incoming: &State) {
        let registers: BTreeSet<Register> = self.proofs.kept_shapes.keys().chain(incoming.proofs.kept_shapes.keys()).copied().collect();
        for register in registers {
            let typed = |state: &State| state.values[usize::from(register)].value.ty.object_shape();
            match (self.shape_of(register), incoming.shape_of(register)) {
                (Some(shape), Some(other)) if shape == other && !(typed(self) == Some(shape) && typed(incoming) == Some(shape)) => {
                    self.proofs.kept_shapes.insert(register, shape);
                }
                _ => { self.proofs.kept_shapes.remove(&register); }
            }
        }
    }
}

impl EmitFrame<'_> {
    /// `headers::object_header` of `owner`'s object, after the shape its register's object had
    /// before the last call is held again.
    pub(crate) fn object_header(&mut self, builder: &mut FunctionBuilder<'_>, entry: &Entry, owner: Option<Register>,
        read: bool, miss: ir::Block) -> ObjectHeader {
        if let Some(register) = owner { self.hold_kept_shape(builder, register, miss); }
        headers::object_header(builder, self.registers, self.state, (entry, self.payload), owner, read, miss)
    }

    /// Makes the shape `register`'s object had before a call its type again where the VM holds it
    /// stable (`StableHold::hold_shape`), leaving at `miss` where a departure retired the code.
    fn hold_kept_shape(&mut self, builder: &mut FunctionBuilder<'_>, register: Register, miss: ir::Block) {
        let Some(shape) = self.state.proofs.kept_shapes.remove(&register) else { return };
        let fact = self.state.values[usize::from(register)];
        // An unboxed register has no tagged view to keep (`headers::object_header`); a register
        // whose type names a shape holds that one (`prove_shape`).
        if fact.value.ty.object_shape().is_some() || fact.value.representation != Representation::Tagged || !fact.tagged.current() { return; }
        let Some(facts) = self.fields.as_mut() else { return };
        if facts.stable.hold_shape((builder, self.helpers), self.state, miss, shape).is_none() { return; }
        self.state.values[usize::from(register)] = RegisterFact { tagged: fact.tagged,
            ..RegisterFact::definition(Type::direct_object(shape), Representation::Tagged) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object(shape: u32) -> RegisterFact {
        RegisterFact::definition(Type::direct_object(shape), Representation::Tagged)
    }

    #[test]
    fn a_call_retires_each_shape_and_keeps_it_for_the_register() {
        let mut state = State::canonical(2);
        state.values[0] = object(7);
        state.call_effect();
        assert_eq!(state.values[0].value.ty, Type::DIRECT_OBJECT_FAMILY);
        assert_eq!(state.proofs.kept_shapes.iter().collect::<Vec<_>>(), [(&0, &7)]);
    }

    #[test]
    fn a_join_keeps_a_shape_both_arrivals_gave_the_object_and_drops_any_other() {
        let family = RegisterFact::definition(Type::DIRECT_OBJECT_FAMILY, Representation::Tagged);
        let mut after_call = State::canonical(3);
        after_call.values = vec![object(7), object(3), object(5)];
        after_call.call_effect();
        after_call.values[2] = object(5);
        after_call.proofs.kept_shapes.remove(&2);
        let mut other = State::canonical(3);
        other.values = vec![object(7), family, object(5)];
        other.proofs.kept_shapes.insert(1, 4);
        after_call.join_kept_shapes(&other);
        assert_eq!(after_call.proofs.kept_shapes.iter().collect::<Vec<_>>(), [(&0, &7)],
            "register 0 holds shape 7 on both arrivals, once kept; 1 holds two, 2 holds 5 as a type");
    }
}
