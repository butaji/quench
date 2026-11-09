//! How a named field read defines its register (`Lowering::get_field`): a whole-program
//! prototype value folded under its guard, or the read value with the int32 view a slot
//! that has only held int32 values proves.
use super::*;

impl Lowering<'_, '_> {
    /// Folds the read of `first` through `base` (its register and whether it is a proved
    /// receiver) into `destination` when the closed program gives the field one value: the
    /// field is read once here as data and required to hold it, so a changed prototype
    /// fails the guard and leaves to the shared body. False, emitting nothing, otherwise.
    pub(super) fn fold_frozen_field(&mut self, state: &mut State, (register, receiver): (Option<Register>, bool),
        first: (Atom, u16), destination: Register, exit: ir::Block) -> bool {
        let Some(expected) = self.vm.frozen_fields.and_then(|fields| fields.get(&first.0)).copied() else { return false };
        let actual = self.base_field_read(state, register, receiver, first, exit);
        let same = self.ins().icmp_imm_u(IntCC::Equal, actual, expected.to_bits() as i64);
        emit_js_guard(&mut self.builder, same, exit);
        let value = self.ins().iconst(types::I64, expected.to_bits() as i64);
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
