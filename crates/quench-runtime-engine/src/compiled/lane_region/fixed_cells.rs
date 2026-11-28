//! Named reads and stores of a fixed class's receiver (`AotFacts::class_fixed_cell`). Every instance of the class is a class
//! cell of its union layout from birth, so the receiver's cell is its direct reference's granule (`ObjectRecord::FixedCell`),
//! and union slot `k` is the cell's field at `fields_offset + field_size * k`. No record presence, departed, class or shape
//! test is made: the class is the fact. A slot of an absent field holds `DELETED`, which a read answers by the union's
//! `FieldDefault`. A store writes its slot in place, since the union's domain admits every store of the field; a tagged store
//! first passes the class space's barrier (`Stage::store_gate`), keyed by the cell's granule.

use super::*;
use super::super::object_slots::{emit, Emit, ObjectRecord};
use quench_runtime::bytecode::{Atom, Register};
use quench_runtime::compile::aot_facts::{AotFacts, FieldDefault, UnionField};
use quench_runtime::staged::layout_offsets::NATIVE_CLASS_CELL_LAYOUT;
use quench_runtime::staged::machine::{Lane, MachineAccess, MachineScalar, Relation, Unary};
use quench_runtime::staged::{Receiver, StagedRecordSpace};
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

/// The class cell the entry proved of the receiver `owner` (None for `this`), where `invariant_field_records` proved its
/// owner a fixed class's cell. The entry's record of `this` is its last (the registers' records come first).
pub(crate) fn entry_fixed_record(entry: &Entry, owner: Option<Register>) -> Option<ObjectRecord> {
    let index = match owner {
        Some(register) => usize::from(register),
        None => entry.records.len().checked_sub(1)?,
    };
    match entry.records.get(index)? {
        Some(record @ ObjectRecord::FixedCell { .. }) => Some(*record),
        _ => None,
    }
}

impl Lowering<'_, '_> {
    /// Whether the entry proved the receiver `owner` (None for `this`) a fixed class's cell (`entry_fixed_record`). Its record
    /// then has no shape, storage form or presence to test, so no shape guard is made of it (`hoist_receivers`, `conform_object`).
    pub(crate) fn entry_fixed(&self, owner: Option<Register>) -> bool {
        self.entry.as_ref().is_some_and(|entry| entry_fixed_record(entry, owner).is_some())
    }

    /// The fixed class `receiver` holds just before unit PC `pc` (`fixed_class`), for a register or `this`.
    pub(crate) fn fixed_receiver_class(&self, pc: usize, receiver: Receiver) -> Option<u32> {
        match receiver {
            Receiver::Register(register) => self.fixed_class(pc, Some(register)),
            Receiver::This { path: None } => self.fixed_class(pc, None),
            _ => None,
        }
    }

    /// The receiver `owner` (None for `this`) as its class cell: the entry's cell where it proved one (`entry_fixed_record`),
    /// else the cell of the owner's word (`ObjectRecord::prove_fixed`). No test is made.
    fn fixed_cell(&mut self, state: &mut State, owner: Option<Register>) -> Option<ObjectRecord> {
        if let Some(record) = self.entry.as_ref().and_then(|entry| entry_fixed_record(entry, owner)) {
            return Some(record);
        }
        let bits = match owner {
            Some(register) => self.tagged(state, register),
            None => {
                let this = self.entry.as_ref()?.this;
                headers::this_word(&mut self.builder, this)
            }
        };
        Some(self.fixed_record(bits))
    }

    /// The class cell of the direct object reference `bits` (`ObjectRecord::prove_fixed`), with no test.
    pub(crate) fn fixed_record(&mut self, bits: ir::Value) -> ObjectRecord {
        let class_base = headers::class_table_base(&mut self.builder, self.payload);
        ObjectRecord::prove_fixed(&mut self.builder, bits, class_base)
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

    /// The word of union field `atom` of the fixed receiver `owner` (None for `this`) at unit PC `pc`, read on a loop's
    /// entering edge (`loop_hoists`) from the receiver's cell: the slot's tagged word and its type, which an absent field
    /// joins with `undefined`. A Number slot's raw bits are its tagged word. None where the receiver is no fixed class's.
    pub(crate) fn fixed_hoisted_word(&mut self, state: &mut State, pc: usize, owner: Option<Register>, atom: Atom,
        exit: ir::Block) -> Option<(ir::Value, Type)> {
        let fixed = self.fixed_field_of(pc, owner, atom)?;
        let cell = self.fixed_cell(state, owner)?;
        let displacement = self.raw_const(Lane::I64, slot_displacement(fixed.slot));
        let address = cell.offset_address(&mut self.builder, displacement);
        Some(match self.slot_word(pc, address, fixed, exit) {
            SlotWord::Number(number) => (self.raw_unary(Unary::Bitcast(Lane::I64), number), fixed.field.cell.ty),
            SlotWord::Tagged(word, ty) => (word, ty),
        })
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
            self.fixed_store_barrier(state, (owner, cell), source, value, exit);
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

    /// The store barrier (4.7) of the tagged `value` (of register `source`) into the fixed class cell `cell` of `owner`: the
    /// class space's gate (`Stage::store_gate`, `StagedRecordSpace::Class`) keyed by the cell's granule, which leaves at `exit`
    /// where it logs the store. An immediate needs no barrier. A store into a holder this unit allocated with no safepoint
    /// since (`holder_fresh`) logs nothing, so only the observation gate is asked (4.7 (b)).
    fn fixed_store_barrier(&mut self, state: &mut State, (owner, cell): (Option<Register>, ObjectRecord), source: Register,
        value: ir::Value, exit: ir::Block) {
        if state.values[usize::from(source)].value.ty.is_subtype(Type::IMMEDIATE) { return; }
        let Some(granule) = cell.granule() else { return };
        let fresh = self.holder_fresh(state, owner);
        let payload = self.payload;
        let mut stage = Emit { builder: &mut self.builder, property_values: None, gate: Some(payload), payload: Some(payload),
            facts: Default::default(), fresh };
        let open = stage.store_gate(StagedRecordSpace::Class, granule, value);
        stage.guard(open, exit).expect("an Emit guard always continues");
    }
}
