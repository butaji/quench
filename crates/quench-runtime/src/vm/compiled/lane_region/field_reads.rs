//! How a named field read defines its register (`Lowering::get_field`): a whole-program
//! prototype value folded under its guard, or the read value with the int32 view a slot
//! that has only held int32 values proves.
use super::*;

impl Lowering<'_, '_> {
    /// Whether a reached instruction reads or stores a named field: only then does the body
    /// look at the receiver's record and the property arena at entry.
    pub(super) fn accesses_fields(&self, instructions: &[WideInstruction]) -> bool {
        instructions.iter().enumerate().any(|(pc, instruction)| self.facts.reached(pc) && instruction.op().is_named_field_access())
    }

    /// The entry facts of a body that never looks at the receiver's record: no object, no
    /// shape, no arena.
    pub(super) fn absent_receiver(&mut self) -> LaneReceiver {
        let receiver = LaneReceiver { object: self.builder.declare_var(types::I64), shape: self.builder.declare_var(types::I32),
            inline: self.builder.declare_var(types::I8), properties: self.builder.declare_var(types::I64) };
        let (zero, none, outside) = (self.word_const(0), self.int_const(types::I32, i64::from(super::super::NO_FIELD_RECEIVER)), self.int_const(types::I8, 0));
        for (variable, value) in [(receiver.object, zero), (receiver.properties, zero), (receiver.shape, none), (receiver.inline, outside)] {
            self.builder.def_var(variable, value);
        }
        receiver
    }

    /// Folds the read of `first` through `base` (its register and whether it is a proved
    /// receiver) into `destination` when the closed program gives the field one value: the
    /// field is read once here as data and required to hold it, so a changed prototype
    /// fails the guard and leaves to the shared body. False, emitting nothing, otherwise.
    pub(super) fn fold_frozen_field(&mut self, state: &mut State, (register, receiver): (Option<Register>, bool),
        first: (Atom, u16), destination: Register, exit: ir::Block) -> bool {
        if !self.division().folds(crate::staged::division::FROZEN_FIELD) { return false; }
        let Some(expected) = self.vm.frozen_fields.and_then(|fields| fields.get(&first.0)).copied() else { return false };
        let actual = self.base_field_read(state, register, receiver, first, exit);
        let same = self.is_identity(actual, expected);
        emit_js_guard(&mut self.builder, same, exit);
        let value = self.identity_constant(expected);
        self.define(state, destination, value, RegisterFact::definition(Type::immediate(expected), Representation::Tagged));
        true
    }

    /// Defines `instruction`'s register as the `value` it read: `field` names the slot a
    /// one-read lookup (`single`) reads, whose proof `known` held before. A slot that has
    /// only held int32 values is read as one: its value's tag is tested here, once, and a
    /// value it no longer holds leaves through `exit` before the read is defined.
    pub(super) fn define_field_read(&mut self, state: &mut State, instruction: WideInstruction, value: ir::Value,
        (field, single, known): (Option<(Option<Register>, Atom)>, bool, Option<FieldValue>), exit: ir::Block) {
        let integer = single && known.is_some_and(|field| field.ty == Type::INT32);
        let ty = if integer || int32_read(self.program, self.fields, instruction) {
            if !integer {
                let integer = is_int_tagged(&mut self.builder, value);
                emit_js_guard(&mut self.builder, integer, exit);
            }
            Type::INT32
        } else { Type::ANY };
        // A fused chain can consume an existing first-slot proof, but its guest
        // result names the final slot. Do not retain a new anonymous intermediate
        // solely for forwarding; the existing proof needs no duplicate update.
        if let Some((owner, atom)) = field.filter(|_| single) {
            let read = FieldValue { owner, atom, value, ty };
            match state.proofs.fields.iter_mut().find(|field| field.owner == owner && field.atom == atom) {
                Some(field) => *field = read,
                None => state.proofs.fields.push(read),
            }
        }
        let destination = instruction.result_register();
        self.set_tagged(state, destination, value);
        if ty == Type::INT32 {
            let int = self.ins().ireduce(types::I32, value);
            self.define_int_view(state, destination, value, int);
        }
    }

    /// Gives `destination`, defined as the tagged `value`, the int32 view `int` of it.
    pub(super) fn define_int_view(&mut self, state: &mut State, destination: Register, value: ir::Value, int: ir::Value) {
        let variable = self.int_var(usize::from(destination));
        self.builder.def_var(variable, int);
        state.values[usize::from(destination)] = RegisterFact { tagged: TaggedView::Value(value),
            ..RegisterFact::definition(Type::INT32, Representation::Int32) };
    }
}
