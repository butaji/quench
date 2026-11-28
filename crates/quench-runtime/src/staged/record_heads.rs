// The heads of the native records that generated code reads by field offset. ONE source: the runtime
// embeds each head as the first field of its record (`head`, with `Deref`/`DerefMut` forwarding so
// existing field accesses keep compiling), and the stencil crate mounts this file (`stencils/layout.rs`)
// and takes the `offset_of!` values as the `NATIVE_*_LAYOUT` constants of `staged/layout_offsets.rs`.
// Only word-sized fields, `#[repr(C)]` (packed where the record is), so the offsets are the same in both
// builds. Nothing here names a runtime item: the file must compile in the stencil crate.
#![allow(dead_code)]

use core::cell::Cell;
use core::sync::atomic::AtomicBool;

/// The words of an ordinary object's record (`heap::cell::Object`): its payload word and its layout id.
#[repr(C, packed(4))]
pub struct ObjectHead {
    /// One word: with `ObjectExtras`, `EXTRAS_TAG` and a pointer to them; without extras, the inline values as
    /// narrow values, or the storage position (`ValueVec`'s start), or an array's element backing.
    pub payload: u64,
    /// The object's layout (`heap::layouts`): the interned pair of its header words.
    pub layout: u32,
}

/// An ordinary object's record as generated code reads it (`heap::cell::OrdinaryRecord`, whose `object` is an
/// `ObjectHead`): its head, then the granule of its region 0 stub, which an empty allocation stores after the
/// head's words. Sixteen bytes with the stub's padding.
#[repr(C)]
pub struct OrdinaryRecordHead {
    pub object: ObjectHead,
    pub stub: u32,
}

/// The head of a Wasm GC object's record: the address of its payload bytes (`heap::cell::WasmGcFields`),
/// their length in bytes, and the type index the object was built from. Compiled code reads a field's
/// slot as `payload + offset`, bounded by `length`, so a field access takes no callout, box or barrier.
/// The record writes this head whenever it stores the payload; the payload stays owned by the record.
#[repr(C)]
pub struct WasmGcHead {
    /// The first byte of the payload, or 0 where the payload is not allocated.
    pub payload: u64,
    /// The payload's length in bytes.
    pub length: u64,
    /// The index of the object's type in its module's types.
    pub ty: u32,
}

/// A dense element backing's header (`heap::array_elements::Header`), the words before its values.
#[repr(C)]
pub struct ArrayHeaderHead {
    /// Owners of this backing; zero where no owner counts it.
    pub owners: u32,
    /// The number of values the backing holds.
    pub len: u32,
    /// Room for values after the header, with the allocator's flags in the bits it sets.
    pub capacity: u32,
    /// How many values below `len` are not int32s, holes included (`OTHERS_UNKNOWN` when uncounted).
    pub others: u32,
}

/// What native code reads of a typed array's view record (`heap::typed_view::NativeView`): the first
/// element's address, the publication stamp, the element count, the kind, and the three flags.
#[repr(C)]
#[derive(Debug)]
pub struct NativeViewHead {
    /// The first element's address.
    pub data: usize,
    /// The buffer's relocation count when `data` and `length` were published.
    pub stamp: u64,
    /// The element count, below 2^31.
    pub length: u32,
    /// The `TypedArrayKind` discriminant, or the no-kind marker.
    pub kind: u8,
    /// Set while the view cannot be addressed in place.
    pub detached: u8,
    /// Set when the buffer's bytes belong to this view's buffer alone.
    pub unshared: u8,
    /// The table entry that holds this record's address, null while it has none.
    pub home: *mut usize,
}

/// A function record (`heap::cell::FunctionRecord`, which names this with the runtime's own `Object`,
/// `FunctionKind` and `Value`). Its layout is the declaration's (`repr(C)`), so generated code reads the
/// object at `offset_of!(FunctionRecord, object)` and the record is `size_of` it. The stencils instantiate
/// the parameters with words of the same size and alignment (`layout_offsets::NATIVE_FUNCTION_LAYOUT`).
#[derive(Clone, Debug)]
#[repr(C)]
pub struct FunctionRecord<O, K, V> {
    /// The function's own ordinary object (its properties); first, so its words are at offset 0.
    pub object: O,
    /// Beside the object, whose 12 bytes it fills to the words after: after them, its 12
    /// bytes and two of padding made the record 48 bytes (jsdom-d3: 114K records at its peak).
    pub kind: K,
    /// The closure environment.
    pub env: V,
}

