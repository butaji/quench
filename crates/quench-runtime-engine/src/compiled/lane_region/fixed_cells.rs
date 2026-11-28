//! Named reads and stores of a fixed class's receiver (`AotFacts::class_fixed_cell`). Every instance of the class is a class
//! cell of its union layout from birth, so the receiver's cell is its direct reference's granule (`ObjectRecord::FixedCell`),
//! and union slot `k` is the cell's field at `fields_offset + field_size * k`. No record presence, departed, class or shape
//! test is made: the class is the fact. A slot of an absent field holds `DELETED`, which a read answers by the union's
//! `FieldDefault`. A store writes its slot in place, since the union's domain admits every store of the field.

use super::*;
use super::super::object_slots::{emit, ObjectRecord};
use quench_runtime::bytecode::{Atom, Register};
use quench_runtime::compile::aot_facts::{AotFacts, FieldDefault, UnionField};
use quench_runtime::staged::layout_offsets::NATIVE_CLASS_CELL_LAYOUT;
use quench_runtime::staged::machine::{Lane, MachineAccess, MachineScalar, Relation, Unary};
use quench_runtime::Value;

/// Union slot `slot` of the instances of a fixed class, as its displacement from the class cell's address.
fn slot_displacement(slot: usize) -> i64 {
    (NATIVE_CLASS_CELL_LAYOUT.fields_offset + slot * NATIVE_CLASS_CELL_LAYOUT.field_size) as i64
}

/// The union field `atom` of `class`, with its slot (`AotFacts::class_union_layout`): None where the union names no such field.
fn union_field(facts: &AotFacts, class: u32, atom: Atom) -> Option<(usize, UnionField)> {
    let layout = facts.class_union_layout(class)?;
    layout.fields.iter().enumerate().find(|(_, (name, _))| *name == atom).map(|(slot, (_, field))| (slot, *field))
}

impl Lowering<'_, '_> {
    /// The receiver `owner` (None for `this`) as its class cell (`ObjectRecord::prove_fixed`). No test is made.
    fn fixed_cell(&mut self, state: &mut State, owner: Option<Register>) -> Option<ObjectRecord> {
        let bits = match owner {
            Some(register) => self.tagged(state, register),
            None => {
                let this = self.entry.as_ref()?.this;
                headers::this_word(&mut self.builder, this)
            }
        };
        let class_base = headers::class_table_base(&mut self.builder, self.payload);
        Some(ObjectRecord::prove_fixed(&mut self.builder, bits, class_base))
    }

    /// The fixed class and the union field `atom` of the receiver `owner` at unit PC `pc` (`tag_proofs::fixed_class`).
    fn fixed_field_of(&self, pc: usize, owner: Option<Register>, atom: Atom) -> Option<(usize, UnionField)> {
        let class = self.fixed_class(pc, owner)?;
        union_field(self.vm.facts?, class, atom)
    }

    /// The word the union slot at `address` holds for the access at unit PC `pc`, under the alias region of its field.
    fn slot_load(&mut self, pc: usize, lane: Lane, address: ir::Value) -> ir::Value {
        let property = regions::property_flags(self.field_region(pc));
        emit(&mut self.builder).raw_load(lane, property, address, 0)
    }

    /// The tagged word of the union slot at `address`, and its type, with `field`'s default applied to a `DELETED` slot: an
    /// absent field reads `undefined`; a prototype's default leaves through `exit`, where the VM reads it along the chain.
    fn tagged_slot(&mut self, pc: usize, address: ir::Value, field: UnionField, exit: ir::Block) -> (ir::Value, Type) {
        let bits = self.slot_load(pc, Lane::I64, address);
        let deleted = Value::DELETED.to_bits() as i64;
        match field.default {
            FieldDefault::Absent => {
                let absent = self.raw_compare_imm(Relation::Equal, bits, deleted);
                let undefined = self.raw_const(Lane::I64, Value::UNDEFINED.to_bits() as i64);
                let word = self.raw_select(absent, undefined, bits);
                (word, field.cell.ty.join(Type::language(quench_runtime::ValueType::Undefined)))
            }
            FieldDefault::Prototype(_) => {
                let present = self.raw_compare_imm(Relation::NotEqual, bits, deleted);
                emit_js_guard(&mut self.builder, present, exit);
                (bits, field.cell.ty)
            }
        }
    }

    /// `GetField` of the union field `atom` of a fixed receiver `owner` (None for `this`), defining the instruction's
    /// register. False, emitting nothing, where the receiver is no fixed class's or the union names no such field.
    pub(crate) fn fixed_field_read(&mut self, state: &mut State, pc: usize, instruction: WideInstruction,
        (owner, atom): (Option<Register>, Atom), exit: ir::Block) -> bool {
        let Some((slot, field)) = self.fixed_field_of(pc, owner, atom) else { return false };
        let Some(cell) = self.fixed_cell(state, owner) else { return false };
        let displacement = self.raw_const(Lane::I64, slot_displacement(slot));
        let address = cell.offset_address(&mut self.builder, displacement);
        let destination = instruction.result_register();
        // A Number slot holds the raw binary64 word, which no absent field's `DELETED` can be: its union field is present.
        if super::number_domains::is_double(field.cell.ty) {
            let number = self.slot_load(pc, Lane::F64, address);
            self.set_number(state, destination, number);
            return true;
        }
        let (word, ty) = self.tagged_slot(pc, address, field, exit);
        if ty == Type::INT32 {
            let int = self.raw_unary(Unary::Narrow(Lane::I32), word);
            self.define_int_view(state, destination, word, int);
        } else {
            self.define(state, destination, word, RegisterFact::definition(ty, Representation::Tagged));
        }
        true
    }

    /// `SetField` of the union field `atom` of a fixed receiver `owner` (None for `this`), written in place. The union's
    /// domain admits every store of the field (`AotFacts::slot_domain`), so a value of that type needs no admission and
    /// changes no class. False, emitting nothing, where the receiver is no fixed class's, the union names no such field,
    /// or the stored value's type is not the domain's.
    pub(crate) fn fixed_field_store(&mut self, state: &mut State, pc: usize, instruction: WideInstruction,
        owner: Option<Register>, exit: ir::Block) -> bool {
        let source = instruction.register_a();
        let atom = instruction.atom_index();
        let Some((slot, field)) = self.fixed_field_of(pc, owner, atom) else { return false };
        if !state.values[usize::from(source)].value.ty.is_subtype(field.cell.ty) { return false; }
        let Some(cell) = self.fixed_cell(state, owner) else { return false };
        let displacement = self.raw_const(Lane::I64, slot_displacement(slot));
        let address = cell.offset_address(&mut self.builder, displacement);
        let property = regions::property_flags(self.field_region(pc));
        if super::number_domains::is_double(field.cell.ty) {
            let number = self.number(state, source, exit);
            emit(&mut self.builder).raw_store(Lane::F64, property, (number, false), address, 0);
        } else {
            let value = self.tagged(state, source);
            emit(&mut self.builder).raw_store(Lane::I64, property, (value, false), address, 0);
        }
        state.proofs.write_field(atom);
        true
    }
}
