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
use quench_runtime::staged::machine::{Binary, Lane, MachineAccess, MachineScalar, Relation, Unary};
use quench_runtime::value::ValueLayout;
use quench_runtime::Value;

/// Union slot `slot` of the instances of a fixed class, as its displacement from the class cell's address.
fn slot_displacement(slot: usize) -> i64 {
    (NATIVE_CLASS_CELL_LAYOUT.fields_offset + slot * NATIVE_CLASS_CELL_LAYOUT.field_size) as i64
}

/// A union field of a fixed receiver, read or stored: its slot, its settled union field, and whether every instance holds it
/// from construction on (`AotFacts::field_always_stored`), so a read needs no hole test and no `FieldDefault`.
#[derive(Clone, Copy)]
struct FixedSlot {
    slot: usize,
    field: UnionField,
    stored: bool,
}

/// The word a fixed slot read yields: a raw Number (the binary64 bits, which are its tagged word too), or a tagged word of a type.
enum SlotWord {
    Number(ir::Value),
    Tagged(ir::Value, Type),
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
    fn fixed_field_of(&self, pc: usize, owner: Option<Register>, atom: Atom) -> Option<FixedSlot> {
        let class = self.fixed_class(pc, owner)?;
        let facts = self.vm.facts?;
        let (slot, field) = union_field(facts, class, atom)?;
        Some(FixedSlot { slot, field, stored: facts.field_always_stored(class, atom) })
    }

    /// The word the union slot at `address` holds for the access at unit PC `pc`, under the alias region of its field.
    fn slot_load(&mut self, pc: usize, lane: Lane, address: ir::Value) -> ir::Value {
        let property = regions::property_flags(self.field_region(pc));
        emit(&mut self.builder).raw_load(lane, property, address, 0)
    }

    /// The word of the union slot at `address`. A slot that `field_always_stored` proves holds its value from birth is read as it is.
    /// Otherwise a `DELETED` slot is a hole, which a read answers by the field's default: an absent field reads `undefined`;
    /// a prototype's default leaves through `exit`, where the VM reads it along the chain. A Number slot's hole is tested on
    /// its raw bits as well (no Number's binary64 bits are `DELETED`, which is a tag-space NaN no stored Number takes).
    fn slot_word(&mut self, pc: usize, address: ir::Value, fixed: FixedSlot, exit: ir::Block) -> SlotWord {
        let number = super::number_domains::is_double(fixed.field.cell.ty);
        if fixed.stored {
            return if number { SlotWord::Number(self.slot_load(pc, Lane::F64, address)) }
                else { SlotWord::Tagged(self.slot_load(pc, Lane::I64, address), fixed.field.cell.ty) };
        }
        let bits = self.slot_load(pc, Lane::I64, address);
        let deleted = Value::DELETED.to_bits() as i64;
        match fixed.field.default {
            FieldDefault::Absent => {
                let absent = self.raw_compare_imm(Relation::Equal, bits, deleted);
                let undefined = self.raw_const(Lane::I64, Value::UNDEFINED.to_bits() as i64);
                let word = self.raw_select(absent, undefined, bits);
                SlotWord::Tagged(word, fixed.field.cell.ty.join(Type::language(quench_runtime::ValueType::Undefined)))
            }
            FieldDefault::Prototype(_) => {
                let present = self.raw_compare_imm(Relation::NotEqual, bits, deleted);
                emit_js_guard(&mut self.builder, present, exit);
                if number { SlotWord::Number(self.slot_load(pc, Lane::F64, address)) }
                    else { SlotWord::Tagged(bits, fixed.field.cell.ty) }
            }
        }
    }

