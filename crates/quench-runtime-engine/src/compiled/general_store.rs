//! A field store's in-place case in native code: an own data slot of its site's
//! cached exact shape in plain arena storage, for a value of the class the site's last
//! helper store proved its slot representation already holds (`record_store_class`). Slot
//! representations only widen (`ValueArena::note_slot_value`), and rewriting or invalidating
//! the entry clears the class, so the proof holds until then. Any other store takes the
//! helper (`field_set_in_lane`), which records the proof.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::{emit_js_payload_word, JsStepDispatch, Vm};
use quench_runtime::bytecode::Atom;
use quench_runtime::typed_ir::SlotType;
use crate::compiled::regions;
use quench_runtime::Value;
use cranelift_codegen::ir::{self, types};
use super::object_slots::emit;
use quench_runtime::staged::machine::{Binary, Lane, MachineAccess, MachineFlow, MachineScalar, Relation};
use cranelift_frontend::FunctionBuilder;

/// The proof's place above the slot in its entry's slot word, whose low half the slot fills.
const STORE_CLASS_SHIFT: u32 = u16::BITS;

/// The store class of `value` that a proof names, or `None`: `SlotType::of(value)` filtered to
/// the four classes a proof records (an int32, a direct object, null, undefined). Those are the
/// value classes `ValueArena::note_slot_value` joins alike for every value of it, so one store
/// that widened nothing proves every later one. Objects join their own shape as the slot's
/// target, so theirs needs a settled target too.
fn store_class(value: Value) -> Option<SlotType> {
    match SlotType::of(value) {
        class @ (SlotType::Int32 | SlotType::DirectObject | SlotType::Null | SlotType::Undefined) => Some(class),
        _ => None,
    }
}

/// Records on `site`'s entry the class of `value`, which a native-preserving helper store just
/// wrote through it without widening its slot's representation, where the class proves
/// every later store of it (`store_class`).
pub(crate) fn record_store_class(vm: &mut Vm, site: u16, value: Value) {
    let Some(class) = store_class(value) else { return };
    // A native store writes the value's tagged word, which a slot of raw doubles does not hold.
    let entry = vm.field_cache_entry(site);
    if vm.heap_reads().slot_holds_raw_doubles(entry.receiver, usize::from(entry.slot)) { return; }
    let index = vm.field_cache_index(site);
    if !vm.field_cache_records(index) {
        return;
    }
    let word = &mut vm.field_caches.stored_mut(index).slot;
    let recorded = (*word & !(quench_runtime::vm::object::FIELD_BYTE << STORE_CLASS_SHIFT)) | ((class as u32) << STORE_CLASS_SHIFT);
    if *word != recorded {
        *word = recorded;
        vm.note_field_cache_write(index);
    }
}

/// Whether no method cache names `atom` (`Vm::method_caches_may_name`), from the payload's
/// atom bits; an atom past them is unnamed.
pub(crate) fn emit_atom_unnamed(builder: &mut FunctionBuilder<'_>, payload: ir::Value, atom: Atom) -> ir::Value {
    let word_index = atom / u64::BITS;
    let words = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, head.method_cache_atoms));
    let words_len = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, head.method_cache_atom_words));
    let covered = emit(builder).raw_compare_imm(Relation::UnsignedGreater, words_len, i64::from(word_index));
    let (present, join) = (builder.create_block(), builder.create_block());
    let free = builder.append_block_param(join, types::I8);
    let one = emit(builder).raw_const(Lane::I8, 1);
    emit(builder).branch_if(covered, (present, &[]), (join, &[one]));
    builder.switch_to_block(present);
    let word = emit(builder).raw_load(Lane::I64, regions::heap_meta(), words, i32::try_from(word_index as usize * size_of::<u64>()).expect("atom word offset fits i32"));
    let named = emit(builder).raw_binary_imm(Binary::And, word, (1_u64 << (atom % u64::BITS)) as i64);
    let unnamed = emit(builder).raw_compare_imm(Relation::Equal, named, 0);
    emit(builder).goto(join, &[unnamed]);
    builder.switch_to_block(join);
    free
}
