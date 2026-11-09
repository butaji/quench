//! A field store's in-place case in general dispatch code: an own data slot of its site's
//! cached exact shape in plain arena storage, for a value of the class the site's last
//! helper store proved its slot representation already holds (`record_store_class`). Slot
//! representations only widen (`ValueArena::note_slot_value`), and rewriting or invalidating
//! the entry clears the class, so the proof holds until then. Any other store takes the
//! helper (`lane_field_set`), which records the proof.
use super::{emit_js_cached_field_slot, emit_js_guard, emit_js_payload_word, emit_lane_helper_call, emit_unlogged_property_store_guard, Host,
    JsFieldReceiver, JsStepDispatch, Vm};
use super::super::NATIVE_FIELD_CACHE_LAYOUT as CACHE;
use crate::bytecode::Atom;
use crate::value::ValueLayout;
use crate::Value;
use cranelift_codegen::ir::{self, condcodes::IntCC, types, InstBuilder, MemFlagsData};
use cranelift_frontend::FunctionBuilder;

/// The value classes a store proof names: each one `ValueArena::note_slot_value` joins alike
/// for every value of it, so one store that widened nothing proves every later one. Objects
/// join their own shape as the slot's target, so theirs needs a settled target too.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
enum StoreClass { Int32 = 1, DirectObject, Null, Undefined }

impl StoreClass {
    const ALL: [Self; 4] = [Self::Int32, Self::DirectObject, Self::Null, Self::Undefined];
    /// The proof's place above the slot in its entry's slot word, whose low half the slot fills.
    const SHIFT: u32 = u16::BITS;
    /// The entry's slot word among its stored words.
    const WORD: usize = CACHE.slot_offset / size_of::<u32>();

    fn of(value: Value) -> Option<Self> {
        if value.as_int().is_some() { Some(Self::Int32) }
        else if value.direct_object_index().is_some() { Some(Self::DirectObject) }
        else if value.is_null() { Some(Self::Null) }
        else if value.is_undefined() { Some(Self::Undefined) }
        else { None }
    }

    /// Whether `bits` is a value of this class, as `of` decides.
    fn emit_holds(self, builder: &mut FunctionBuilder<'_>, bits: ir::Value) -> ir::Value {
        let (mask, expected) = match self {
            Self::Int32 => (ValueLayout::TAG_MASK, ValueLayout::INT_TAG),
            Self::DirectObject => (ValueLayout::HEAP_FAMILY_MASK, ValueLayout::DIRECT_OBJECT_TAG),
            Self::Null => (u64::MAX, Value::NULL.to_bits()),
            Self::Undefined => (u64::MAX, Value::UNDEFINED.to_bits()),
        };
        let masked = builder.ins().band_imm_u(bits, mask as i64);
        builder.ins().icmp_imm_u(IntCC::Equal, masked, expected as i64)
    }
}

/// Records on `site`'s entry the class of `value`, which a native-preserving helper store just
/// wrote through it without widening its slot's representation, where the class proves
/// every later store of it (`StoreClass`).
pub(super) fn record_store_class<H: Host>(vm: &mut Vm<H>, site: u16, value: Value) {
    let Some(class) = StoreClass::of(value) else { return };
    let index = vm.field_cache_index(site);
    if !vm.field_cache_records(index) {
        return;
    }
    let word = &mut vm.field_caches.stored_mut(index)[StoreClass::WORD];
    let recorded = (*word & u32::from(u16::MAX)) | ((class as u32) << StoreClass::SHIFT);
    if *word != recorded {
        *word = recorded;
        vm.note_field_cache_write(index);
    }
}

