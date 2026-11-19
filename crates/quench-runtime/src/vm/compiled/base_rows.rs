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

/// The stencil layout numbers (`staged/layout_offsets.rs`, shared with the stencils by `#[path]`) are the
/// runtime's own: each is a `const` check against the runtime's `offset_of!`, `size_of` or layout const.
/// The stencils are x86-64 only, so the checks are too.
#[cfg(target_arch = "x86_64")]
mod layout_checks {
    use super::super::{CallContext, JsStepDispatch};
    use crate::staged::layout_offsets as L;
    use std::mem::offset_of;

    const _: () = assert!(offset_of!(CallContext, adapter_payload) == L::CALL_CONTEXT_ADAPTER_PAYLOAD);
    const _: () = assert!(L::CAPTURE_ADDRESS_PAD == u64::BITS - quench_bjit::shared_words::CAPTURE_ADDRESS_BITS);

    // The dispatch record's head is its first field, so the stencil's `JS_STEP_*` offsets (`js_step_head`) are its own.
    const _: () = assert!(offset_of!(JsStepDispatch, head) == 0);

    // The layout structs: every field of each is the runtime's.
    // NATIVE_OBJECT_LAYOUT
    const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.record_capacity == L::NATIVE_OBJECT_LAYOUT.record_capacity);
    const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.record_size == L::NATIVE_OBJECT_LAYOUT.record_size);
    const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.record_align == L::NATIVE_OBJECT_LAYOUT.record_align);
    const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.layout_offset == L::NATIVE_OBJECT_LAYOUT.layout_offset);
    const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.meta_offset == L::NATIVE_OBJECT_LAYOUT.meta_offset);
    const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.auxiliary_offset == L::NATIVE_OBJECT_LAYOUT.auxiliary_offset);
    const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.payload_offset == L::NATIVE_OBJECT_LAYOUT.payload_offset);
    const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.extras_arena_start_offset == L::NATIVE_OBJECT_LAYOUT.extras_arena_start_offset);
    const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.extras_inline_values_offset == L::NATIVE_OBJECT_LAYOUT.extras_inline_values_offset);
    const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.inline_storage_mask == L::NATIVE_OBJECT_LAYOUT.inline_storage_mask);
    const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.array_mask == L::NATIVE_OBJECT_LAYOUT.array_mask);
    const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.storage_start_mask == L::NATIVE_OBJECT_LAYOUT.storage_start_mask);
    const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.dictionary_mask == L::NATIVE_OBJECT_LAYOUT.dictionary_mask);
    const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.inline_form == L::NATIVE_OBJECT_LAYOUT.inline_form);
    const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.extras_form == L::NATIVE_OBJECT_LAYOUT.extras_form);
    const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.narrow_form == L::NATIVE_OBJECT_LAYOUT.narrow_form);
    const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.narrow_run_tag == L::NATIVE_OBJECT_LAYOUT.narrow_run_tag);
    const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.integrity_mask == L::NATIVE_OBJECT_LAYOUT.integrity_mask);
    const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.inline_property_count == L::NATIVE_OBJECT_LAYOUT.inline_property_count);
    const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.extras_tag == L::NATIVE_OBJECT_LAYOUT.extras_tag);
    const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.extras_tag_mask == L::NATIVE_OBJECT_LAYOUT.extras_tag_mask);
    const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.proto_mask == L::NATIVE_OBJECT_LAYOUT.proto_mask);
    const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.proto_record_base == L::NATIVE_OBJECT_LAYOUT.proto_record_base);
    const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.proto_record_end == L::NATIVE_OBJECT_LAYOUT.proto_record_end);
    const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.mark_records_per_block == L::NATIVE_OBJECT_LAYOUT.mark_records_per_block);
    const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.mark_word_bits == L::NATIVE_OBJECT_LAYOUT.mark_word_bits);
    const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.mark_block_size == L::NATIVE_OBJECT_LAYOUT.mark_block_size);
    const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.mark_offset == L::NATIVE_OBJECT_LAYOUT.mark_offset);
    const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.property_value_size == L::NATIVE_OBJECT_LAYOUT.property_value_size);
    const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.property_value_align == L::NATIVE_OBJECT_LAYOUT.property_value_align);

    // NATIVE_FUNCTION_LAYOUT
    const _: () = assert!(crate::heap::NATIVE_FUNCTION_LAYOUT.record_size == L::NATIVE_FUNCTION_LAYOUT.record_size);
    const _: () = assert!(crate::heap::NATIVE_FUNCTION_LAYOUT.object_offset == L::NATIVE_FUNCTION_LAYOUT.object_offset);

    // NATIVE_ARRAY_LAYOUT
    const _: () = assert!(crate::heap::NATIVE_ARRAY_LAYOUT.record_capacity == L::NATIVE_ARRAY_LAYOUT.record_capacity);
    const _: () = assert!(crate::heap::NATIVE_ARRAY_LAYOUT.record_size == L::NATIVE_ARRAY_LAYOUT.record_size);
    const _: () = assert!(crate::heap::NATIVE_ARRAY_LAYOUT.record_align == L::NATIVE_ARRAY_LAYOUT.record_align);
    const _: () = assert!(crate::heap::NATIVE_ARRAY_LAYOUT.elements_offset == L::NATIVE_ARRAY_LAYOUT.elements_offset);
    const _: () = assert!(crate::heap::NATIVE_ARRAY_LAYOUT.extras_elements_offset == L::NATIVE_ARRAY_LAYOUT.extras_elements_offset);
    const _: () = assert!(crate::heap::NATIVE_ARRAY_LAYOUT.elements_header_offset == L::NATIVE_ARRAY_LAYOUT.elements_header_offset);
    const _: () = assert!(crate::heap::NATIVE_ARRAY_LAYOUT.header_length_offset == L::NATIVE_ARRAY_LAYOUT.header_length_offset);
    const _: () = assert!(crate::heap::NATIVE_ARRAY_LAYOUT.header_values_offset == L::NATIVE_ARRAY_LAYOUT.header_values_offset);
    const _: () = assert!(crate::heap::NATIVE_ARRAY_LAYOUT.header_owners_offset == L::NATIVE_ARRAY_LAYOUT.header_owners_offset);
    const _: () = assert!(crate::heap::NATIVE_ARRAY_LAYOUT.header_capacity_offset == L::NATIVE_ARRAY_LAYOUT.header_capacity_offset);
    const _: () = assert!(crate::heap::NATIVE_ARRAY_LAYOUT.capacity_mask == L::NATIVE_ARRAY_LAYOUT.capacity_mask);
    const _: () = assert!(crate::heap::NATIVE_ARRAY_LAYOUT.object_offset == L::NATIVE_ARRAY_LAYOUT.object_offset);
    const _: () = assert!(crate::heap::NATIVE_ARRAY_LAYOUT.frozen_mask == L::NATIVE_ARRAY_LAYOUT.frozen_mask);
    const _: () = assert!(crate::heap::NATIVE_ARRAY_LAYOUT.integrity_mask == L::NATIVE_ARRAY_LAYOUT.integrity_mask);

    // NATIVE_TYPED_ARRAY_LAYOUT
    const _: () = assert!(crate::heap::NATIVE_TYPED_ARRAY_LAYOUT.entry_size == L::NATIVE_TYPED_ARRAY_LAYOUT.entry_size);
    const _: () = assert!(crate::heap::NATIVE_TYPED_ARRAY_LAYOUT.page_shift == L::NATIVE_TYPED_ARRAY_LAYOUT.page_shift);
    const _: () = assert!(crate::heap::NATIVE_TYPED_ARRAY_LAYOUT.page_mask == L::NATIVE_TYPED_ARRAY_LAYOUT.page_mask);
    const _: () = assert!(crate::heap::NATIVE_TYPED_ARRAY_LAYOUT.kind_offset == L::NATIVE_TYPED_ARRAY_LAYOUT.kind_offset);
    const _: () = assert!(crate::heap::NATIVE_TYPED_ARRAY_LAYOUT.detached_offset == L::NATIVE_TYPED_ARRAY_LAYOUT.detached_offset);
    const _: () = assert!(crate::heap::NATIVE_TYPED_ARRAY_LAYOUT.unshared_offset == L::NATIVE_TYPED_ARRAY_LAYOUT.unshared_offset);
    const _: () = assert!(crate::heap::NATIVE_TYPED_ARRAY_LAYOUT.data_offset == L::NATIVE_TYPED_ARRAY_LAYOUT.data_offset);
    const _: () = assert!(crate::heap::NATIVE_TYPED_ARRAY_LAYOUT.length_offset == L::NATIVE_TYPED_ARRAY_LAYOUT.length_offset);
    const _: () = assert!(crate::heap::NATIVE_TYPED_ARRAY_LAYOUT.stamp_offset == L::NATIVE_TYPED_ARRAY_LAYOUT.stamp_offset);

    // NATIVE_ENVIRONMENT_LAYOUT
    const _: () = assert!(crate::heap::NATIVE_ENVIRONMENT_LAYOUT.header_pointer_offset == L::NATIVE_ENVIRONMENT_LAYOUT.header_pointer_offset);
    const _: () = assert!(crate::heap::NATIVE_ENVIRONMENT_LAYOUT.sparse_mark == L::NATIVE_ENVIRONMENT_LAYOUT.sparse_mark);
    const _: () = assert!(crate::heap::NATIVE_ENVIRONMENT_LAYOUT.slots_offset == L::NATIVE_ENVIRONMENT_LAYOUT.slots_offset);
    const _: () = assert!(crate::heap::NATIVE_ENVIRONMENT_LAYOUT.slot_size == L::NATIVE_ENVIRONMENT_LAYOUT.slot_size);

    // NATIVE_FIELD_CACHE_LAYOUT
    const _: () = assert!(crate::vm::NATIVE_FIELD_CACHE_LAYOUT.entry_size == L::NATIVE_FIELD_CACHE_LAYOUT.entry_size);
    const _: () = assert!(crate::vm::NATIVE_FIELD_CACHE_LAYOUT.receiver_offset == L::NATIVE_FIELD_CACHE_LAYOUT.receiver_offset);
    const _: () = assert!(crate::vm::NATIVE_FIELD_CACHE_LAYOUT.holder_offset == L::NATIVE_FIELD_CACHE_LAYOUT.holder_offset);
    const _: () = assert!(crate::vm::NATIVE_FIELD_CACHE_LAYOUT.slot_offset == L::NATIVE_FIELD_CACHE_LAYOUT.slot_offset);
    const _: () = assert!(crate::vm::NATIVE_FIELD_CACHE_LAYOUT.receiver_complement == L::NATIVE_FIELD_CACHE_LAYOUT.receiver_complement);
    const _: () = assert!(crate::vm::NATIVE_FIELD_CACHE_LAYOUT.holder_complement == L::NATIVE_FIELD_CACHE_LAYOUT.holder_complement);
    const _: () = assert!(crate::vm::NATIVE_FIELD_CACHE_LAYOUT.slot_complement == L::NATIVE_FIELD_CACHE_LAYOUT.slot_complement);
    const _: () = assert!(crate::vm::NATIVE_FIELD_CACHE_LAYOUT.own_holder == L::NATIVE_FIELD_CACHE_LAYOUT.own_holder);

}

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
    /// `Index` and `Store` of a typed array of the planned `TypedArrayKind` (its discriminant) the loop's entry viewed:
    /// the array's entry words are at the record offset in hole 1 (a read) or hole 0 (a store). The entry proved the
    /// kind (`LoopTier::select`), so the row, one per kind, tests none.
    TypedIndex(u8),
    TypedStore(u8),
    /// A typed array's `length`: the count its entry words hold (offset in hole 1), an int32.
    TypedLength,
    /// The field value the loop entry recorded: its byte offset in the record at hole 1.
    Field,
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
    /// `String.prototype.charCodeAt` of the flat string at hole 1, at the int32 index at hole 2 (`CHAR_CODE_AT_ROW`): the call
    /// the method read's native folds into (`base_region::function::native_call`).
    CharCodeAt,
    /// `parseInt` of the flat string at hole 1 in radix 10 (`PARSE_INT_ROW`).
    ParseInt,
    /// Stores the value operand (hole 1) into the own data field the site's cache entry (hole 2) names
    /// of the object register whose frame offset is hole 0, where the store gate admits it.
    SiteStore,
    /// `SiteStore` into the activation's receiver: hole 0 is unused.
    ThisStore,
    /// The inline-cache read (`ic_rows`) of the own slot of the object register (hole 1), whose
    /// exact shape (hole 2) and slot byte offset (hole 3) the code holds from the site's one entry,
    /// into the destination (hole 0): a shape compare and a load, no cache table read (`staged::ic::own_field`).
    IcGet,
    /// The inline-cache store of a Number (hole 1) into that slot of the object register (hole 0),
    /// the atom, shape and slot byte offset in holes 2 to 4, where the slot's observed type holds
    /// Number (`int`: only int32, which takes only int32 words), so no observation widens.
    IcSet { int: bool },
    /// `IcGet` and `IcSet` of a receiver every path proved a direct object (`base_region::proofs`): one compare
    /// of its auxiliary word with the entry's shape proves the shape and flat plain values (holes as theirs).
    IcKnownGet,
    IcKnownSet { int: bool },
    /// Leaves through exit 0 unless the object register at hole 0 is a direct object: what an access's miss
    /// runs after its site row, so the receiver is proved one on every edge out of the access.
    IcProof,
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
    /// The register at hole 0 defined as that slot's value, over a dead zone or any value but a shared slot (`InitializeTdz`
    /// passes the hole), else the row leaves into the helper step; the slot is hole 1 and its byte offset hole 2.
    /// It continues at TAKEN.
    EnvDefine(u16),
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
    /// The entry test of a declared parameter (`base_region::declared`): the register at hole 0 holds a word whose
    /// tag-decided languages meet the declared mask at hole 1, else the entry leaves with nothing written.
    Declared,
    /// The entry test of a captured declaration a body calls by its identity (`base_region::link::entry_rows`): the
    /// depth-0 capture at the slot of hole 0 holds the word of hole 1, else the entry leaves with nothing written.
    CaptureIdentity,
    /// The entry test of a class-typed parameter (`base_region::link::entry_rows`): the register at hole 0 holds a direct
    /// object whose exact shape and prototype link are the pair in hole 1 (`entry::shape_link_matches`), else the entry
    /// leaves with nothing written.
    Prototype,
    /// A prototype-held field folded to the value its holder held when the code was compiled (hole 2):
    /// the object register whose frame offset sits above the destination's in hole 0 is a direct object
    /// of the exact shape in hole 1's low half and the prototype link in its high half, else the row
    /// leaves. A Stable fact holds the value (`PrototypeProof::Linked`, `entry::held_constant`).
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
    /// The word of the global lexical binding at the address in hole 1 into the destination where it is initialized
    /// (`staged::lexical_read`), else the row leaves, for the interpreter to raise the dead zone's `ReferenceError`: a Stable
    /// fact holds the address (the global layout).
    LexicalRead,
    /// The first operand stored to the initialized global lexical binding at the address in hole 2, which no code holds the value of.
    LexicalWrite,
    /// The tagged word of hole 1 into the destination: an immutable global, a folded `typeof` name, and
    /// the `undefined` receiver of a name call.
    Word,
    /// An inlined callee's depth-0 capture: the ordinary value of the slot at the address in hole 1 (the
    /// guarded closure's environment is fixed) into the destination, else the row leaves inside the callee.
    CalleeCapture,
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
    /// A staged row of the `library::STAGED_FIRST..=STAGED_LAST` run (`staged_rows.rs`), by its code: a fast arm over the
    /// register operands the planner names (`steps::fast_arm`), its destination a frame slot, which a miss leaves for the
    /// helper step after the row. A hit continues at TAKEN.
    Staged(u8),
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
        self.slot_code().or_else(|| self.element_code()).or_else(|| self.object_code()).or_else(|| self.test_code()).or_else(|| self.global_code()).or_else(|| self.staged_code()).unwrap_or_else(|| self.other_code())
    }

    /// The code of a staged row, which names its own (`library::STAGED_*`).
    fn staged_code(self) -> Option<u8> {
        match self {
            Heap::Staged(code) => Some(code),
            _ => None,
        }
    }

    /// The code of a row addressed by a frame, capture or environment slot.
    fn slot_code(self) -> Option<u8> {
        Some(match self {
            Heap::Capture(_) => library::CAPTURE_ROW,
            Heap::CaptureSet(_) => library::CAPTURE_SET_ROW,
            Heap::EnvLocal(_) => library::ENV_ROW,
            Heap::EnvStore(_) => library::ENV_SET_ROW,
            Heap::EnvDefine(_) => library::ENV_DEFINE_ROW,
            Heap::LoadLocal(_) => library::LOAD_LOCAL_ROW,
            Heap::StoreLocal(_) => library::STORE_LOCAL_ROW,
            Heap::LoadChecked(_) => library::LOAD_CHECKED_ROW,
            Heap::StoreChecked(_) => library::STORE_CHECKED_ROW,
            _ => return None,
        })
    }

    /// The code of a row that reads or writes an array's or typed array's elements.
    fn element_code(self) -> Option<u8> {
        Some(match self {
            Heap::Index => library::INDEX_ROW,
            Heap::Store => library::STORE_ROW,
            Heap::TypedIndex(kind) => library::TYPED_INDEX_ROW + kind,
            Heap::TypedStore(kind) => library::TYPED_STORE_ROW + kind,
            Heap::TypedLength => library::TYPED_LENGTH_ROW,
            _ => return None,
        })
    }

    /// The code of a row that reads or writes an object's fields or sites.
    fn object_code(self) -> Option<u8> {
        Some(match self {
            Heap::Field => library::FIELD_ROW,
            Heap::Site | Heap::SiteChain => library::SITE_ROW,
            Heap::ThisSite | Heap::ThisChain => library::THIS_SITE_ROW,
            Heap::SiteStore => library::SITE_STORE_ROW,
            Heap::ThisStore => library::THIS_STORE_ROW,
            Heap::IcGet => library::IC_GET_ROW,
            Heap::IcSet { int: false } => library::IC_SET_ROW,
            Heap::IcSet { int: true } => library::IC_SET_INT_ROW,
            Heap::IcKnownGet => library::IC_KNOWN_GET_ROW,
            Heap::IcKnownSet { int: false } => library::IC_KNOWN_SET_ROW,
            Heap::IcKnownSet { int: true } => library::IC_KNOWN_SET_INT_ROW,
            Heap::IcProof => library::IC_PROOF_ROW,
            Heap::This => library::THIS_ROW,
            Heap::Held | Heap::ThisHeld => library::HELD_ROW,
            _ => return None,
        })
    }

    /// The code of a row that tests a value: its identity, type or prototype chain.
    fn test_code(self) -> Option<u8> {
        Some(match self {
            Heap::Identity => library::IDENTITY_ROW,
            Heap::Declared => library::DECLARED_ROW,
            Heap::CaptureIdentity => library::CAPTURE_IDENTITY_ROW,
            Heap::Prototype => library::PROTOTYPE_ROW,
            Heap::TypeofIndex => library::TYPEOF_INDEX_ROW,
            Heap::TypeofString => library::TYPEOF_STRING_ROW,
            Heap::Instanceof => library::INSTANCEOF_ROW,
            Heap::TypeofIs { negated: false, .. } => library::TYPEOF_IS_ROW,
            Heap::TypeofIs { negated: true, .. } => library::TYPEOF_ISNOT_ROW,
            Heap::Live => library::LIVE_ROW,
            Heap::Object => library::OBJECT_ROW,
            Heap::Key => library::KEY_ROW,
            _ => return None,
        })
    }

    /// The code of a row that reads or writes the realm's globals.
    fn global_code(self) -> Option<u8> {
        Some(match self {
            Heap::Realm => library::REALM_ROW,
            Heap::GlobalRead => library::GLOBAL_READ_ROW,
            Heap::GlobalStore => library::GLOBAL_STORE_ROW,
            Heap::LexicalRead => library::LEXICAL_READ_ROW,
            Heap::CalleeCapture => library::CALLEE_CAPTURE_ROW,
            Heap::LexicalWrite => library::LEXICAL_WRITE_ROW,
            _ => return None,
        })
    }

    /// The code of the remaining rows: arrays, constants, names and adds.
    fn other_code(self) -> u8 {
        match self {
            Heap::Length => library::LENGTH_ROW,
            Heap::CharCodeAt => library::CHAR_CODE_AT_ROW,
            Heap::ParseInt => library::PARSE_INT_ROW,
            Heap::Word => library::CONSTANT_ROW,
            Heap::Append => library::APPEND_ROW,
            Heap::ForIn => library::FOR_IN_ROW,
            Heap::Name { form } => library::NAME_ROWS[usize::from(form == Form::Arena)],
            Heap::Add { form, this } => library::ADD_ROWS[usize::from(this) * FORMS.len() + FORMS.iter().position(|known| *known == form).unwrap_or_default()],
            _ => unreachable!("every other row has a code of an earlier group"),
        }
    }

    /// Whether a hit continues at the row's TAKEN exit, not its NEXT: the rows of one convention with the fast arms, whose
    /// hit hops past the step laid out after them (`base_region::steps::plan_fast`).
    pub(super) fn continues_at_taken(self) -> bool {
        matches!(self, Heap::StoreLocal(_) | Heap::Capture(_) | Heap::CaptureSet(_) | Heap::EnvLocal(_) | Heap::EnvStore(_) | Heap::EnvDefine(_))
    }

    /// Whether the row reads a word of its operand's frame slot (a fast arm that tests the word),
    /// which a register held in a pinned register as a Number does not have there.
    pub(super) fn reads_frame_word(self) -> bool {
        matches!(self, Heap::Object | Heap::Key | Heap::ForIn | Heap::Declared | Heap::Prototype | Heap::TypeofIndex | Heap::TypeofString | Heap::Instanceof | Heap::TypeofIs { .. })
    }

    /// Whether the row's result is a tagged word whatever the registers around it hold: a value
    /// the activation holds, not a Number the region proved.
    /// Whether the row's result is a Number a double holds (a typed element), whatever the site's feedback.
    pub(super) fn yields_double(self) -> bool {
        matches!(self, Heap::TypedIndex(_))
    }

    pub(super) fn yields_tagged(self) -> bool {
        matches!(self, Heap::This | Heap::LoadLocal(_) | Heap::Word | Heap::CharCodeAt | Heap::ParseInt)
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

#[cfg(all(test, target_arch = "x86_64"))]
mod tests {

    /// The numbers the stencil rows are compiled with (`quench-bjit/stencils/layout.rs`) are
    /// the runtime's own.
    use super::super::CallContext;
    use crate::vm::wtf16::native_string as form;
    #[allow(dead_code)]
    mod stencil_layout {
        include!("../../../../quench-bjit/stencils/layout.rs");
    }
    mod stencil_units {
        include!("../../../../quench-bjit/stencils/units.rs");
    }


    /// Every payload field a stencil row reads, as (name, the runtime's offset, the stencil layout's).
    fn payload_offsets() -> Vec<(&'static str, usize, usize)> {
        use stencil_layout as m;
        use std::mem::offset_of as at;
        let own = vec![
            ("adapter_payload", at!(CallContext, adapter_payload), m::CALL_CONTEXT_ADAPTER_PAYLOAD),
            ("property_buffer_base", crate::value_vec::NATIVE_PROPERTY_BUFFER_LAYOUT.base, m::PROPERTY_BUFFER_BASE),
            ("activation_kind", crate::vm::dispatch_fast::KIND_OFFSET, m::ACTIVATION_KIND_OFFSET),
            ("cell_entry", at!(super::super::entry_cell::EntryCell, entry), m::CELL_ENTRY_AT),
            ("cell_recovery", at!(super::super::entry_cell::EntryCell, recovery), m::CELL_RECOVERY_AT),
            ("cell_body", at!(super::super::entry_cell::EntryCell, body), m::CELL_BODY_AT),
            ("cell_counters", at!(super::super::entry_cell::EntryCell, counters), m::CELL_COUNTERS_AT),
            ("raised_run", super::super::entry_cell::RAISED_RUN as usize, m::RAISED_RUN as usize),
            ("frame_bytes", super::super::FRAME_BYTES, m::ACTIVATION_BYTES),
            ("layout_table_base", crate::heap::layouts::table_base().addr(), m::LAYOUT_TABLE_BASE as usize),
            ("short_units", form::SHORT_UNITS_OFFSET as usize, m::string_form::SHORT_UNITS_OFFSET as usize),
            ("shared_pointer", form::SHARED_POINTER_OFFSET as usize, m::string_form::SHARED_POINTER_OFFSET as usize),
            ("shared_units", form::shared_units_offset() as usize, m::string_form::shared_units_offset() as usize),
        ];
        own
    }

    /// The value and narrow-value words the stencils are compiled with are the runtime's own.
    fn value_words_match() {
        use crate::value::{NarrowLayout, ValueLayout};
        use stencil_layout as mirror;
        use mirror::words as w;
        assert_eq!(
            [ValueLayout::TAG_MASK, ValueLayout::INT_TAG, ValueLayout::INTERNAL_TAG, ValueLayout::HEAP_TAG, ValueLayout::DIRECT_ARRAY_TAG, ValueLayout::DIRECT_OBJECT_TAG, ValueLayout::DIRECT_FUNCTION_TAG, ValueLayout::DIRECT_PROMISE_TAG, ValueLayout::DIRECT_ITERATOR_TAG, ValueLayout::HEAP_FAMILY_MASK, ValueLayout::DIRECT_STRING_TAG, u64::from(ValueLayout::TAG_SHIFT), ValueLayout::FIRST_TAG_FIELD, ValueLayout::TAG_FIELD_SPAN, ValueLayout::CANONICAL_NAN, crate::Value::NULL.to_bits(), crate::Value::UNDEFINED.to_bits(), crate::Value::DELETED.to_bits(), crate::Value::FALSE.to_bits(), crate::Value::TRUE.to_bits()],
            [w::TAG_MASK, w::INT_TAG, w::INTERNAL_TAG, w::HEAP_TAG, w::DIRECT_ARRAY_TAG, w::DIRECT_OBJECT_TAG, w::DIRECT_FUNCTION_TAG, w::DIRECT_PROMISE_TAG, w::DIRECT_ITERATOR_TAG, w::HEAP_FAMILY_MASK, w::DIRECT_STRING_TAG, u64::from(w::TAG_SHIFT), w::FIRST_TAG_FIELD, w::TAG_FIELD_SPAN, w::CANONICAL_NAN, w::NULL, w::UNDEFINED, w::DELETED, w::FALSE, w::TRUE]
        );
        assert_eq!(
            [crate::value::LOWER_WORD, crate::value::HEAP_FAMILY_BITS, u64::from(crate::value::HEAP_FAMILY_SHIFT), crate::value::NARROW_HEAP_FAMILIES as u64, crate::value::TAG_UNDEFINED, crate::value::TAG_NULL, crate::value::TAG_BOOL, crate::value::TAG_DELETED],
            [w::LOWER_WORD, w::HEAP_FAMILY_BITS, u64::from(w::HEAP_FAMILY_SHIFT), w::NARROW_HEAP_FAMILIES as u64, w::UNDEFINED, w::NULL, w::FALSE, w::DELETED]
        );
        assert_eq!(
            [u64::from(NarrowLayout::INTEGER_FLAG), u64::from(NarrowLayout::INTEGER_BITS), u64::from(NarrowLayout::CODE_SHIFT), u64::from(NarrowLayout::CODE_MASK), u64::from(NarrowLayout::INDEX_SHIFT), u64::from(NarrowLayout::INDEX_BITS), NarrowLayout::DIRECT_OBJECT_UPPER, u64::from(NarrowLayout::DIRECT_OBJECT_CODE)],
            [u64::from(w::NARROW_INT_FLAG), u64::from(w::NARROW_INTEGER_BITS), u64::from(w::NARROW_CODE_SHIFT), u64::from(w::NARROW_CODE_MASK), u64::from(w::NARROW_INDEX_SHIFT), u64::from(w::NARROW_INDEX_BITS), w::DIRECT_OBJECT_UPPER, u64::from(w::DIRECT_OBJECT_NARROW_CODE)]
        );
    }

    /// The typed-array kinds the stencils name are the runtime's, in its order, with its widths.
    fn typed_kinds_match() {
        use stencil_layout::TypedArrayKind as Mirror;
        let mirrored = Mirror::ALL;
        assert_eq!(usize::from(stencil_layout::LOOP_ENTRY_ARRAY_COUNT_AT as u8), std::mem::size_of::<u64>(), "the count follows the elements' address");
        let native_kinds: Vec<u8> = (0..quench_bjit::numeric::library::TYPED_KINDS).filter(|kind| !quench_bjit::numeric::library::NON_NATIVE_KINDS.contains(kind)).collect();
        let runtime_native: Vec<u8> = crate::heap::TYPED_ARRAYS.iter().filter(|row| crate::staged::typed_elements::NativeKind::new(row.kind).is_some()).map(|row| row.kind as u8).collect();
        assert_eq!(native_kinds, runtime_native);
        assert_eq!(crate::heap::TYPED_ARRAYS.len(), mirrored.len());
        for (row, mirror) in crate::heap::TYPED_ARRAYS.iter().zip(mirrored) {
            assert_eq!((row.kind as usize, row.width), (mirror as usize, mirror.width()), "{}", row.name);
        }
    }

    /// The conversions the stencils make of a binary64's bits are the runtime's own of its value.
    #[test]
    fn stencil_units_match() {
        let samples = [
            0.0, -0.0, 0.5, -0.5, 1.0, 1.5, -1.5, 2.5, 254.5, 255.0, 255.5, 256.0, 0.499_999_999_999_999_94, 0.500_000_000_000_000_1,
            2_147_483_648.0, -2_147_483_649.0, 4_294_967_295.0, 4_294_967_296.5, 1e10, -1e10, 9_007_199_254_740_993.0, 1e19, 1e300,
            f64::MAX, f64::MIN, f64::MIN_POSITIVE, f64::INFINITY, f64::NEG_INFINITY, f64::NAN,
        ];
        for value in samples {
            assert_eq!(stencil_units::to_int32(value), crate::units::to_int32(value), "ToInt32 of {value}");
            let clamped = if value.is_nan() { 0 } else { value.clamp(f64::from(u8::MIN), f64::from(u8::MAX)).round_ties_even() as u64 };
            assert_eq!(stencil_units::clamp_u8(value.to_bits()), clamped, "clamp of {value}");
        }
    }

    #[test]
    fn stencil_layout_matches() {
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
            [crate::heap::NARROW_LEN as u64, crate::heap::DOUBLE_LEN as u64, crate::heap::ENCODED_LEN as u64, crate::heap::OTHERS_UNKNOWN as u64, crate::heap::LENGTH_FROM_VALUES as i64 as u64, crate::heap::OTHERS_FROM_VALUES as i64 as u64, crate::value_vec::INLINE_PROPERTY_COUNT as u64, u64::from(crate::heap::SMALL_STRING_ID_MASK), u64::from(crate::heap::SMALL_STRING_ID_BASE)],
            [mirror::NARROW_LEN as u64, mirror::DOUBLE_LEN as u64, mirror::ENCODED_LEN as u64, mirror::OTHERS_UNKNOWN as u64, mirror::LENGTH_FROM_VALUES as i64 as u64, mirror::OTHERS_FROM_VALUES as i64 as u64, mirror::INLINE_PROPERTIES as u64, u64::from(mirror::SMALL_STRING_ID_MASK), u64::from(mirror::SMALL_STRING_ID_BASE)]
        );
        value_words_match();
        typed_kinds_match();
        assert_eq!(super::super::NATIVE_JS_STOPPED, mirror::call_record::NATIVE_JS_STOPPED);
        for (name, runtime, copy) in payload_offsets() {
            assert_eq!(runtime, copy, "{name}: the runtime's offset is the stencil layout's");
        }
        assert_eq!(
            [form::RECORD_SIZE as i32, form::SHORT_LENGTH_OFFSET, form::SHARED_LENGTH_OFFSET],
            [mirror::string_form::RECORD_SIZE as i32, mirror::string_form::SHORT_LENGTH_OFFSET, mirror::string_form::SHARED_LENGTH_OFFSET]
        );
    }
}
