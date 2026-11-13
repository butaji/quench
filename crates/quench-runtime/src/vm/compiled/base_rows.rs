//! Stencil rows that read the heap: a captured value, a dense array element read or replaced, an
//! entry-record field. Each row is the staged row (`staged/{environment,elements,entry,value}.rs`)
//! run on the stencil stage and compiled by rustc at build time (`quench-bjit/stencils`), so a row
//! has one definition of its layout reads across the interpreter, the specializers and here.
//!
//! Operands are holes: 0 is the destination frame offset (the array's, for a store), 1 and 2 the
//! operands' (a field row's entry-record byte offset in 1; a capture's slot and the slot's byte
//! offset; a site field row packs its object register's frame offset above the destination's in
//! 0, takes the narrow upper-word table in 1 and the cache entry's byte offset in 2). Every failed proof leaves through exit 0 with nothing written, so the interpreter
//! runs the instruction.

use crate::staged::Form;
use quench_bjit::numeric::{library, Place};
use quench_bjit::Stencil;
use std::collections::HashMap;
use std::sync::Mutex;

/// What a heap row does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Heap {
    /// Depth-0 capture `slot` of the running activation. It continues at TAKEN (`continues_at_taken`), as a fast arm
    /// of a body's `LoadCapture` whose helper step answers a slot the dense run does not hold (a root binding's shared slot).
    Capture(u16),
    /// `dst = array[index]` of a dense array: the array word at hole 1, the index at hole 2.
    Index,
    /// `array[index] = value` replacing a present element in place: the array word at hole 0, the
    /// value at hole 1, the index at hole 2.
    Store,
    /// The field value the loop entry recorded: its byte offset in the record at hole 1.
    Field,
    /// The array word at hole 1 is a direct array (what `RequireObjectCoercible` of one proves).
    Array,
    /// The data field the site's field cache entry names, own or held by the receiver's prototype
    /// (`entry::site_read`), read from the object register whose frame offset sits above the
    /// destination's in hole 0 (hole 2: the entry's byte offset).
    Site,
    /// A chain of two such reads, `object.first.second` (a nested field lookup), whose entry offsets
    /// share hole 2: the first below `CHAIN_SECOND_AT` bits, the second above them.
    SiteChain,
    /// `Site` and `SiteChain` of the activation's receiver, which the payload holds: hole 0 is the
    /// destination alone.
    ThisSite,
    ThisChain,
    /// The `length` of the direct array at hole 1, an int32 (what the array's record counts).
    Length,
    /// Stores the value operand (hole 1) into the own data field the site's cache entry (hole 2) names
    /// of the object register whose frame offset is hole 0, where the store gate admits it.
    SiteStore,
    /// `SiteStore` into the activation's receiver: hole 0 is unused.
    ThisStore,
    /// Stores the value operand (hole 0) into depth-0 capture `slot` of an activation whose slots
    /// admit every direct store; the slot is hole 1 and its byte offset hole 2. It continues at TAKEN, as `Capture` does.
    CaptureSet(u16),
    /// The running activation's `this`.
    This,
    /// The activation's local slot `slot`, whose number is hole 1.
    LoadLocal(u16),
    /// The initialized binding `slot` (hole 1, its byte offset hole 2) of the storage of the running activation's
    /// environment window (`JsStepDispatch::environment_storage`) into the register at hole 0, else the row leaves into
    /// the helper step after it. It continues at TAKEN, as `Capture` does.
    EnvLocal(u16),
    /// The register at hole 0 stored to that slot where its binding is initialized, else the row leaves into the helper
    /// step after it; the slot is hole 1 and its byte offset hole 2. It continues at TAKEN.
    EnvStore(u16),
    /// The first operand written to the activation's local slot `slot`, whose number is hole 2. It continues at TAKEN
    /// (`continues_at_taken`): as a fast arm it hops past the helper step, in a loop region it branches to the next
    /// instruction.
    StoreLocal(u16),
    /// The fast arm of a `LoadLocal` of a frame-owned slot (`staged::read_initialized_local`): slot `slot`
    /// (hole 1) into the register at hole 0 where its binding is initialized, else the row leaves into the
    /// helper step after it, which raises.
    LoadChecked(u16),
    /// The fast arm of a `StoreLocal` that does not initialize its binding (`staged::write_initialized_local`):
    /// the first operand to slot `slot` (hole 2) where the binding is initialized.
    StoreChecked(u16),
    /// The register at the frame offset of hole 1 holds exactly the word of hole 2.
    Identity,
    /// A prototype-held field folded to the value its holder held when the code was compiled (hole 2):
    /// the object register whose frame offset sits above the destination's in hole 0 is a direct object
    /// of the exact shape in hole 1's low half and the prototype link in its high half, else the row
    /// leaves. A Stable fact holds the value (`entry::held_receiver`).
    Held,
    /// `Held` of the activation's receiver, which the payload holds: hole 0 is the destination alone.
    ThisHeld,
    /// Branches to its taken successor, an entry's failure, unless the active realm's global object (the
    /// word at the byte offset in hole 1 of the VM) is the word of hole 2: the realm a body folded
    /// facts of its callees or its global names in (`Vm::realm_globals_offset`), proved once, at the entry.
    Realm,
    /// Leaves unless the invalidation word at the address in hole 1 is zero (`code_dependencies::Invalidation`):
    /// the check before a row that holds a Stable fact where a call may have changed it.
    Live,
    /// The fast arm of `RequireIteratorResult` (`staged::iteration`): the register at the frame offset
    /// of hole 1 holds a direct object or array record, else the row leaves into the helper step after it.
    Object,
    /// The fast arm of `ToPropertyKey`: the register at hole 1 holds a direct string, which is copied to
    /// the register at hole 0, else the row leaves into the helper step after it.
    Key,
    /// The fast arm of `Unary`'s `typeof` index (`staged::value::immediate_typeof_index`): the register at
    /// hole 1 holds a value whose tag decides its index, an int written to the register at hole 0, else the
    /// row leaves into the helper step after it.
    TypeofIndex,
    /// The fast arm of `Unary`'s `typeof` string (`staged::value::immediate_typeof_string`): the register at hole 1
    /// holds a value whose tag decides its index, the entry of the table of result strings at hole 2 written to
    /// the register at hole 0, else the row leaves into the helper step after it.
    TypeofString,
    /// The fast arm of `Binary`'s `instanceof` of a constructor the code holds (`staged::object::instanceof_by_words`): the
    /// register at hole 0's upper half is walked along its prototype chain for the `prototype` link the code holds (hole 1's
    /// upper half, beside the constructor register's offset), where the constructor register holds exactly the identity of
    /// hole 2, a boolean written to the register at hole 0's lower half, else the row leaves into the helper step after it.
    Instanceof,
    /// The fast arm of `Unary`'s `typeof` test: whether (or, where `negated`, whether not) that index is
    /// `index` (hole 2), as a boolean.
    TypeofIs { index: u8, negated: bool },
    /// The global object's data slot (hole 2: its byte offset in the arena run behind its extras) read into the
    /// destination, the object being the record id of hole 1: a Stable fact holds the object's layout.
    GlobalRead,
    /// The first operand stored to that slot (hole 0: the record id), where the row's `entry` variant takes the
    /// store gate for an operand that may be a heap reference, and a Number operand needs none.
    GlobalStore,
    /// The tagged word of hole 1 into the destination: an immutable global, a folded `typeof` name, and
    /// the `undefined` receiver of a name call.
    Word,
    /// The fast arm of `AppendArray` (`elements::append_in_room`): the Number operand at hole 1 is appended to
    /// the unique writable dense array in the frame slot of hole 0 where the backing's capacity word proves
    /// room, else the row leaves into the helper step after it, which grows the backing.
    Append,
    /// The property add of a `DefineField` or an adding `SetField` (`staged::add::site_add`): the value
    /// operand (hole 1) joins the object register (its frame offset in hole 2) or, with `this`, the
    /// activation's receiver, by the transition holes 0 and 2 carry (`library::ADD_AFTER_AT`,
    /// `ADD_OBJECT_AT`), where the receiver holds the before-shape in `form`'s storage and, for the
    /// row's `entry` variant, the prototype link hole 2 names. A hit continues at TAKEN; a miss leaves
    /// into the helper step after the row, or the leave a guarded row makes.
    Add { form: Form, this: bool },
    /// The fast arm of `ForInNext` (`staged::iteration::for_in_next`): the for-in state in the frame slot of
    /// hole 1 yields its next key to the slot of hole 0 where its memo proves the snapshot current, else the row
    /// leaves into the helper step after it, which takes any other step and the exhausted one's jump.
    ForIn,
    /// The fast arm of `SetFunctionName` (`staged::naming::site_name`): the function in the frame slot
    /// whose offset hole 2 carries (`library::NAME_FUNCTION_AT`) takes the name word of hole 1 in the slot
    /// hole 2 names, moving between the shapes of hole 0 (`library::ADD_AFTER_AT`), where it holds the empty
    /// name in `form`'s storage. A hit continues at TAKEN; a miss leaves into the helper step after the row.
    Name { form: Form },
}