/// The in-place store of `value` into `receiver`'s field `atom` at `site` (the module's case),
/// continuing at `done`; any other store continues at `helper`. Packed inline storage takes
/// the value's narrow form (`Value::to_narrow`), a value without one the helper.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_store(builder: &mut FunctionBuilder<'_>, (arguments, payload): (ir::Value, ir::Value),
    receiver: ir::Value, atom: Atom, site: u16, value: ir::Value, done: ir::Block, helper: ir::Block) {
    let admitted = emit_store_admitted(builder, payload, atom);
    let probe = builder.create_block();
    builder.ins().brif(admitted, probe, &[], helper, &[]);
    builder.switch_to_block(probe);
    let entries = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, field_entries));
    let entry = usize::from(site) * CACHE.entry_size + CACHE.slot_offset;
    let word = builder.ins().load(types::I32, MemFlagsData::trusted(), entries,
        i32::try_from(entry).expect("field entry offset fits i32"));
    let recorded = builder.ins().ushr_imm_u(word, i64::from(StoreClass::SHIFT));
    let classes = StoreClass::ALL.map(|class| (class, class.emit_holds(builder, value)));
    let mut proved = builder.ins().iconst(types::I8, 0);
    for (class, holds) in classes {
        let named = builder.ins().icmp_imm_u(IntCC::Equal, recorded, i64::from(class as u32));
        let both = builder.ins().band(named, holds);
        proved = builder.ins().bor(proved, both);
    }
    emit_js_guard(builder, proved, helper);
    let packed = builder.create_block();
    let packed_address = builder.append_block_param(packed, types::I64);
    let (address, record) = emit_js_cached_field_slot(builder, arguments, payload, JsFieldReceiver::Value(receiver),
        site, false, helper, None, None, Some(packed));
    emit_unlogged_property_store_guard(builder, payload, record, value, crate::typed_ir::Type::ANY, helper);
    builder.ins().store(MemFlagsData::trusted(), value, address, 0);
    builder.ins().jump(done, &[]);
    builder.switch_to_block(packed);
    let narrow = emit_narrow(builder, value, classes, helper);
    emit_unlogged_property_store_guard(builder, payload, record, value, crate::typed_ir::Type::ANY, helper);
    builder.ins().store(MemFlagsData::trusted(), narrow, packed_address, 0);
    builder.ins().jump(done, &[]);
}

/// The narrow form (`Value::to_narrow`) of `value`, whose class `classes` decides; `helper` for
/// an integer or object index too wide for one.
fn emit_narrow(builder: &mut FunctionBuilder<'_>, value: ir::Value, classes: [(StoreClass, ir::Value); 4],
    helper: ir::Block) -> ir::Value {
    use crate::value::{DIRECT_OBJECT_NARROW_CODE, DIRECT_OBJECT_UPPER, NARROW_CODE_SHIFT, NARROW_INDEX_BITS,
        NARROW_INDEX_SHIFT, NARROW_INTEGER_BITS, NARROW_INTEGER_FLAG};
    let constant = |builder: &mut FunctionBuilder<'_>, value: Value| {
        let narrow = value.to_narrow().expect("a nullish constant has a narrow form");
        builder.ins().iconst(types::I32, i64::from(narrow))
    };
    let lower = builder.ins().ireduce(types::I32, value);
    // An integer within the narrow range: its bias by half the range is below the range.
    let (range, half) = (1_i64 << NARROW_INTEGER_BITS, 1_i64 << (NARROW_INTEGER_BITS - 1));
    let biased = builder.ins().iadd_imm_s(lower, half);
    let integer_fits = builder.ins().icmp_imm_u(IntCC::UnsignedLessThan, biased, range);
    let shifted = builder.ins().ishl_imm_u(lower, i64::from(NARROW_CODE_SHIFT));
    let integer = builder.ins().bor_imm_u(shifted, i64::from(NARROW_INTEGER_FLAG));
    // An object whose handle word is exact and whose record index fits the narrow index.
    let upper = builder.ins().band_imm_u(value, !i64::from(u32::MAX));
    let handle = builder.ins().icmp_imm_u(IntCC::Equal, upper, DIRECT_OBJECT_UPPER as i64);
    let wide = builder.ins().ushr_imm_u(lower, i64::from(NARROW_INDEX_BITS));
    let index_fits = builder.ins().icmp_imm_u(IntCC::Equal, wide, 0);
    let object_fits = builder.ins().band(handle, index_fits);
    let shifted = builder.ins().ishl_imm_u(lower, i64::from(NARROW_INDEX_SHIFT));
    let object = builder.ins().bor_imm_u(shifted, i64::from(DIRECT_OBJECT_NARROW_CODE << NARROW_CODE_SHIFT));
    let [(_, is_integer), (_, is_object), (_, is_null), _] = classes;
    let (null, undefined) = (constant(builder, Value::NULL), constant(builder, Value::UNDEFINED));
    let nullish = builder.ins().select(is_null, null, undefined);
    let reference = builder.ins().select(is_object, object, nullish);
    let narrow = builder.ins().select(is_integer, integer, reference);
    let one = builder.ins().iconst(types::I8, 1);
    let reference_fits = builder.ins().select(is_object, object_fits, one);
    let fits = builder.ins().select(is_integer, integer_fits, reference_fits);
    emit_js_guard(builder, fits, helper);
    narrow
}

