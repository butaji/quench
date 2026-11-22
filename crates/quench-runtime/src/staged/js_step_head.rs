// The head of the dispatch payload (`JsStepDispatch`) that generated code reads by field offset.
// ONE source: the runtime embeds it as the first field of `JsStepDispatch` (`head`), and the stencil
// crate includes this file (`stencils/layout.rs` mounts it) and takes its `offset_of!` values as the
// `JS_STEP_*` constants. Only word-sized fields, `#[repr(C)]`, so the offsets are the same in both
// builds. The pointee type of the two pointers is a parameter: the runtime uses `Value`, the
// stencils `u8`; a pointer is one word either way.
#![allow(dead_code)]

#[repr(C)]
pub(crate) struct JsStepHead<V> {
    /// The loop entry record whose counter a region's backward edges bump; first, as a `Crossing::Bare` payload is this word alone.
    pub(crate) loop_entry: usize,
    /// Stable base of this VM's direct-array record reservation. The field is
    /// refreshed after VM callouts because the reservation may be lazy.
    pub(crate) array_record_base: usize,
    /// The activation's local slots and their count (`native_locals`, `local_count`).
    pub(crate) native_locals: *mut V,
    pub(crate) local_count: usize,
    /// The heap facts generated field reads use (`emit_js_own_field_get`): the object
    /// record reservation, the property value arena and its length, and the field cache
    /// entries of `program_id`'s sites; zero where absent. A callout may grow or compact any
    /// of them, so each is read again after one (`refresh_heap_bases`).
    pub(crate) object_record_base: usize,
    /// The function record reservation, whose records' own fields reads take
    /// (`ObjectHeader::prove_readable`); zero where absent.
    pub(crate) function_record_base: usize,
    pub(crate) object_mark_blocks: usize,
    pub(crate) object_mark_block_count: usize,
    /// Canonical allocator header in this entry's live VM. Its address is fixed
    /// through callouts; growth/compaction update its base/length in place.
    pub(crate) property_buffer: usize,
    pub(crate) field_entries: usize,
    /// The names method caches hold (`Vm::method_caches_may_name`), one bit each, and its
    /// length in words; and whether the VM and program run specialized, which a cached store
    /// requires (`Vm::cached_field_store_hit`).
    pub(crate) method_cache_atoms: usize,
    pub(crate) method_cache_atom_words: usize,
    pub(crate) stores_specialized: usize,
    /// What watches a store (`Heap::store_watch_word`): bit `STORE_WATCH_TRACKS` while the property
    /// arena records its slots' value classes, bit `STORE_WATCH_COUNTS` while old records keep
    /// reference counts. The store gate requires it zero; an immediate store, which no other test
    /// stops, requires its counts bit clear (`Stage::immediate_store_open`).
    pub(crate) store_watch: usize,
    /// A lane body's VM and running residual, which its semantic helpers take.
    pub(crate) vm: usize,
    pub(crate) residual: usize,
    /// A lane body's running lane context, which its native calls make activations in.
    pub(crate) lane_context: usize,
    /// The record a lane body's completion runs (`LANE_BODY_RETURNED`).
    pub(crate) returned_pc: usize,
    /// A lane run's lane stack, where a direct call places its callee's registers and its
    /// native frame (`DirectCallee`): the words that hold the values' and the activations'
    /// tops, which the VM's `LaneStack` owns (`Vm::lane_stack_words`), the end of the values'
    /// capacity, and the end of the frames the run nests (`SHADOW_FRAMES`).
    pub(crate) lane_values_top: usize,
    pub(crate) lane_values_end: usize,
    pub(crate) lane_activations_top: usize,
    pub(crate) lane_frames_end: usize,
    /// The heap's collection epoch as of the last refresh, which a for-in step compares to the
    /// epoch its state's snapshot was listed in (`staged::iteration::for_in_next`).
    pub(crate) epoch: u64,
    /// The string record reservation, whose records' flat units a string row reads in place
    /// (`staged::strings::flat_string`); zero where absent. A callout may reserve it, so it is
    /// read again after one (`refresh_object_bases`).
    pub(crate) string_record_base: usize,
    /// The function records' mark blocks and their count, which a store into a function's own
    /// properties reads as the object records' are (`RecordSpace::Function`).
    pub(crate) function_mark_blocks: usize,
    pub(crate) function_mark_block_count: usize,
    /// The empty string's word, which a function's inferred name replaces (`staged::naming`).
    pub(crate) empty_string: u64,
    /// The word of the storage the running lane body's environment-owned locals live in, which its
    /// environment rows read through (`staged::environment::environment_slot`).
    pub(crate) environment_storage: u64,
    /// The empty allocations' inputs (`heap_rows::empty_row`, `lane_region::frame_calls`): the object space's
    /// record cursor and the empty object's captured template (layout, payload and the prototype it was
    /// captured under), then the same for the array space and the zero-length array. A template word of zero
    /// means none is captured, and a cursor of zero that the space has none; either sends the allocation to
    /// its helper.
    pub(crate) object_cursor: usize,
    pub(crate) empty_object_layout: u64,
    pub(crate) empty_object_payload: u64,
    pub(crate) empty_object_proto: u64,
    pub(crate) array_cursor: usize,
    pub(crate) empty_array_layout: u64,
    pub(crate) empty_array_payload: u64,
    pub(crate) empty_array_proto: u64,
    /// The base of the heap's one-unit ASCII strings (`Heap::ascii_unit_strings_base`), which a
    /// `charAt` row indexes by unit (`staged::builtins::char_at_ascii`); zero where absent.
    pub(crate) ascii_unit_strings: usize,
    /// The address of the thread's armed array proofs word (`heap::array_proofs::armed_proofs_address`).
    /// Its word is nonzero while a lane run's proofs are armed, which a row that shrinks an array's
    /// backing must miss on: it cannot retire them. Read at every use, so never stale.
    pub(crate) armed_array_proofs: usize,
    /// The address of the thread's index-free chain link word (`heap::index_free_chain_link_address`):
    /// its word is the encoded link of `Array.prototype` while the held index-free chain proof covers
    /// its chain, else zero. Read at every use, so never stale.
    pub(crate) array_chain_index_free_address: usize,
    /// The address of the lane run's plain-frame depth word (`PlainFrames`, `frame::PLAIN_DEPTH_AT`); its
    /// registers word is `frame::PLAIN_REGISTERS_AT` past it. Emitted calls bump the depth and set the registers.
    /// (reconciled with u-jit F5 on merge)
    pub(crate) plain_depth: usize,
    /// The plain depth's bound: a call whose header would pass it leaves to the helper row.
    /// (reconciled with u-jit F5 on merge)
    pub(crate) plain_depth_limit: usize,
    /// The address of the word holding `frames_below_plain()` (records plus sealed headers): the body table
    /// is indexed by `*activation_count + plain_depth`, since a record push resets the plain depth to 0.
    pub(crate) activation_count: usize,
    /// The running body (`LaneMethod`) of each native frame, indexed by `*activation_count + plain_depth`, and the address of
    /// the table's top word. (reconciled with u-jit F5 on merge)
    pub(crate) body_table: usize,
    pub(crate) body_top: usize,
    /// The DirectCallee descriptor of each native frame, indexed as `body_table` (the u-jit half of F5).
    pub(crate) native_table: usize,
    /// The return of an emitted call whose native header Rust observed, spilled into a record or linked
    /// (`LaneStack::pop_observed_header`): `extern "C" fn(vm, residual) -> u64`, which the b-jit direct
    /// row calls where it finds no unlinked plain header to pop, as the u-jit's `pop_native` calls it.
    pub(crate) pop_observed_header: usize,
    /// The array records' mark blocks and their count, which a store of a reference into a direct
    /// array's elements reads as the object records' are (`RecordSpace::Array`, `Heap::store_logs`);
    /// refreshed with the other bases, as a callout may move the block list.
    pub(crate) array_mark_blocks: usize,
    pub(crate) array_mark_block_count: usize,
}
