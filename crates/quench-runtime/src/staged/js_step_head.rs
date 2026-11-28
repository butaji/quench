// The head of the dispatch payload (`JsStepDispatch`) that generated code reads by field offset.
// ONE source: the runtime embeds it as the first field of `JsStepDispatch` (`head`), and the stencil
// crate includes this file (`stencils/layout.rs` mounts it) and takes its `offset_of!` values as the
// `JS_STEP_*` constants. Only word-sized fields, `#[repr(C)]`, so the offsets are the same in both
// builds. The pointee type of the two pointers is a parameter: the runtime uses `Value`, the
// stencils `u8`; a pointer is one word either way.
#![allow(dead_code)]

#[repr(C)]
pub struct JsStepHead<V> {
    /// The loop entry record whose counter a region's backward edges bump; first, as a `Crossing::Bare` payload is this word alone.
    pub loop_entry: usize,
    /// Stable base of this VM's direct-array record reservation. The field is
    /// refreshed after VM callouts because the reservation may be lazy.
    pub array_record_base: usize,
    /// The activation's local slots and their count (`native_locals`, `local_count`).
    pub native_locals: *mut V,
    pub local_count: usize,
    /// The class space's base (`Heap::class_space_base`): a direct object's cell is at this plus its
    /// granule times 8, and its header's high half is the record id (R4-B2b). Zero where absent; refreshed
    /// with the other record bases.
    pub class_base: usize,
    /// The function record reservation, whose records' own fields reads take
    /// (`ObjectHeader::prove_readable`); zero where absent.
    pub function_record_base: usize,
    /// Canonical allocator header in this entry's live VM. Its address is fixed
    /// through callouts; growth/compaction update its base/length in place.
    pub property_buffer: usize,
    pub field_entries: usize,
    /// The names method caches hold (`Vm::method_caches_may_name`), one bit each, and its
    /// length in words; and whether the VM and program run specialized, which a cached store
    /// requires (`Vm::cached_field_store_hit`).
    pub method_cache_atoms: usize,
    pub method_cache_atom_words: usize,
    pub stores_specialized: usize,
    /// What watches a store (`Heap::store_watch_word`): bit `STORE_WATCH_TRACKS` while the property
    /// arena records its slots' value classes, bit `STORE_WATCH_COUNTS` while old records keep
    /// reference counts. The store gate requires it zero; an immediate store, which no other test
    /// stops, requires its counts bit clear (`Stage::immediate_store_open`).
    pub store_watch: usize,
    /// A lane body's VM and running residual, which its semantic helpers take.
    pub vm: usize,
    pub residual: usize,
    /// A lane body's running lane context, which its native calls make activations in.
    pub lane_context: usize,
    /// The record a lane body's completion runs (`LANE_BODY_RETURNED`).
    pub returned_pc: usize,
    /// A lane run's lane stack, where a direct call places its callee's registers and its
    /// native frame (`DirectCallee`): the words that hold the values' and the activations'
    /// tops, which the VM's `LaneStack` owns (`Vm::lane_stack_words`), the end of the values'
    /// capacity, and the end of the frames the run nests (`SHADOW_FRAMES`).
    pub lane_values_top: usize,
    pub lane_values_end: usize,
    pub lane_activations_top: usize,
    pub lane_frames_end: usize,
    /// The heap's collection epoch as of the last refresh, which a for-in step compares to the
    /// epoch its state's snapshot was listed in (`staged::iteration::for_in_next`).
    pub epoch: u64,
    /// The string record reservation, whose records' flat units a string row reads in place
    /// (`staged::strings::flat_string`); zero where absent. A callout may reserve it, so it is
    /// read again after one (`refresh_object_bases`).
    pub string_record_base: usize,
    /// The function records' mark blocks and their count, which a store into a function's own
    /// properties reads as the object records' are (`RecordSpace::Function`).
    pub function_mark_blocks: usize,
    pub function_mark_block_count: usize,
    /// The empty string's word, which a function's inferred name replaces (`staged::naming`).
    pub empty_string: u64,
    /// The word of the storage the running lane body's environment-owned locals live in, which its
    /// environment rows read through (`staged::environment::environment_slot`).
    pub environment_storage: u64,
    /// The empty allocations' inputs (`heap_rows::empty_row`, `lane_region::frame_calls`): the region 1 class cell
    /// cursor (`Heap::native_cell_cursor(1)`), which an empty object's class cell is claimed from, and the empty
    /// object's captured class (its header word, which names its shape and prototype link) with the prototype it was
    /// captured under; then the same for the array space and the zero-length array. A class or template word of zero
    /// means none is captured, and a cursor of zero that the space has none; either sends the allocation to its helper.
    pub empty_object_cursor: usize,
    pub empty_object_class: u64,
    pub empty_object_proto: u64,
    pub array_cursor: usize,
    pub empty_array_layout: u64,
    pub empty_array_payload: u64,
    pub empty_array_proto: u64,
    /// The base of the heap's one-unit ASCII strings (`Heap::ascii_unit_strings_base`), which a
    /// `charAt` row indexes by unit (`staged::builtins::char_at_ascii`); zero where absent.
    pub ascii_unit_strings: usize,
    /// The address of the thread's armed array proofs word (`heap::array_proofs::armed_proofs_address`).
    /// Its word is nonzero while a lane run's proofs are armed, which a row that shrinks an array's
    /// backing must miss on: it cannot retire them. Read at every use, so never stale.
    pub armed_array_proofs: usize,
    /// The address of the thread's index-free chain link word (`heap::index_free_chain_link_address`):
    /// its word is the encoded link of `Array.prototype` while the held index-free chain proof covers
    /// its chain, else zero. Read at every use, so never stale.
    pub array_chain_index_free_address: usize,
    /// The address of the lane run's plain-frame depth word (`PlainFrames`, `frame::PLAIN_DEPTH_AT`); its
    /// registers word is `frame::PLAIN_REGISTERS_AT` past it. Emitted calls bump the depth and set the registers.
    /// (reconciled with u-jit F5 on merge)
    pub plain_depth: usize,
    /// The plain depth's bound: a call whose header would pass it leaves to the helper row.
    /// (reconciled with u-jit F5 on merge)
    pub plain_depth_limit: usize,
    /// The address of the word holding `frames_below_plain()` (records plus sealed headers): the body table
    /// is indexed by `*activation_count + plain_depth`, since a record push resets the plain depth to 0.
    pub activation_count: usize,
    /// The running body (`LaneMethod`) of each native frame, indexed by `*activation_count + plain_depth`, and the address of
    /// the table's top word. (reconciled with u-jit F5 on merge)
    pub body_table: usize,
    pub body_top: usize,
    /// The DirectCallee descriptor of each native frame, indexed as `body_table` (the u-jit half of F5).
    pub native_table: usize,
    /// The return of an emitted call whose native header Rust observed, spilled into a record or linked
    /// (`LaneStack::pop_observed_header`): `extern "C" fn(vm, residual) -> u64`, which the b-jit direct
    /// row calls where it finds no unlinked plain header to pop, as the u-jit's `pop_native` calls it.
    pub pop_observed_header: usize,
    /// The array records' mark blocks and their count, which a store of a reference into a direct
    /// array's elements reads as the object records' are (`RecordSpace::Array`, `Heap::store_logs`);
    /// refreshed with the other bases, as a callout may move the block list.
    pub array_mark_blocks: usize,
    pub array_mark_block_count: usize,
    /// The class cells' mark blocks and counts, as (blocks, count) pairs of two words by cell region: entry `r`
    /// is at byte `16 * r`, region 0 (the stubs, which keep no class mark) is zero. A store of a reference into a
    /// class cell reads its region's entry (`staged::barrier::class_unmarked`); refreshed with the other bases.
    pub class_mark_lists: [usize; 12],
    /// The class space's spill arena base (`heap::spill`): the address of its first word, which a spilled class field's
    /// run offset is added to (`staged::entry::class`). Refreshed with the other record bases; a move of the arena
    /// changes `native_layout_epoch`, so a reader takes it again.
    pub spill_base: usize,
    /// The typed arrays' view record directory and its page count (`Heap::typed_view_directory`),
    /// refreshed with the other record bases, and the address of the process's buffer-relocation count
    /// (`buffer_relocations_address`): what a typed array's view proof reads
    /// (`staged::typed_view::TypedRecords`), in a u-jit lowering and a b-jit row alike.
    pub typed_view_directory: usize,
    pub typed_view_pages: usize,
    pub buffer_relocations: usize,
    /// The Map and Set record reservation (`Heap::collection_record_base`), refreshed with the other
    /// record bases; zero where absent. A method read by name of a collection tests its record
    /// (`elements::plain_collection_link`).
    pub collection_record_base: usize,
}