/// The storage forms an add row exists for, in the order of `library::ADD_ROWS` (packed, arena, either).
pub(super) const FORMS: [Form; 3] = [Form::Packed, Form::Arena, Form::Either];

/// A heap row: its operation and where its operands and destination live.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct HeapRow {
    pub(super) heap: Heap,
    pub(super) dst: Place,
    pub(super) a: Place,
    pub(super) b: Place,
    /// The row's variant: an array's backing is the entry record's (the offset at the array hole), not
    /// looked up per access (`Index`, `Store`); a site read is a chain of two reads (`SiteChain`).
    pub(super) entry: bool,
}

impl Heap {
    /// The row's code in `quench_bjit::numeric::library`.
    pub(super) fn code(self) -> u8 {
        match self {
            Heap::Capture(_) => library::CAPTURE_ROW,
            Heap::Index => library::INDEX_ROW,
            Heap::Store => library::STORE_ROW,
            Heap::Field => library::FIELD_ROW,
            Heap::Array => library::ARRAY_ROW,
            Heap::Site | Heap::SiteChain => library::SITE_ROW,
            Heap::ThisSite | Heap::ThisChain => library::THIS_SITE_ROW,
            Heap::Length => library::LENGTH_ROW,
            Heap::CaptureSet(_) => library::CAPTURE_SET_ROW,
            Heap::EnvLocal(_) => library::ENV_ROW,
            Heap::EnvStore(_) => library::ENV_SET_ROW,
            Heap::SiteStore => library::SITE_STORE_ROW,
            Heap::ThisStore => library::THIS_STORE_ROW,
            Heap::This => library::THIS_ROW,
            Heap::LoadLocal(_) => library::LOAD_LOCAL_ROW,
            Heap::StoreLocal(_) => library::STORE_LOCAL_ROW,
            Heap::LoadChecked(_) => library::LOAD_CHECKED_ROW,
            Heap::StoreChecked(_) => library::STORE_CHECKED_ROW,
            Heap::Identity => library::IDENTITY_ROW,
            Heap::Held | Heap::ThisHeld => library::HELD_ROW,
            Heap::Live => library::LIVE_ROW,
            Heap::Object => library::OBJECT_ROW,
            Heap::Key => library::KEY_ROW,
            Heap::TypeofIndex => library::TYPEOF_INDEX_ROW,
            Heap::TypeofString => library::TYPEOF_STRING_ROW,
            Heap::Instanceof => library::INSTANCEOF_ROW,
            Heap::TypeofIs { negated: false, .. } => library::TYPEOF_IS_ROW,
            Heap::TypeofIs { negated: true, .. } => library::TYPEOF_ISNOT_ROW,
            Heap::Realm => library::REALM_ROW,
            Heap::GlobalRead => library::GLOBAL_READ_ROW,
            Heap::GlobalStore => library::GLOBAL_STORE_ROW,
            Heap::Word => library::CONSTANT_ROW,
            Heap::Append => library::APPEND_ROW,
            Heap::ForIn => library::FOR_IN_ROW,
            Heap::Name { form } => library::NAME_ROWS[usize::from(form == Form::Arena)],
            Heap::Add { form, this } => library::ADD_ROWS[usize::from(this) * FORMS.len() + FORMS.iter().position(|known| *known == form).unwrap_or_default()],
        }
    }

