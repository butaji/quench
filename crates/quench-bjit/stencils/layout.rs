//! The stencils' layout numbers. The values live in `quench-runtime/src/staged/layout_offsets.rs`,
//! which both builds include (`#[path]`), so a stencil and the runtime read one set of literals; the
//! runtime checks them against its own `offset_of!` values in `vm/compiled/base_rows.rs`.
//! The dispatch payload's head (`JS_STEP_*`) is `offset_of!` of one `#[repr(C)]` struct,
//! `staged/js_step_head.rs`, which the runtime embeds in `JsStepDispatch`. The two words shared
//! with the host (`call_record`, `shared_words`) are mounted here.

#[path = "../../quench-runtime/src/staged/js_step_head.rs"]
pub(crate) mod js_step_head;

#[path = "../../quench-runtime/src/staged/record_heads.rs"]
pub(crate) mod record_heads;

#[path = "../../quench-runtime/src/staged/encodings.rs"]
pub(crate) mod encodings;

#[path = "../../quench-runtime/src/staged/layout_offsets.rs"]
mod offsets;
pub(crate) use offsets::*;

/// The lane-call protocol, shared with the host (`quench_bjit::call_record`).
#[path = "../src/call_record.rs"]
pub(crate) mod call_record;
/// The words shared with the host (`quench_bjit::shared_words`).
#[path = "../src/shared_words.rs"]
pub(crate) mod shared_words;
pub(crate) const JS_STEP_LOOP_ENTRY: usize = 0;
pub(crate) const JS_STEP_CAPTURE_VALUES: usize = 80;
pub(crate) const JS_STEP_CAPTURE_LEN: usize = 88;
pub(crate) const JS_STEP_ARRAY_RECORD_BASE: usize = 104;
/// Whether the capture slots admit every direct store, which a store row requires
/// (`environment::dense_capture_store`); the bit of a body's capture word that says so
/// (`dispatch_fast::CAPTURE_STORES`) moves here at its entry.
pub(crate) const JS_STEP_CAPTURE_STORES: usize = 96;
/// The storage word of the environment window the running activation holds its environment-owned locals in
/// (`JsStepDispatch::environment_storage`), which the environment rows read their slots through
/// (`staged::environment::environment_slot`).
pub(crate) const JS_STEP_ENV_STORAGE: usize = 440;
/// The running activation's receiver (`this`) word, `DELETED` where it has none: the entry of a
/// lane body or loop region publishes the one it is entered with, for its run.
pub(crate) const JS_STEP_RECEIVER: usize = 192;
pub(crate) const CAPTURE_STORES_BIT: u64 = 1;
/// The activation's local slots and their count (`native_locals`, `local_count`).
pub(crate) const JS_STEP_NATIVE_LOCALS: usize = 56;
pub(crate) const JS_STEP_LOCAL_COUNT: usize = 64;
/// The heap bases a field read through a site's cache entry reads: the object record
/// reservation, the allocator header of the property values (whose base is at
/// `PROPERTY_BUFFER_BASE`) and the field cache entries of the running program.
pub(crate) const JS_STEP_OBJECT_RECORD_BASE: usize = 112;
pub(crate) const JS_STEP_PROPERTY_BUFFER: usize = 144;
/// What a store's barrier reads (`Stage::store_gate`): the object records' mark blocks and their
/// count, and whether the property arena records its slots' value classes (`slots_tracked`), which
/// an unobserved store requires clear.
pub(crate) const JS_STEP_OBJECT_MARK_BLOCKS: usize = 128;
pub(crate) const JS_STEP_OBJECT_MARK_BLOCK_COUNT: usize = 136;
pub(crate) const JS_STEP_SLOTS_TRACKED: usize = 184;
/// The heap's collection epoch as of the last refresh (`JsStepDispatch::epoch`), which a for-in step
/// compares to the epoch its state's snapshot was listed in (`staged::iteration::for_in_next`).
pub(crate) const JS_STEP_EPOCH: usize = 336;
/// The string record range's base, which a string `length` read finds the record from.
pub(crate) const JS_STEP_STRING_RECORD_BASE: usize = 376;
/// The table of one-unit ASCII strings `charAt` answers from (`JsStepDispatch::ascii_unit_strings`), zero before the
/// first exists; it follows the string record base in the dispatch payload.
pub(crate) const JS_STEP_ASCII_UNIT_STRINGS: usize = 384;
/// What a store into a function's own properties reads (`RecordSpace::Function`): the function
/// records' mark blocks and their count, as the object records' (`JS_STEP_OBJECT_MARK_BLOCKS`); and the
/// empty string's word, which a function's inferred name replaces (`staged::naming`).
pub(crate) const JS_STEP_FUNCTION_MARK_BLOCKS: usize = 416;
pub(crate) const JS_STEP_FUNCTION_MARK_BLOCK_COUNT: usize = 424;
pub(crate) const JS_STEP_EMPTY_STRING: usize = 432;
/// The function record reservation's base (`JsStepDispatch::function_record_base`).
pub(crate) const JS_STEP_FUNCTION_RECORD_BASE: usize = 120;
pub(crate) const JS_STEP_FIELD_ENTRIES: usize = 152;
pub(crate) const PROPERTY_BUFFER_BASE: usize = 0;
/// Bits of a body's capture word that hold the dense slots' address; the slot count sits above
/// them (`entry_cell::CAPTURE_ADDRESS_BITS`), and the pad is what a shift pair clears them with.
pub(crate) const CAPTURE_ADDRESS_PAD: u32 = u64::BITS - shared_words::CAPTURE_ADDRESS_BITS;
/// The record a completed body names (`LANE_BODY_RETURNED`).
pub(crate) const JS_STEP_RETURNED_PC: usize = 248;
/// More `JsStepDispatch` fields the call rows read: the lane body's VM, residual and lane
/// context (a helper call's first three arguments), the lane stack's free span and the shadow
/// frames' span. Checked against the runtime by `stencil_layout_matches`.
pub(crate) const JS_STEP_VM: usize = 200;
pub(crate) const JS_STEP_RESIDUAL: usize = 208;
pub(crate) const JS_STEP_LANE_CONTEXT: usize = 240;
/// The lane stack's words (`Vm::stack_words_in_lane`): the addresses of the values' and the activations' tops,
/// the end of the values' room and the end of the frames' room.
pub(crate) const JS_STEP_LANE_VALUES_TOP: usize = 288;
pub(crate) const JS_STEP_LANE_VALUES_END: usize = 296;
pub(crate) const JS_STEP_LANE_ACTIVATIONS_TOP: usize = 304;
pub(crate) const JS_STEP_LANE_FRAMES_END: usize = 312;
/// An activation's size, and where its kind tag lies; `KIND_NATIVE` marks a frame a direct call wrote.
pub(crate) const ACTIVATION_BYTES: usize = crate::staged::frame::ACTIVATION_BYTES;
pub(crate) const ACTIVATION_KIND_OFFSET: usize = crate::staged::frame::ACTIVATION_KIND_OFFSET;
/// The entry cell's run counters, and the unit a raised run adds (`entry_cell::RAISED_RUN`).
pub(crate) const CELL_COUNTERS_AT: usize = crate::staged::call::ENTRY_CELL.counters_offset as usize;
pub(crate) const RAISED_RUN: u64 = 1 << 32;

