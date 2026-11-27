//! A field access's proofs of its owner's object: the record header proved once and kept in
//! `State` until an effect that may change the object retires it, and the shape tests later
//! reads fold against it. The lowering and the frame's property rows share these.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use super::super::object_slots::emit;
use quench_runtime::staged::machine::{Lane, MachineScalar};
use quench_runtime::staged::ShapeFact;

/// `owner`'s value: a register's, or the fixed `this`.
pub(crate) fn owner_bits(builder: &mut FunctionBuilder<'_>, registers: &mut Registers, state: &mut State, entry: &Entry,
    owner: Option<Register>) -> ir::Value {
    match owner {
        Some(register) => frame::register_tagged(builder, registers, state, register),
        None => builder.use_var(entry.this),
    }
}

/// The record table of the run: the entry's checked reservation, else the dispatch payload's.
pub(crate) fn object_table(builder: &mut FunctionBuilder<'_>, entry: &Entry, payload: ir::Value) -> ObjectTable {
    match entry.object_table {
        Some(table) => ObjectTable::Checked(table),
        None => ObjectTable::Unchecked(emit_js_record_base(builder, payload, std::mem::offset_of!(JsStepDispatch, head.object_record_base))),
    }
}

/// The object record reservation's base (`object_table`).
pub(crate) fn object_table_base(builder: &mut FunctionBuilder<'_>, entry: &Entry, payload: ir::Value) -> ir::Value {
    match object_table(builder, entry, payload) {
        ObjectTable::Checked(table) => table.base(),
        ObjectTable::Unchecked(base) => base,
    }
}

/// A register whose object an object proof names holds a direct object from then on.
fn refine_owner(state: &mut State, index: usize, fact: Option<VerifiedFact>) {
    // An unboxed register has no tagged view to keep: an object proof of it is a
    // contradiction its reads leave at, so the register keeps its own fact.
    if let Some(fact) = fact.filter(|_| state.values[index].tagged.current()) {
        state.values[index] = RegisterFact { tagged: state.values[index].tagged,
            ..RegisterFact::definition(
                fact.ty.direct_object_part().unwrap_or(Type::DIRECT_OBJECT_FAMILY),
                Representation::Tagged) };
    }
}

/// A dominating proof of `owner`'s object (a register's, or `this`'s), for its lifetime until
/// an effect that may change it retires the proof (`State::headers`), derived from the
/// entry's record certificate where it has one, otherwise proved here: for a `read` of a
/// value not known to be a direct object, an object's or a function's record
/// (`ObjectHeader::prove_readable`); for a store, an object's only. A store of a read's proof
/// tests the family once, after which the proof is the object's.
pub(crate) fn object_header(builder: &mut FunctionBuilder<'_>, registers: &mut Registers, state: &mut State,
    (entry, payload): (&Entry, ir::Value), owner: Option<Register>, read: bool, exit: ir::Block) -> ObjectHeader {
    let index = state.header_index(owner);
    let fact = owner.map(|register| state.values[usize::from(register)].value);
    if let Some(header) = state.proofs.headers[index] {
        if !read && state.proofs.readable.contains(index) {
            let bits = owner_bits(builder, registers, state, entry, owner);
            let object = quench_runtime::staged::value::is_direct_object(&mut super::super::object_slots::emit(builder), bits);
            emit_js_guard(builder, object, exit);
            state.proofs.readable.remove(index);
            refine_owner(state, index, fact);
            if let Some(slot) = state.proofs.records.get_mut(index) { *slot = Some(header.record); }
        }
        return header;
    }
    let mut readable = false;
    // The record the owner's value names, proved before a call that retired the header's shape
    // (`LocalProofs::records`): the shape is read again through it, with no family test or lookup.
    let header = if let Some(record) = entry.records[index].or(state.proofs.records.get(index).copied().flatten()) {
        record.header(builder, fact)
    } else {
        let bits = owner_bits(builder, registers, state, entry, owner);
        if read && !fact.is_some_and(|fact| fact.ty.is_direct_object_family()) {
            readable = true;
            let records = object_table_base(builder, entry, payload);
            let functions = emit_js_record_base(builder, payload, std::mem::offset_of!(JsStepDispatch, head.function_record_base));
            ObjectHeader::prove_readable(builder, bits, records, functions, exit)
        } else {
            let records = object_table(builder, entry, payload);
            ObjectHeader::prove(builder, bits, records, fact, exit)
        }
    };
    state.proofs.headers[index] = Some(header);
    state.proofs.set_readable(index, readable);
    if !readable {
        refine_owner(state, index, fact);
        if let Some(slot) = state.proofs.records.get_mut(index) { *slot = Some(header.record); }
    }
    header
}

/// The staged shape guard (`ic::guard_shape`) on `owner`'s object, whose header `header` read its
/// shape word: the shape is `expected`, else leave at `exit`. The owner's header then holds the
/// shape as that constant, under the guard's fact, which folds every later test of it.
pub(crate) fn prove_header_shape(builder: &mut FunctionBuilder<'_>, state: &mut State, owner: Option<Register>,
    header: ObjectHeader, expected: u32, exit: ir::Block) {
    let at = quench_runtime::staged::ic::Reached { shape: Some(header.shape), ..quench_runtime::staged::ic::Reached::bare(header.address) };
    let shaped = quench_runtime::staged::ic::guard_shape(&mut emit(builder), &at, expected, exit).expect("an emitted guard continues");
    note_header_shape(builder, state, owner, (expected, shaped.fact()));
}

/// The owner's header holds the shape `expected`, as a constant, under `fact`.
pub(crate) fn note_header_shape(builder: &mut FunctionBuilder<'_>, state: &mut State, owner: Option<Register>,
    (expected, fact): (u32, ShapeFact)) {
    let index = state.header_index(owner);
    if let Some(proof) = &mut state.proofs.headers[index] {
        proof.shape = emit(builder).raw_const(Lane::I32, i64::from(expected));
        proof.shape_fact = Some(fact);
    }
}

/// The exact shape `owner`'s header holds as a constant, and the fact that proved it: a fact of
/// the register's type, or an earlier guard.
pub(crate) fn header_shape(builder: &FunctionBuilder<'_>, state: &State, owner: Option<Register>) -> Option<(u32, ShapeFact)> {
    let header = state.proofs.headers[state.header_index(owner)]?;
    let fact = header.shape_fact?;
    let ir::ValueDef::Result(constant, _) = builder.func.dfg.value_def(header.shape) else { return None };
    let ir::InstructionData::UnaryImm { opcode: ir::Opcode::Iconst, imm } = builder.func.dfg.insts[constant] else { return None };
    u32::try_from(imm.bits()).ok().map(|shape| (shape, fact))
}