    /// Whether a hit continues at the row's TAKEN exit, not its NEXT: the rows of one convention with the fast arms, whose
    /// hit hops past the step laid out after them (`base_region::steps::plan_fast`).
    pub(super) fn continues_at_taken(self) -> bool {
        matches!(self, Heap::StoreLocal(_) | Heap::Capture(_) | Heap::CaptureSet(_) | Heap::EnvLocal(_) | Heap::EnvStore(_))
    }

    /// Whether the row reads a snapshot of the heap the loop's entry took (the record's captures,
    /// fields and array views) or tables a helper may move: not the activation's `this` and local
    /// slots, which every run reads live, and the rows that read no heap.
    pub(super) fn reads_snapshot(self) -> bool {
        !matches!(self, Heap::This | Heap::LoadLocal(_) | Heap::StoreLocal(_) | Heap::LoadChecked(_) | Heap::StoreChecked(_) | Heap::EnvLocal(_) | Heap::EnvStore(_) | Heap::Identity | Heap::Live | Heap::Realm | Heap::Word | Heap::Object | Heap::Key | Heap::Append | Heap::Add { .. } | Heap::ForIn | Heap::Name { .. } | Heap::TypeofIndex | Heap::TypeofString | Heap::Instanceof | Heap::TypeofIs { .. })
    }