/// The ABI prefix of a compiled function's call context (`vm::crate::vm::engine_types::CallContext`): every field up
/// to and including `adapter_payload`, in order, which generated code loads by offset. The runtime embeds
/// it as the first field `head` of `CallContext`, with `Deref`/`DerefMut`. Its non-word fields are
/// parameters: `E` the environment value (`Value`), `C` the compiled artifact, `T` the table words' record,
/// `D` the reference call-site diagnostics, `P` the program id and `R` the raised completion (`Option<Raised>`).
/// The runtime instantiates them with its own types; `layout_offsets` instantiates them with words of the same
/// size and alignment, and `base_rows` checks each of those sizes against the runtime type.
#[repr(C)]
pub struct CallContextHead<E, C, T, D, P, R> {
    /// The lowest stack address compiled code may use before a call traps as exhausted.
    pub stack_limit: usize,
    /// The attention flag the code polls (`ExecutionInterrupt::attention_flag`).
    pub interrupt: *const AtomicBool,
    pub table: *const usize,
    pub vm: *mut (),
    pub env: E,
    /// The entry trampoline's stack pointer after it saved the caller's registers; a guard fault resumes there.
    pub entry_stack: usize,
    /// The guarded memory reservation compiled code may fault in.
    pub guard_start: usize,
    pub guard_end: usize,
    /// Per environment slot (the instance's global area), the address of the native slot of the
    /// global bound there, read and written raw; null for any other binding. Heap cells never
    /// move, so the addresses outlive callouts.
    pub globals: *const *mut u64,
    /// Per environment slot, the words that publish the table bound there (`TableWords`): the
    /// table's rows are read in place, its growth rewrites the words.
    pub table_words: *const *const T,
    /// `import_callout` for the VM's host type.
    pub callout: usize,
    /// VM helper for indirect calls, including table checks and invocation.
    pub call_reference: usize,
    /// The artifact containing the currently executing call site. The active
    /// entry (or its caller) keeps this artifact strongly owned for the callout.
    pub compiled: *const C,
    /// The program of the code running (the VM's; compiled code never reads it).
    pub program: P,
    /// Number of Wasm-to-host callouts made by this native invocation.
    pub import_callouts: *mut u64,
    /// Number of direct known calls that had to use the VM fallback.
    pub known_callouts: *mut u64,
    /// Number of Wasm indirect-reference callouts made by this native invocation.
    pub reference_callouts: *mut u64,
    /// The bound linear memory whose base and size this call uses.
    pub memory: E,
    /// Its exact current byte length, refreshed when a callout may grow it.
    pub memory_length: u64,
    /// What a callout that returned the raised status raised.
    pub raised: R,
    /// Diagnostic-only ABI fields are appended so existing repr(C) offsets
    /// used by generated code stay fixed.
    pub call_reference_diagnostic: usize,
    pub reference_call_sites: *const D,
    pub reference_call_site_count: usize,
    /// The VM's poll page (`ExecutionInterrupt::poll_page`), whose fault ends the call as
    /// terminated; empty when compiled code checks the flag instead.
    pub poll_start: usize,
    pub poll_end: usize,
    /// Diagnostic-only count of direct Emscripten same-memory copies.
    pub emscripten_memcpy_fast_path_hits: *mut u64,
    /// Per-import attribution, present only during opt-in diagnostic runs.
    pub import_callout_counts: *const Cell<u64>,
    pub import_callout_count: usize,
    /// A stack-owned adapter payload for JS and OSR entries that use this
    /// module's common native-entry trampoline without Wasm callouts.
    pub adapter_payload: *mut (),
    /// The SysV shim every native caller enters a lane body through (`lane_body_abi::body_shim`), which
    /// takes the body to call as its fifth argument; zero where the host has no native code.
    pub body_shim: usize,
}