/// The helper's store of `value` into `receiver`'s field `atom` at `site`, or its add by the
/// site's cached transition (`general_field_store`), continuing at `done`; a store neither
/// takes continues at `fallback`.
pub(super) fn emit_helper_store(builder: &mut FunctionBuilder<'_>, payload: ir::Value,
    (receiver, atom, site, value): (ir::Value, Atom, u16, ir::Value), (done, fallback): (ir::Block, ir::Block)) {
    let write = builder.ins().iconst(types::I64, (i64::from(atom) << u16::BITS) | i64::from(site));
    let helper = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, field_store));
    let stored = emit_lane_helper_call(builder, payload, helper, &[payload, receiver, write, value]);
    let stored = builder.ins().icmp_imm_u(IntCC::NotEqual, stored, 0);
    emit_js_guard(builder, stored, fallback);
    builder.ins().jump(done, &[]);
}

/// Whether a cached store of `atom` may run in place (`Vm::primary_own_field_store_hit`): the
/// VM and program run specialized and no method cache names `atom`, read from the payload.
pub(super) fn emit_store_admitted(builder: &mut FunctionBuilder<'_>, payload: ir::Value, atom: Atom) -> ir::Value {
    let specialized = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, stores_specialized));
    let specialized = builder.ins().icmp_imm_u(IntCC::NotEqual, specialized, 0);
    let unnamed = emit_atom_unnamed(builder, payload, atom);
    builder.ins().band(specialized, unnamed)
}

/// Whether no method cache names `atom` (`Vm::method_caches_may_name`), from the payload's
/// atom bits; an atom past them is unnamed.
pub(super) fn emit_atom_unnamed(builder: &mut FunctionBuilder<'_>, payload: ir::Value, atom: Atom) -> ir::Value {
    let word_index = atom / u64::BITS;
    let words = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, method_cache_atoms));
    let words_len = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, method_cache_atom_words));
    let covered = builder.ins().icmp_imm_u(IntCC::UnsignedGreaterThan, words_len, i64::from(word_index));
    let (present, join) = (builder.create_block(), builder.create_block());
    let free = builder.append_block_param(join, types::I8);
    let one = builder.ins().iconst(types::I8, 1);
    builder.ins().brif(covered, present, &[], join, &[ir::BlockArg::Value(one)]);
    builder.switch_to_block(present);
    let word = builder.ins().load(types::I64, MemFlagsData::trusted(), words,
        i32::try_from(word_index as usize * size_of::<u64>()).expect("atom word offset fits i32"));
    let named = builder.ins().band_imm_u(word, (1_u64 << (atom % u64::BITS)) as i64);
    let unnamed = builder.ins().icmp_imm_u(IntCC::Equal, named, 0);
    builder.ins().jump(join, &[ir::BlockArg::Value(unnamed)]);
    builder.switch_to_block(join);
    free
}