    /// Whether the row reads a word of its operand's frame slot (a fast arm that tests the word),
    /// which a register held in a pinned register as a Number does not have there.
    pub(super) fn reads_frame_word(self) -> bool {
        matches!(self, Heap::Object | Heap::Key | Heap::ForIn | Heap::TypeofIndex | Heap::TypeofString | Heap::Instanceof | Heap::TypeofIs { .. })
    }

    /// Whether the row's result is a tagged word whatever the registers around it hold: a value
    /// the activation holds, not a Number the region proved.
    pub(super) fn yields_tagged(self) -> bool {
        matches!(self, Heap::This | Heap::LoadLocal(_) | Heap::Word)
    }
}

impl HeapRow {
    /// The row's canonical code in `quench_bjit::numeric::library`.
    pub(super) fn code(self) -> library::HeapCode {
        library::heap_canonical((self.heap.code(), self.entry, self.dst.code(), self.a.code(), self.b.code()))
    }
}

/// The stencil of `row`, built at compile time by rustc; a row the library lacks declines the region.
pub(super) fn stencil(row: HeapRow) -> Result<&'static Stencil, String> {
    static TABLE: Mutex<Option<HashMap<library::HeapCode, &'static Stencil>>> = Mutex::new(None);
    let code = row.code();
    let mut table = TABLE.lock().map_err(|e| e.to_string())?;
    let table = table.get_or_insert_with(HashMap::new);
    if let Some(found) = table.get(&code) {
        return Ok(found);
    }
    let found = quench_bjit::rustc::stencil(&library::heap_name(code)).ok_or_else(|| format!("no stencil row {row:?}"))?;
    let stencil: &'static Stencil = Box::leak(Box::new(found));
    table.insert(code, stencil);
    Ok(stencil)
}

#[cfg(test)]
mod tests {

    /// The numbers the stencil rows are compiled with (`quench-bjit/stencils/layout.rs`) are
    /// the runtime's own.
    use super::super::{CallContext, JsStepDispatch};
    use crate::vm::wtf16::native_string as form;
    #[allow(dead_code)]
    mod stencil_layout {
        include!("../../../../quench-bjit/stencils/layout.rs");
    }


