//! The exact shape an object had before a call, held again past it, by the object's resolved word.
//!
//! A call may move any object out of its shape, so `State::call_effect` retires every register's
//! shape fact, and the next access proves the shape anew. Where the VM holds the shape stable
//! (`Vm::shape_is_stable`: no object has left it yet) the code depends on its departures instead
//! (`StableUses::shapes`): the first object to leave it retires the code, which sets the
//! invalidation word, so a use after a call reads that word once (`StableHold::hold_shape`) and
//! needs no shape test. What a call does not keep is the layout: an object's storage form and
//! arena address move without a change of shape, so they are proved again (`kept_layout`).

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

#[quench_macros::ext(StateOps2)]
impl State {
    /// The kept shapes of this state past a join with `incoming`: an object both arrivals keep the same
    /// shape of stays kept; any other object keeps none.
    pub(crate) fn join_kept_shapes(&mut self, incoming: &State) {
        self.proofs.kept_shapes.retain(|word, shape| incoming.proofs.kept_shapes.get(word).copied() == Some(*shape));
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

    /// Makes the shape the object of `register` had before a call its type again where the VM holds it
    /// stable (`StableHold::hold_shape`), leaving at `miss` where a departure retired the code.
    fn hold_kept_shape(&mut self, builder: &mut FunctionBuilder<'_>, register: Register, miss: ir::Block) {
        let fact = self.state.values[usize::from(register)];
        // An unboxed register has no tagged view to keep (`headers::object_header`); a register
        // whose type names a shape holds that one (`prove_shape`).
        if fact.value.ty.object_shape().is_some() || fact.value.representation != Representation::Tagged || !fact.tagged.current() { return; }
        let word = headers::register_word(builder, self.registers, self.state, register);
        let Some(shape) = self.state.proofs.kept_shapes.remove(&word) else { return };
        let Some(facts) = self.fields.as_mut() else { return };
        if facts.stable.hold_shape((builder, self.helpers), self.state, miss, shape).is_none() { return; }
        self.state.proofs.kept_shapes.insert(word, shape);
        self.state.values[usize::from(register)] = RegisterFact { tagged: fact.tagged,
            ..RegisterFact::definition(Type::direct_object(shape), Representation::Tagged) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(index: usize) -> ir::Value { ir::Value::new(index) }

    #[test]
    fn a_call_keeps_each_words_shape_and_a_join_keeps_only_the_shape_both_arrivals_hold() {
        let mut state = State::canonical(2);
        state.proofs.kept_shapes.insert(word(0), 7);
        state.proofs.kept_shapes.insert(word(1), 4);
        state.call_effect();
        assert_eq!(state.proofs.kept_shapes.iter().collect::<Vec<_>>(), [(&word(0), &7), (&word(1), &4)]);
        let mut other = State::canonical(2);
        other.proofs.kept_shapes.insert(word(0), 7);
        other.proofs.kept_shapes.insert(word(1), 5);
        state.join_kept_shapes(&other);
        assert_eq!(state.proofs.kept_shapes.iter().collect::<Vec<_>>(), [(&word(0), &7)],
            "word 0 holds shape 7 on both arrivals; word 1 holds 4 and 5");
    }
}