    /// `GetField` of the union field `atom` of a fixed receiver `owner` (None for `this`), defining the instruction's
    /// register. False, emitting nothing, where the receiver is no fixed class's or the union names no such field.
    pub(crate) fn fixed_field_read(&mut self, state: &mut State, pc: usize, instruction: WideInstruction,
        (owner, atom): (Option<Register>, Atom), exit: ir::Block) -> bool {
        let Some(fixed) = self.fixed_field_of(pc, owner, atom) else { return false };
        let Some(cell) = self.fixed_cell(state, owner) else { return false };
        let displacement = self.raw_const(Lane::I64, slot_displacement(fixed.slot));
        let address = cell.offset_address(&mut self.builder, displacement);
        let destination = instruction.result_register();
        match self.slot_word(pc, address, fixed, exit) {
            SlotWord::Number(number) => self.set_number(state, destination, number),
            SlotWord::Tagged(word, ty) if ty == Type::INT32 => {
                let int = self.raw_unary(Unary::Narrow(Lane::I32), word);
                self.define_int_view(state, destination, word, int);
            }
            SlotWord::Tagged(word, ty) => self.define(state, destination, word, RegisterFact::definition(ty, Representation::Tagged)),
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
        let Some(fixed) = self.fixed_field_of(pc, owner, atom) else { return false };
        let field = fixed.field;
        if !state.values[usize::from(source)].value.ty.is_subtype(field.cell.ty) { return false; }
        let Some(cell) = self.fixed_cell(state, owner) else { return false };
        let displacement = self.raw_const(Lane::I64, slot_displacement(fixed.slot));
        let address = cell.offset_address(&mut self.builder, displacement);
        let property = regions::property_flags(self.field_region(pc));
        if super::number_domains::is_double(field.cell.ty) {
            // A raw Number is no heap reference: its store needs no barrier.
            let number = self.number(state, source, exit);
            emit(&mut self.builder).raw_store(Lane::F64, property, (number, false), address, 0);
        } else {
            let value = self.tagged(state, source);
            self.fixed_store_barrier(state, owner, source, value, exit);
            emit(&mut self.builder).raw_store(Lane::I64, property, (value, false), address, 0);
        }
        state.proofs.write_field(atom);
        true
    }

    /// The projected slot of a loop's store at `store` into the fixed class cell of `owner` (`LoopStore`): the union slot's
    /// address in the cell, for the domain the store's value takes (a Number the raw doubles, else an int32). Neither needs a
    /// barrier. None where the receiver is no fixed class's, the union names no such field, or the slot does not admit the domain.
    pub(crate) fn fixed_loop_store(&mut self, state: &mut State, store: usize, owner: Register) -> Option<LoopStore> {
        let instruction = self.unit.instructions[store];
        let fixed = self.fixed_field_of(store, Some(owner), instruction.atom_index())?;
        let domain = if super::number_domains::is_double(fixed.field.cell.ty) { Type::NUMBER } else { Type::INT32 };
        if !domain.is_subtype(fixed.field.cell.ty) { return None; }
        let cell = self.fixed_cell(state, Some(owner))?;
        let header = cell.header(&mut self.builder, None);
        let displacement = self.raw_const(Lane::I64, slot_displacement(fixed.slot));
        let address = cell.offset_address(&mut self.builder, displacement);
        Some(LoopStore { header, address, packed_range: None, domain })
    }

    /// The store barrier (4.7) of the tagged `value` (of register `source`) into the fixed class cell of `owner`.
    /// The class space logs every heap reference stored into a cell (`class_cell_store`, which tests no mark), and the lane
    /// has no mark list for the class space, so a heap reference leaves to the VM's store at `exit`. An immediate needs no
    /// barrier, and neither does a store into a holder this unit allocated with no safepoint since (`holder_fresh`).
    fn fixed_store_barrier(&mut self, state: &mut State, owner: Option<Register>, source: Register, value: ir::Value, exit: ir::Block) {
        if state.values[usize::from(source)].value.ty.is_subtype(Type::IMMEDIATE) || self.holder_fresh(state, owner) { return; }
        let tag = self.raw_binary_imm(Binary::And, value, ValueLayout::TAG_MASK as i64);
        let heap = self.raw_compare_imm(Relation::Equal, tag, ValueLayout::HEAP_TAG as i64);
        let plain = self.raw_compare_imm(Relation::Equal, heap, 0);
        emit_js_guard(&mut self.builder, plain, exit);
    }
}