    /// Every payload field a stencil row reads, as (name, the runtime's offset, the stencil layout's).
    fn payload_offsets() -> Vec<(&'static str, usize, usize)> {
        use stencil_layout as m;
        use std::mem::offset_of as at;
                vec![
            ("adapter_payload", at!(CallContext, adapter_payload), m::CALL_CONTEXT_ADAPTER_PAYLOAD),
            ("loop_entry", at!(JsStepDispatch, loop_entry), m::JS_STEP_LOOP_ENTRY),
            ("capture_values", at!(JsStepDispatch, capture_values), m::JS_STEP_CAPTURE_VALUES),
            ("capture_len", at!(JsStepDispatch, capture_len), m::JS_STEP_CAPTURE_LEN),
            ("capture_stores", at!(JsStepDispatch, capture_stores), m::JS_STEP_CAPTURE_STORES),
            ("array_record_base", at!(JsStepDispatch, array_record_base), m::JS_STEP_ARRAY_RECORD_BASE),
            ("object_record_base", at!(JsStepDispatch, object_record_base), m::JS_STEP_OBJECT_RECORD_BASE),
            ("function_record_base", at!(JsStepDispatch, function_record_base), m::JS_STEP_FUNCTION_RECORD_BASE),
            ("object_mark_blocks", at!(JsStepDispatch, object_mark_blocks), m::JS_STEP_OBJECT_MARK_BLOCKS),
            ("object_mark_block_count", at!(JsStepDispatch, object_mark_block_count), m::JS_STEP_OBJECT_MARK_BLOCK_COUNT),
            ("property_buffer", at!(JsStepDispatch, property_buffer), m::JS_STEP_PROPERTY_BUFFER),
            ("field_entries", at!(JsStepDispatch, field_entries), m::JS_STEP_FIELD_ENTRIES),
            ("method_cache_atoms", at!(JsStepDispatch, method_cache_atoms), m::JS_STEP_METHOD_CACHE_ATOMS),
            ("method_cache_atom_words", at!(JsStepDispatch, method_cache_atom_words), m::JS_STEP_METHOD_CACHE_ATOM_WORDS),
            ("stores_specialized", at!(JsStepDispatch, stores_specialized), m::JS_STEP_STORES_SPECIALIZED),
            ("slots_tracked", at!(JsStepDispatch, slots_tracked), m::JS_STEP_SLOTS_TRACKED),
            ("receiver", at!(JsStepDispatch, receiver), m::JS_STEP_RECEIVER),
            ("vm", at!(JsStepDispatch, vm), m::JS_STEP_VM),
            ("residual", at!(JsStepDispatch, residual), m::JS_STEP_RESIDUAL),
            ("lane_context", at!(JsStepDispatch, lane_context), m::JS_STEP_LANE_CONTEXT),
            ("returned_pc", at!(JsStepDispatch, returned_pc), m::JS_STEP_RETURNED_PC),
            ("lane_values_top", at!(JsStepDispatch, lane_values_top), m::JS_STEP_LANE_VALUES_TOP),
            ("lane_values_end", at!(JsStepDispatch, lane_values_end), m::JS_STEP_LANE_VALUES_END),
            ("lane_activations_top", at!(JsStepDispatch, lane_activations_top), m::JS_STEP_LANE_ACTIVATIONS_TOP),
            ("lane_frames_end", at!(JsStepDispatch, lane_frames_end), m::JS_STEP_LANE_FRAMES_END),
            ("native_locals", at!(JsStepDispatch, native_locals), m::JS_STEP_NATIVE_LOCALS),
            ("local_count", at!(JsStepDispatch, local_count), m::JS_STEP_LOCAL_COUNT),
            ("epoch", at!(JsStepDispatch, epoch), m::JS_STEP_EPOCH),
            ("string_record_base", at!(JsStepDispatch, string_record_base), m::JS_STEP_STRING_RECORD_BASE),
            ("function_mark_blocks", at!(JsStepDispatch, function_mark_blocks), m::JS_STEP_FUNCTION_MARK_BLOCKS),
            ("function_mark_block_count", at!(JsStepDispatch, function_mark_block_count), m::JS_STEP_FUNCTION_MARK_BLOCK_COUNT),
            ("empty_string", at!(JsStepDispatch, empty_string), m::JS_STEP_EMPTY_STRING),
            ("environment_storage", at!(JsStepDispatch, environment_storage), m::JS_STEP_ENV_STORAGE),
            ("property_buffer_base", crate::value_vec::NATIVE_PROPERTY_BUFFER_LAYOUT.base, m::PROPERTY_BUFFER_BASE),
            ("capture_address_bits", super::super::entry_cell::CAPTURE_ADDRESS_BITS as usize, m::CAPTURE_ADDRESS_BITS as usize),
            ("capture_stores_bit", super::super::super::dispatch_fast::CAPTURE_STORES, m::CAPTURE_STORES_BIT as usize),
            ("activation_kind", crate::vm::dispatch_fast::KIND_OFFSET, m::ACTIVATION_KIND_OFFSET),
            ("kind_native", usize::from(crate::vm::dispatch_fast::KIND_NATIVE), usize::from(m::KIND_NATIVE)),
            ("cell_entry", at!(super::super::entry_cell::EntryCell, entry), m::CELL_ENTRY_AT),
            ("cell_recovery", at!(super::super::entry_cell::EntryCell, recovery), m::CELL_RECOVERY_AT),
            ("cell_body", at!(super::super::entry_cell::EntryCell, body), m::CELL_BODY_AT),
            ("cell_counters", at!(super::super::entry_cell::EntryCell, counters), m::CELL_COUNTERS_AT),
            ("raised_run", super::super::entry_cell::RAISED_RUN as usize, m::RAISED_RUN as usize),
            ("frame_bytes", super::super::FRAME_BYTES, m::ACTIVATION_BYTES),
            ("short_units", form::SHORT_UNITS_OFFSET as usize, m::string_form::SHORT_UNITS_OFFSET as usize),
            ("shared_pointer", form::SHARED_POINTER_OFFSET as usize, m::string_form::SHARED_POINTER_OFFSET as usize),
            ("shared_units", form::shared_units_offset() as usize, m::string_form::shared_units_offset() as usize),
        ]
    }

