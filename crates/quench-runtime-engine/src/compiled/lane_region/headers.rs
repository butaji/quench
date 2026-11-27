//! A field access's proofs of its owner's object: the record header proved once and kept in
//! `State` until an effect that may change the object retires it, and the shape tests later
//! reads fold against it. The lowering and the frame's property rows share these.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use super::super::object_slots::emit;
use quench_runtime::staged::machine::{Lane, MachineScalar};
use quench_runtime::staged::ShapeFact;

/// `register`'s tagged word, resolved to its value: the key its proofs are kept by, so a copy or an
/// inlined frame's `this` holding the same word shares them.
pub(crate) fn register_word(builder: &mut FunctionBuilder<'_>, registers: &mut Registers, state: &mut State,
    register: Register) -> ir::Value {
    let bits = frame::register_tagged(builder, registers, state, register);
    builder.func.dfg.resolve_aliases(bits)
}

/// The fixed `this`'s word, resolved (the key of the receiver's proofs).
pub(crate) fn this_word(builder: &mut FunctionBuilder<'_>, this: Variable) -> ir::Value {
    let bits = builder.use_var(this);
    builder.func.dfg.resolve_aliases(bits)
}

/// `owner`'s word: a register's, or fixed `this`'s where the entry has a `this` variable.
pub(crate) fn owner_word(builder: &mut FunctionBuilder<'_>, registers: &mut Registers, state: &mut State,
    this: Option<Variable>, owner: Option<Register>) -> Option<ir::Value> {
    match owner {
        Some(register) => Some(register_word(builder, registers, state, register)),
        None => Some(this_word(builder, this?)),
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
fn refine_owner(state: &mut State, owner: Option<Register>, fact: Option<VerifiedFact>) {
    let Some(register) = owner else { return };
    let index = usize::from(register);
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
    let fact = owner.map(|register| state.values[usize::from(register)].value);
    let bits = match owner {
        Some(register) => register_word(builder, registers, state, register),
        None => this_word(builder, entry.this),
    };
    if let Some(index) = state.proofs.headers.iter().position(|(word, ..)| *word == bits) {
        let (_, header, readable) = state.proofs.headers[index];
        if !read && readable {
            let object = quench_runtime::staged::value::is_direct_object(&mut super::super::object_slots::emit(builder), bits);
            emit_js_guard(builder, object, exit);
            state.proofs.headers[index].2 = false;
            refine_owner(state, owner, fact);
            for record in state.proofs.records.iter_mut().filter(|(word, ..)| *word == bits) { record.2 = false; }
        }
        return header;
    }
    let index = owner.map_or(state.values.len(), usize::from);
    let mut readable = false;
    let header = if let Some(record) = entry.records[index] {
        record.header(builder, fact)
    } else {
        // The record this word names, proved before (`LocalProofs::records`): by any register that holds it,
        // and before a call that retired the header's shape. The shape is read again through it, with no
        // family test or record lookup; a store of a read's proof still tests the family once.
        let kept = state.proofs.records.iter().find(|(word, ..)| *word == bits).copied();
        let header = if let Some((_, record, was_readable)) = kept.filter(|(.., was_readable)| !*was_readable || read) {
            readable = was_readable;
            record.header(builder, fact)
        } else if read && !fact.is_some_and(|fact| fact.ty.is_direct_object_family()) {
            readable = true;
            let records = object_table_base(builder, entry, payload);
            let functions = emit_js_record_base(builder, payload, std::mem::offset_of!(JsStepDispatch, head.function_record_base));
            ObjectHeader::prove_readable(builder, bits, records, functions, exit)
        } else {
            let records = object_table(builder, entry, payload);
            ObjectHeader::prove(builder, bits, records, fact, exit)
        };
        if kept.is_none_or(|(.., was_readable)| was_readable && !readable) {
            state.proofs.records.retain(|(word, ..)| *word != bits);
            state.proofs.records.push((bits, header.record, readable));
        }
        header
    };
    state.proofs.headers.push((bits, header, readable));
    if !readable { refine_owner(state, owner, fact); }
    header
}

/// The staged shape guard (`ic::guard_shape`) on the object `word` names, whose header `header` read its
/// shape word: the shape is `expected`, else leave at `exit`. The header then holds the
/// shape as that constant, under the guard's fact, which folds every later test of it.
pub(crate) fn prove_header_shape(builder: &mut FunctionBuilder<'_>, state: &mut State, word: ir::Value,
    header: ObjectHeader, expected: u32, exit: ir::Block) {
    let at = quench_runtime::staged::ic::Reached { shape: Some(header.shape), ..quench_runtime::staged::ic::Reached::bare(header.address) };
    let shaped = quench_runtime::staged::ic::guard_shape(&mut emit(builder), &at, expected, exit).expect("an emitted guard continues");
    note_header_shape(builder, state, word, (expected, shaped.fact()));
}

/// The header of the object `word` names holds the shape `expected`, as a constant, under `fact`.
pub(crate) fn note_header_shape(builder: &mut FunctionBuilder<'_>, state: &mut State, word: ir::Value,
    (expected, fact): (u32, ShapeFact)) {
    if let Some((_, proof, _)) = state.proofs.headers.iter_mut().find(|(known, ..)| *known == word) {
        proof.shape = emit(builder).raw_const(Lane::I32, i64::from(expected));
        proof.shape_fact = Some(fact);
    }
}

/// The exact shape the header of the object `word` names holds as a constant, and the fact that proved
/// it: a fact of the register's type, or an earlier guard.
pub(crate) fn header_shape(builder: &FunctionBuilder<'_>, state: &State, word: ir::Value) -> Option<(u32, ShapeFact)> {
    let (_, header, _) = *state.proofs.headers.iter().find(|(known, ..)| *known == word)?;
    let fact = header.shape_fact?;
    let ir::ValueDef::Result(constant, _) = builder.func.dfg.value_def(header.shape) else { return None };
    let ir::InstructionData::UnaryImm { opcode: ir::Opcode::Iconst, imm } = builder.func.dfg.insts[constant] else { return None };
    u32::try_from(imm.bits()).ok().map(|shape| (shape, fact))
}