/// The context words below every lane window's register 0 (the runtime's
/// `register_file::FRAME_CONTEXT`): a pushed window leaves them free past its registers for the
/// next. Checked against the runtime by `stencil_layout_matches`.
pub(crate) const FRAME_CONTEXT_WORDS: usize = crate::staged::frame::CONTEXT_WORDS;

/// The entry cell's entry, recovery and body words (`EntryCell`), and the recovery space's
/// values and activations (`LaneRecoverySpace`).
pub(crate) const CELL_ENTRY_AT: usize = crate::staged::call::ENTRY_CELL.entry_offset as usize;
pub(crate) const CELL_RECOVERY_AT: usize = crate::staged::call::ENTRY_CELL.recovery_offset as usize;
pub(crate) const CELL_BODY_AT: usize = crate::staged::call::ENTRY_CELL.body_offset as usize;
pub(crate) const CELL_ROWS_AT: usize = crate::staged::call::ENTRY_CELL.rows_offset as usize;
pub(crate) const RECOVERY_VALUES_AT: usize = 0;
pub(crate) const RECOVERY_ACTIVATIONS_AT: usize = 8;

/// The flat string forms a string record is read in (`vm::wtf16::native_string`): the record's
/// size (three words), where it keeps its form's tag, each form's tag, and where a short and a
/// shared form keep their unit count. Checked against the runtime by `stencil_layout_matches`.
pub(crate) mod string_form {
    pub(crate) const RECORD_SIZE: usize = 3 * core::mem::size_of::<usize>();
    /// A short record's length byte follows its tag byte.
    pub(crate) const SHORT_LENGTH_OFFSET: i32 = 1;
    /// A shared record's pointer follows its tag at the next word, then its length.
    pub(crate) const SHARED_LENGTH_OFFSET: i32 = 2 * core::mem::size_of::<usize>() as i32;
    /// The units of a short record follow its length byte.
    pub(crate) const SHORT_UNITS_OFFSET: u64 = 2;
    /// A shared record's pointer follows its tag at the next word.
    pub(crate) const SHARED_POINTER_OFFSET: i32 = core::mem::size_of::<usize>() as i32;
    /// The reference counts (strong and weak) an `Rc` allocation holds before its value.
    const RC_COUNTS: u64 = 2;
    /// How far into the shared allocation the units start: past the reference counts of an `Rc`.
    pub(crate) const fn shared_units_offset() -> u64 {
        RC_COUNTS * core::mem::size_of::<usize>() as u64
    }
}
/// What an inline-cache row reads of the dispatch payload (`stencils/ic_rows.rs`): the method-cache atom bits and their
/// word count, which a store row consults, and whether stores are specialized.
pub(crate) const JS_STEP_METHOD_CACHE_ATOMS: usize = 160;
pub(crate) const JS_STEP_METHOD_CACHE_ATOM_WORDS: usize = 168;
pub(crate) const JS_STEP_STORES_SPECIALIZED: usize = 176;
/// The object layout table's fixed address (`heap::layouts::table_base`) on x86-64 Linux, the
/// stencils' target: a row reads an entry with the address as its displacement.
pub(crate) const LAYOUT_TABLE_BASE: u64 = 0x7000_0000;