    #[test]
    fn stencil_layout_matches() {
        use crate::value::{NarrowLayout, ValueLayout};
        use stencil_layout as mirror;
        eprintln!("OFFSETS {:?}", payload_offsets());
        assert_eq!(format!("{:?}", crate::heap::NATIVE_OBJECT_LAYOUT), format!("{:?}", mirror::NATIVE_OBJECT_LAYOUT));
        assert_eq!(format!("{:?}", crate::heap::NATIVE_FUNCTION_LAYOUT), format!("{:?}", mirror::NATIVE_FUNCTION_LAYOUT));
        assert_eq!(format!("{:?}", crate::heap::NATIVE_ARRAY_LAYOUT), format!("{:?}", mirror::NATIVE_ARRAY_LAYOUT));
        assert_eq!(format!("{:?}", crate::heap::NATIVE_TYPED_ARRAY_LAYOUT), format!("{:?}", mirror::NATIVE_TYPED_ARRAY_LAYOUT));
        assert_eq!(format!("{:?}", crate::heap::NATIVE_ENVIRONMENT_LAYOUT), format!("{:?}", mirror::NATIVE_ENVIRONMENT_LAYOUT));
        let (cache, copy) = (crate::vm::NATIVE_FIELD_CACHE_LAYOUT, mirror::NATIVE_FIELD_CACHE_LAYOUT);
        assert_eq!([cache.entry_size, cache.receiver_offset, cache.holder_offset, cache.slot_offset], [copy.entry_size, copy.receiver_offset, copy.holder_offset, copy.slot_offset]);
        assert_eq!(
            [cache.receiver_complement, cache.holder_complement, cache.slot_complement, cache.own_holder],
            [copy.receiver_complement, copy.holder_complement, copy.slot_complement, copy.own_holder]
        );
        assert_eq!(
            [crate::heap::OTHERS_UNKNOWN as u64, crate::heap::LENGTH_FROM_VALUES as i64 as u64, crate::heap::OTHERS_FROM_VALUES as i64 as u64, crate::value_vec::INLINE_PROPERTY_COUNT as u64],
            [mirror::OTHERS_UNKNOWN as u64, mirror::LENGTH_FROM_VALUES as i64 as u64, mirror::OTHERS_FROM_VALUES as i64 as u64, mirror::INLINE_PROPERTIES as u64]
        );
        use mirror::words as w;
        assert_eq!(
            [ValueLayout::TAG_MASK, ValueLayout::INT_TAG, ValueLayout::INTERNAL_TAG, ValueLayout::HEAP_TAG, ValueLayout::DIRECT_ARRAY_TAG, ValueLayout::DIRECT_OBJECT_TAG, ValueLayout::DIRECT_FUNCTION_TAG, ValueLayout::HEAP_FAMILY_MASK, ValueLayout::DIRECT_STRING_TAG, u64::from(ValueLayout::TAG_SHIFT), ValueLayout::FIRST_TAG_FIELD, ValueLayout::TAG_FIELD_SPAN, ValueLayout::CANONICAL_NAN, crate::Value::NULL.to_bits(), crate::Value::UNDEFINED.to_bits(), crate::Value::DELETED.to_bits(), crate::Value::FALSE.to_bits(), crate::Value::TRUE.to_bits()],
            [w::TAG_MASK, w::INT_TAG, w::INTERNAL_TAG, w::HEAP_TAG, w::DIRECT_ARRAY_TAG, w::DIRECT_OBJECT_TAG, w::DIRECT_FUNCTION_TAG, w::HEAP_FAMILY_MASK, w::DIRECT_STRING_TAG, u64::from(w::TAG_SHIFT), w::FIRST_TAG_FIELD, w::TAG_FIELD_SPAN, w::CANONICAL_NAN, w::NULL, w::UNDEFINED, w::DELETED, w::FALSE, w::TRUE]
        );
        assert_eq!(
            [crate::value::LOWER_WORD, crate::value::HEAP_FAMILY_BITS, u64::from(crate::value::HEAP_FAMILY_SHIFT), crate::value::NARROW_HEAP_FAMILIES as u64, crate::value::TAG_UNDEFINED, crate::value::TAG_NULL, crate::value::TAG_BOOL, crate::value::TAG_DELETED],
            [w::LOWER_WORD, w::HEAP_FAMILY_BITS, u64::from(w::HEAP_FAMILY_SHIFT), w::NARROW_HEAP_FAMILIES as u64, w::UNDEFINED, w::NULL, w::FALSE, w::DELETED]
        );
        assert_eq!(
            [u64::from(NarrowLayout::INTEGER_FLAG), u64::from(NarrowLayout::INTEGER_BITS), u64::from(NarrowLayout::CODE_SHIFT), u64::from(NarrowLayout::CODE_MASK), u64::from(NarrowLayout::INDEX_SHIFT), u64::from(NarrowLayout::INDEX_BITS), NarrowLayout::DIRECT_OBJECT_UPPER, u64::from(NarrowLayout::DIRECT_OBJECT_CODE)],
            [u64::from(w::NARROW_INT_FLAG), u64::from(w::NARROW_INTEGER_BITS), u64::from(w::NARROW_CODE_SHIFT), u64::from(w::NARROW_CODE_MASK), u64::from(w::NARROW_INDEX_SHIFT), u64::from(w::NARROW_INDEX_BITS), w::DIRECT_OBJECT_UPPER, u64::from(w::DIRECT_OBJECT_NARROW_CODE)]
        );
        assert_eq!(super::super::NATIVE_JS_STOPPED, mirror::call_record::NATIVE_JS_STOPPED);
        for (name, runtime, copy) in payload_offsets() {
            assert_eq!(runtime, copy, "{name}: the runtime's offset is the stencil layout's");
        }
        assert_eq!(
            [form::RECORD_SIZE as i32, form::TAG_OFFSET, i32::from(form::TAG_SHORT), i32::from(form::TAG_ASCII), i32::from(form::TAG_WIDE), form::SHORT_LENGTH_OFFSET, form::SHARED_LENGTH_OFFSET],
            [mirror::string_form::RECORD_SIZE as i32, mirror::string_form::TAG_OFFSET, i32::from(mirror::string_form::TAG_SHORT), i32::from(mirror::string_form::TAG_ASCII), i32::from(mirror::string_form::TAG_WIDE), mirror::string_form::SHORT_LENGTH_OFFSET, mirror::string_form::SHARED_LENGTH_OFFSET]
        );
    }
}
