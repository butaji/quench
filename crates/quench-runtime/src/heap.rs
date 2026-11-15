use crate::value::{HeapReference, Value};
use crate::vm::wtf16::JsString;
use crate::value_vec::{INLINE_PROPERTY_COUNT, ValueArena, ValueVec};
use crate::staged::wasm_instance::Storage as WasmStorage;
use rayon::prelude::*;
use rayon::ThreadPool;
use rustc_hash::FxHashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use rc_drop::{StoppedCellDrop, cell_rc_drop_lock};
mod access;
mod array_elements;
pub(crate) mod layouts;
mod dense_growth;
pub(crate) use dense_growth::dense_write_gap_limit;
mod store_gate;
mod typed_view;
pub(crate) use typed_view::{NATIVE_TYPED_ARRAY_LAYOUT, NativeView};
use typed_view::TypedViewTable;
pub(crate) use array_elements::{ArrayElements, HEADER_VALUES as ARRAY_HEADER_VALUES, DOUBLE_LEN, ENCODED_LEN, LENGTH_FROM_VALUES, NARROW_LEN, OTHERS_FROM_VALUES, OTHERS_UNKNOWN, UniqueElements};
mod array_proofs;
pub(crate) use array_proofs::{ArrayStorageProofs, LaneProofScope, retire_array_storage_proofs};
mod cell;
mod dependencies;
pub(crate) use dependencies::{
    Dependencies, INHERITED_FIELD_CACHE_MAX_DEPTH, IndexFreeChainProof, InheritedFieldAnswer, InheritedFieldCache,
    NO_PROVED_LAYOUT, ProofKind,
};
mod cell_access;
mod buffer_bytes;
mod collection_entries;
mod concurrent_mark;
mod concurrent_view;
mod environment_trace;
pub(crate) use environment_trace::{ClosureReadSource, EnvironmentReads, EnvironmentTrace};
mod external;
mod float_slots;
#[cfg(test)] mod float_slot_tests;
mod inline_storage;
use inline_storage::widen_inline_storage;
mod memory_stats;
pub use memory_stats::HeapMemoryComposition;
#[cfg(feature = "profile-memory")]
mod memory_profile;
mod pacing;
mod parallel_mark; mod planned;
mod promise_record;
pub(crate) use promise_record::{AbandonedHostContexts, PromiseCell, PromiseReaction, PromiseRecord, PromiseRejectionPhase, PromiseRejectionState, PromiseState, StoredHostContext};
mod rc_drop;
mod rc;
mod records;
mod root;
mod slots;
mod snapshot;
mod string_alloc;
mod string_atoms;
mod native_layout;
mod record_layouts;
pub(crate) use record_layouts::{
    NATIVE_ARRAY_LAYOUT, NATIVE_ENVIRONMENT_LAYOUT, NATIVE_FUNCTION_LAYOUT, NATIVE_OBJECT_LAYOUT, PropertyStoreLayout,
    absent_environment_storage,
};
mod string_parts;
mod small_strings;
pub(crate) use small_strings::{FlatText, SMALL_STRING_ID_BASE, SMALL_STRING_ID_MASK, SmallParts, is_small_string};
mod thin_slice;
pub(crate) use thin_slice::ThinBoxSlice;
mod weak;
pub(crate) use cell::*;
pub(crate) use buffer_bytes::{BufferBytes, BufferRead, buffer_relocations_address, view_epoch};
pub(crate) use collection_entries::{CollectionEntries, CollectionEntry, Entries, WeakMapEntries, WeakSetEntries, number_key_hash};
use external::array_payload_bytes;
use records::{
    ArrayRecordSpace, AwaitRecordSpace, FunctionRecordSpace, IteratorRecordSpace, V128RecordSpace,
    ObjectRecordSpace, PromiseRecordSpace, RecordFamily, StringRecordSpace, RecordId, RecordSweep,
};
use records::RECORD_SPACE_CAPACITY;
pub use root::RootId;
pub(crate) use root::{RootTable, WeakHandle};
use slots::{FREE_SLOT_WORDS, FreeSlotBitmap, SLOTS_PER_SLAB, SlotArena};
mod young;
use young::YoungSlots;

// Keep implicit holes in dense storage when their one-time allocation is
// bounded. 1 MiB covers the measured 16,900-slot NavierStokes grids and the
// 131,072-bucket tables hash-map's rehash allocates with `new Array(capacity)`,
// which at its load factor never fill enough to turn dense, while still sending
// genuinely large `new Array(length)` allocations to the sparse representation.
// The slot limit is derived from Value's representation.
const DENSE_ARRAY_HOLE_BUDGET_BYTES: usize = 1024 * 1024;
const MAX_DENSE_ARRAY_HOLE_LENGTH: usize = DENSE_ARRAY_HOLE_BUDGET_BYTES / size_of::<Value>();
// A sparse element is an index and a value in a hash table, at least this many dense slots;
// a sparse array whose elements fill as many slots as its length turns dense.
const SPARSE_ENTRY_SLOTS: usize = size_of::<(usize, Value)>() / size_of::<Value>();

/// Whether `present` elements of an array of `length` fill as many dense slots as its
/// length, so dense storage takes no more words than sparse entries would.
fn dense_storage_fits(present: usize, length: usize) -> bool {
    present.saturating_mul(SPARSE_ENTRY_SLOTS) >= length
}
pub use pacing::MemoryPolicy;
pub(crate) use pacing::PromotionAge;
/// The prototypes `Heap::mark_prototype` remembers: an ordinary object's and an array's,
/// whose allocations alternate (array literals and objects in one request handler), so one
/// remembered prototype would look the other up on nearly every allocation.
const MARKED_PROTOTYPE_WAYS: usize = 2;
#[inline(always)]
fn payload_unit() -> usize {
    PAYLOAD_BYTES_PER_ALLOCATION
}
/// The fewest allocations after which a collection is full on a small heap: the first
/// collection runs after the young minimum, and is full once the allocation cap has passed.
#[cfg(test)]
pub(crate) fn full_collection_allocation_bound(policy: MemoryPolicy) -> usize {
    let pacing = policy.pacing();
    pacing.minimum_young.max(pacing.minimum_allocation_cap)
}
/// Payload bytes beside a cell (string units, array elements) that weigh as one more
/// allocation toward the next collection.
const PAYLOAD_BYTES_PER_ALLOCATION: usize = 64;
/// The fewest cells for which a collection runs on the parallel pool: live slots and live
/// records for a mark (`Heap::parallel_mark`), which visits what is live, and allocated slots
/// for a sweep (`sweep_full_parallel`), which visits every slot. Below it a full mark takes a
/// few milliseconds, which rounds do not shorten: on a synthetic Throughput record graph
/// (4-vCPU Linux x86_64, four markers) the explicit full collections took as long as serial
/// ones at 52K and 110K live cells, 27% less at 223K and 52% less mark at 1.65M. A
/// MemoryFirst heap rarely reaches it.
const PARALLEL_MARK_MINIMUM_CELLS: usize = 50_000;
const PARALLEL_MARK_MAX_THREADS: usize = 4;
/// The deepest a marker's stack grows before it offers half to an idle worker. A pointer-deep
/// graph keeps its stack shallow (V8-v7 Splay's tree and payload trees), so at 256 helpers
/// starved: Splay's full mark took 345 ms on one marker and 245 ms on four, 140 ms at 16
/// (4-vCPU Linux x86_64, `profile-gc-time` experiment build, five runs each; 32 gave 147 ms,
/// 8 gave 139-179 ms).
const PARALLEL_MARK_SPLIT_THRESHOLD: usize = 16;
const PARALLEL_MARK_STACK_SIZE_BYTES: usize = 256 * 1024;
const PARALLEL_SWEEP_SLAB_BATCH_SIZE: usize = 8;

/// The word and bit of slot `index` in a bit-per-slot vector (`Heap::marks`, `Heap::aged`,
/// `Heap::remembered_marks`).
#[inline(always)]
/// The property owners a compaction moves, in the order of their runs' offsets: the packed
/// slot owners, then the direct ones.
fn compaction_order(slot_owners: Vec<u64>, direct_owners: Vec<(usize, Value)>) -> Vec<(usize, Value)> {
    let mut owners = Vec::with_capacity(slot_owners.len() + direct_owners.len());
    owners.extend(slot_owners.into_iter().map(|key| {
        let offset = (key >> u32::BITS) as usize;
        let owner = owner_of_packed(key) as u32;
        (offset, Value::heap(owner))
    }));
    owners.extend(direct_owners);
    owners.sort_unstable_by_key(|(offset, _)| *offset);
    owners
}

/// The record spaces and slots a property owner's object is reached through during compaction.
struct PropertyOwners<'a> {
    object_records: &'a mut ObjectRecordSpace,
    array_records: &'a mut ArrayRecordSpace,
    function_records: &'a mut FunctionRecordSpace,
    promise_records: &'a mut PromiseRecordSpace,
    iterator_records: &'a mut IteratorRecordSpace,
    slots: &'a mut Vec<Slot>,
}

impl PropertyOwners<'_> {
    /// The object whose property run `owner` names, which compaction keeps live.
    fn object_mut(&mut self, owner: Value) -> &mut Object {
        let object = if let Some(record_id) = owner.direct_object_index() {
            // SAFETY: direct owners were collected from live marked records.
            Some(unsafe { self.object_records.get_live_mut_unbarriered(record_id) })
        } else if let Some(record_id) = owner.direct_array_index() {
            // SAFETY: array owners were collected from live array records.
            Some(&mut **unsafe { self.array_records.get_live_mut_unbarriered(record_id) })
        } else if let Some(record_id) = owner.direct_function_index() {
            // SAFETY: function owners were collected from live function records.
            Some(&mut unsafe { self.function_records.get_live_mut_unbarriered(record_id) }.object)
        } else if let Some(record_id) = owner.direct_promise_index() {
            // SAFETY: promise owners were collected from live promise records.
            Some(&mut unsafe { self.promise_records.get_live_mut_unbarriered(record_id) }.object)
        } else if let Some(record_id) = owner.direct_iterator_index() {
            // SAFETY: iterator owners were collected from live iterator records.
            Some(&mut unsafe { self.iterator_records.get_live_mut_unbarriered(record_id) }.object)
        } else {
            owner.legacy_heap_index().and_then(|index| self.slots.get_mut(index as usize)).and_then(|slot| slot.cell.as_mut()).and_then(Cell::object_mut)
        };
        object.expect("property owner remains live during compaction")
    }
}

fn slot_bit(index: usize) -> (usize, u64) {
    (index / u64::BITS as usize, 1_u64 << (index % u64::BITS as usize))
}

#[inline(always)]
fn mark_word_if_new(word: &AtomicU64, mask: u64) -> bool {
    if word.load(Ordering::Relaxed) & mask != 0 {
        return false;
    }
    word.fetch_or(mask, Ordering::Relaxed) & mask == 0
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum CollectionState {
    #[default]
    Idle,
    MinorMarking,
    FullMarking,
    Sweeping,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum GcTraceKind {
    Collection,
    VerifyFull,
}

/// The object-root trace parallel markers call (`ParallelMarkView::mark_value`), and the
/// shapes whose roots it has already added for every holder that is no prototype: a marker
/// tests that bit inline and calls the trace only for a shape it does not hold. Empty where
/// every holder must reach the trace (owned roots, or a verifying trace).
pub(crate) struct ParallelObjectRootTrace<'a> {
    pub(crate) trace: &'a (dyn Fn(GcTraceKind, Value, u32, bool, &mut Vec<Value>) + Sync + 'a),
    pub(crate) seen_shapes: &'a [std::sync::atomic::AtomicU64],
}

impl ParallelObjectRootTrace<'_> {
    /// Calls the trace unless `shape`'s roots are already added and the holder is no prototype.
    #[inline(always)]
    pub(crate) fn append(&self, kind: GcTraceKind, owner: Value, shape: u32, prototype: bool, roots: &mut Vec<Value>) {
        if !prototype && self.adds_nothing(kind, shape) {
            return;
        }
        (self.trace)(kind, owner, shape, prototype, roots);
    }

    /// Whether the trace would add nothing for a holder of `shape` that is no prototype.
    #[inline(always)]
    pub(crate) fn adds_nothing(&self, kind: GcTraceKind, shape: u32) -> bool {
        kind != GcTraceKind::VerifyFull
            && self.seen_shapes.get(shape as usize / u64::BITS as usize)
                .is_some_and(|word| word.load(Ordering::Relaxed) & (1_u64 << (shape % u64::BITS)) != 0)
    }
}

pub(super) struct Slot {
    cell: Option<Cell>,
}

impl Slot {
    fn is_live(&self) -> bool {
        self.cell.is_some()
    }
}

struct ParallelSweepSlab<'slots> {
    base: usize,
    slots: *mut Slot,
    len: usize,
    _borrow: std::marker::PhantomData<&'slots mut [Slot]>,
}

// SAFETY: these wrappers are built from the disjoint mutable slices returned
// by SlotArena::slabs_mut. Each wrapper is consumed by exactly one Rayon task,
// and all tasks join before any slot is read or mutated on the collector
// thread. The collection has stopped the mutator, and the task only reads live
// cells and takes dead cells from its unique slice.
unsafe impl Send for ParallelSweepSlab<'_> {}

impl<'slots> ParallelSweepSlab<'slots> {
    fn new(base: usize, slots: &'slots mut [Slot]) -> Self {
        Self {
            base,
            slots: slots.as_mut_ptr(),
            len: slots.len(),
            _borrow: std::marker::PhantomData,
        }
    }

    fn sweep(&mut self, marks: &[AtomicU64], compacts: bool) -> ParallelSweepResult {
        let mut result = ParallelSweepResult {
            slab: (self.base / SLOTS_PER_SLAB) as u32,
            ..ParallelSweepResult::default()
        };
        // SAFETY: the wrapper exclusively owns this disjoint slab until the
        // joined Rayon task returns. No other code accesses its cells during
        // the stop-the-world sweep.
        let slots = unsafe { std::slice::from_raw_parts_mut(self.slots, self.len) };
        result.swept_slots = slots.len();
        for (offset, slot) in slots.iter_mut().enumerate() {
            let Some(cell) = slot.cell.as_ref() else {
                continue;
            };
            let index = self.base + offset;
            if Heap::marked(marks, index) {
                result.live += 1;
                result.live_payload_bytes += cell.payload_bytes();
                if let Some(object) = cell.object()
                    && compacts && object.properties().has_compact_range() {
                        result
                            .property_owners
                            .push(pack_offset_owner(object.properties().start_offset(), index));
                    }
                continue;
            }

            let cell = slot.cell.take().expect("occupied dead sweep slot");
            result.external_bytes = result.external_bytes.saturating_add(cell.external_bytes());
            if let Cell::Promise(promise) = &cell {
                result.abandoned_contexts.abandon(&promise.record);
            }
            if let Some(object) = cell.object() {
                result.released_properties.push(object.properties());
            }
            if matches!(cell, Cell::Array { .. }) {
                result.sparse_arrays.push(index as u32);
            }
            result.free_slots.insert(offset);
            let dead = StoppedCellDrop(Some(cell));
            if cell_rc_drop_lock(dead.0.as_ref().expect("dead sweep cell")).is_some() {
                result.rc_cells.push(dead);
            } else {
                drop(dead);
            }
        }
        result
    }
}

#[derive(Default)]
struct ParallelSweepResult {
    slab: u32,
    live: usize,
    property_owners: Vec<u64>,
    free_slots: FreeSlotBitmap,
    released_properties: Vec<ValueVec>,
    sparse_arrays: Vec<u32>,
    rc_cells: Vec<StoppedCellDrop>,
    external_bytes: usize,
    live_payload_bytes: usize,
    abandoned_contexts: AbandonedHostContexts,
    swept_slots: usize,
}

/// The string family's sweep in a full collection, run on a mark-pool worker while the
/// collector sweeps the other families (`collect_with_object_roots`). It touches only the
/// string records, the small strings and `StringAtoms`, which no other family's sweep reads. A dead string that
/// shares an `Rc` is handed back for the collector thread to drop after the join, as
/// `StoppedCellDrop` hands back cells: another family's dead record may drop a clone of the
/// same `Rc` at the same time.
struct StoppedStringSweep<'a> {
    records: &'a mut StringRecordSpace,
    small: &'a mut small_strings::SmallStringSpace,
    atoms: &'a mut string_atoms::StringAtoms,
    full: bool,
    age: PromotionAge,
}

#[derive(Default)]
struct StringSweepOutput {
    live: usize,
    freed: usize,
    /// Units of the surviving strings, summed by full sweeps only.
    live_bytes: usize,
    dead_bytes: usize,
    /// Dead strings sharing an `Rc`, for the collector thread to drop.
    rc_strings: Vec<JsString>,
    /// Dead small strings holding a written text, for the collector thread to drop.
    rc_small: Vec<SmallParts>,
}

// SAFETY: the job holds exclusive borrows of the string records, small strings and atoms, which no other
// code reads until the scope joins, with the mutator stopped. On a worker it drops only
// strings whose drop writes no reference count (`JsString::drop_is_local`,
// `SmallParts::drop_is_local`) and reads the
// others' units without cloning them, so no count is written off the collector thread.
unsafe impl Send for StoppedStringSweep<'_> {}
// SAFETY: as above; `rc_strings` is only moved across the join, never touched on the worker
// after the push.
unsafe impl Send for StringSweepOutput {}

impl StoppedStringSweep<'_> {
    fn run(self, on_worker: bool) -> StringSweepOutput {
        let mut out = StringSweepOutput::default();
        let atoms = self.atoms;
        self.records.warm_before_drop(JsString::prefetch_allocation);
        // Survivors are visited only by a full sweep, which recounts the payload ledger.
        let (live, freed) = self.records.sweep(self.full, self.age, self.full, |record| match record {
            RecordSweep::Keep(_, string) => out.live_bytes += string.flat_unit_bytes(),
            RecordSweep::Dead(record_id, string) => {
                atoms.forget(record_id);
                out.dead_bytes += string.flat_unit_bytes();
                if on_worker && !string.drop_is_local() {
                    out.rc_strings.push(string);
                }
            }
        });
        // Small strings share no atom or record with them; their texts are payload as well.
        let (small_live, small_freed) = self.small.sweep(self.full, self.age, self.full, |record| match record {
            RecordSweep::Keep(_, parts) => out.live_bytes += parts.text_bytes(),
            RecordSweep::Dead(_, parts) => {
                out.dead_bytes += parts.text_bytes();
                if on_worker && !parts.drop_is_local() {
                    out.rc_small.push(parts);
                }
            }
        });
        (out.live, out.freed) = (live + small_live, freed + small_freed);
        out
    }
}

#[derive(Default)]
struct SweepOutput {
    live: usize,
    /// Payload of the surviving cells, summed by full sweeps only.
    live_payload_bytes: usize,
    property_owners: Vec<u64>,
    freed_cells: usize,
    swept_slots: usize,
}

/// The record families `Heap::record_families` lists.
const RECORD_FAMILIES: usize = 9;

/// Every family's mark bits, borrowed apart from the slot a trace holds (`mark_bits!`).
#[derive(Clone, Copy)]
pub(super) struct MarkBits<'a> {
    object_records: &'a ObjectRecordSpace,
    array_records: &'a ArrayRecordSpace,
    function_records: &'a FunctionRecordSpace,
    promise_records: &'a PromiseRecordSpace,
    await_records: &'a AwaitRecordSpace,
    iterator_records: &'a IteratorRecordSpace,
    string_records: &'a StringRecordSpace,
    small_strings: &'a small_strings::SmallStringSpace,
    v128_records: &'a V128RecordSpace,
    marks: &'a [AtomicU64],
}

impl MarkBits<'_> {
    /// Whether `value` is a heap value marked so far.
    pub(super) fn is_marked(self, value: Value) -> bool {
        match value.heap_reference() {
            Some(HeapReference::ObjectRecord(id)) => self.object_records.is_marked(id),
            Some(HeapReference::ArrayRecord(id)) => self.array_records.is_marked(id),
            Some(HeapReference::FunctionRecord(id)) => self.function_records.is_marked(id),
            Some(HeapReference::PromiseRecord(id)) => self.promise_records.is_marked(id),
            Some(HeapReference::AwaitRecord(id)) => self.await_records.is_marked(id),
            Some(HeapReference::IteratorRecord(id)) => self.iterator_records.is_marked(id),
            Some(HeapReference::StringRecord(id)) => self.string_is_marked(id),
            Some(HeapReference::V128Record(id)) => self.v128_records.is_marked(id),
            Some(HeapReference::LegacySlot(index)) => self.legacy_is_marked(index as usize),
            None => false,
        }
    }

    /// Whether a string record, or a small string's slot, is marked.
    fn string_is_marked(self, id: u32) -> bool {
        match small_strings::small_string_local(id) {
            Some(local) => self.small_strings.is_marked(local),
            None => self.string_records.is_marked(id),
        }
    }

    /// Whether the legacy slot `index` is marked, in the mark words.
    fn legacy_is_marked(self, index: usize) -> bool {
        self.marks.get(index / u64::BITS as usize).is_some_and(|word| word.load(Ordering::Relaxed) & (1 << (index % u64::BITS as usize)) != 0)
    }
}

/// `MarkBits` of a heap, borrowing only its mark fields.
macro_rules! mark_bits {
    ($heap:expr) => {
        MarkBits {
            object_records: &$heap.object_records,
            array_records: &$heap.array_records,
            function_records: &$heap.function_records,
            promise_records: &$heap.promise_records,
            await_records: &$heap.await_records,
            iterator_records: &$heap.iterator_records,
            string_records: &$heap.string_records,
            small_strings: &$heap.small_strings,
            v128_records: &$heap.v128_records,
            marks: &$heap.marks,
        }
    };
}
use mark_bits;
mod sweep;
mod collect;
mod collect_sweep;
mod collect_settle;
mod verify;
mod mark;
mod children;
mod cell_kind;
mod storage_census;

/// The ASCII code units, one past the last (`Heap::ascii_unit_strings`).
pub(crate) const ASCII_UNITS: usize = 0x80;
/// The Latin-1 code units, one past the last: `Heap::ascii_unit_strings` holds a string for
/// each, the ASCII ones first (the table's readers index it by ASCII unit), then the rest
/// that a byte decode (`String.fromCharCode(byte)`) makes.
pub(crate) const UNIT_STRINGS: usize = 0x100;

/// Dead prototype record ids a heap withholds from reuse (`Heap::retire_dead_prototypes`)
/// before it advances `prototype_layout_epoch` once and frees them all. A withheld id costs one
/// record's space; each advance retires every cache and proof the VM holds.
const WITHHELD_PROTOTYPE_IDS: usize = 4096;

#[derive(Default)]
pub(crate) struct Heap {
    /// The concurrent cycle's mark state between steps (`concurrent_mark`). First, so a heap
    /// dropped mid-cycle stops its marker before any space the marker reads goes.
    cycle: Option<Box<concurrent_mark::ConcurrentCycle>>,
    slots: SlotArena,
    object_records: ObjectRecordSpace,
    array_records: ArrayRecordSpace,
    function_records: FunctionRecordSpace,
    promise_records: PromiseRecordSpace,
    await_records: AwaitRecordSpace,
    iterator_records: IteratorRecordSpace,
    /// Strings: immutable leaves, never prototypes, with no legacy-slot form.
    string_records: StringRecordSpace,
    /// Small strings (`small_strings`): ids of the string records' hole, and the texts read.
    small_strings: small_strings::SmallStringSpace,
    small_views: small_strings::SmallViews,
    small_templates: small_strings::SmallTemplates,
    string_atoms: string_atoms::StringAtoms,
    /// Wasm vectors: immutable leaves (`Heap::alloc_v128`), in a slot when this is full.
    v128_records: V128RecordSpace,
    /// Allocations per cell variant (`Cell::profile_variant_name`), finer than `CellKind`.
    #[cfg(feature = "profile-aggregate")]
    allocated_variants: std::collections::BTreeMap<&'static str, u64>,
    marks: Vec<AtomicU64>,
    /// The young cells that survived one minor collection (`PromotionAge::Second`), a bit per
    /// slot as in `marks`: unmarked, in `young`, freed or promoted by the next minor one. Empty
    /// until a minor collection ages a survivor; sized to `marks` by each that may.
    aged: Vec<u64>,
    remembered_marks: Vec<u64>,
    remembered: Vec<u32>,
    /// Old owners that may point at what the last minor collection left young
    /// (`PromotionAge::ages`): those the barriers logged before it and those it promoted.
    /// The next minor collection logs and traces them with the barriers' own log; one not
    /// written since then points at nothing young after it, so it is not carried again.
    carried_owners: Vec<Value>,
    /// Bytes of reclaimable programs installed since the last full collection
    /// (`charge_reclaimable_program`).
    reclaimable_program_bytes_since_full: usize,
    /// Bytes of the reclaimable programs that stayed at the last full collection, which the
    /// next one's program growth is measured against.
    reclaimable_program_bytes_after_full: usize,
    young: YoungSlots,
    /// The young cells with weak entries (weak maps and sets, weak refs, finalization
    /// registries): a minor collection prunes only these and the remembered cells.
    young_weak: Vec<u32>,
    free_by_slab: Vec<FreeSlotBitmap>,
    free_slabs: Vec<u32>,
    /// The slots of one free-list word claimed whole (`refill_bump`), which allocations take
    /// highest first, as the free list hands slots out, and the first slot of that word: claimed slots are young already, so an
    /// allocation is a bit scan. A collection hands back the unused ones first (`retire_bump`).
    bump_bits: u64,
    bump_base: usize,
    free_slots: usize,
    external_bytes: usize,
    /// The allocation weight charged since the heap was made or reset: one per cell or
    /// record plus its payload (`charge_allocation`). Pacing reads the weight since the last
    /// collection and since the last full one by subtracting the marks taken then.
    allocated: usize,
    /// `allocated` when the last collection ran.
    allocated_at_collection: usize,
    /// `allocated` when the last full collection ran.
    allocated_at_full: usize,
    /// Payload the live and not yet swept cells and records own (`Cell::payload_bytes`):
    /// allocation and growth add it, reclamation subtracts it. Pacing measures live
    /// weight with it, in the units allocation is charged in.
    payload_bytes: usize,
    /// Payload bytes grown since the last charge, below one allocation's weight.
    uncharged_payload_bytes: usize,
    /// The string each one-unit ASCII text made since the last collection got
    /// (`alloc_string`): strings have no identity, so one record serves every such text, as
    /// V8's single-character string table does. A collection empties it.
    ascii_unit_strings: Option<Box<[Value; UNIT_STRINGS]>>,
    /// What the last full collection kept, which the generational pacing grows from.
    live_after_full: usize,
    /// What the full collection that last traced closure environments through their reads
    /// kept (`EnvironmentTrace`); `None` before one has. Slots it cleared stay cleared, so a
    /// later one finds only what was captured since: it runs again once the live set has
    /// grown by `ENVIRONMENT_TRIM_GROWTH` of that.
    live_at_environment_trim: Option<usize>,
    /// The next collection traces the whole heap.
    full_due: bool,
    /// Selects the pacing table (`MemoryPolicy::pacing`); reset keeps it.
    memory_policy: MemoryPolicy,
    pacing_state: pacing::PacingState,
    /// What the running collection's sweeps found of the aged survivors, for its pacing plan.
    aged_fates: pacing::AgedFates,
    /// Host contexts of promises the sweep freed, for the host to release.
    abandoned_contexts: AbandonedHostContexts,
    /// Live weight after the previous collection.
    live_weight_after_previous: usize,
    live_weight_after_full: usize,
    /// Record and payload bytes occupied when the current full collection started, and what
    /// the last full collection kept (`occupied_resident_bytes`).
    resident_at_full_start: usize,
    resident_after_full: usize,
    /// Allocation weight between the last two full collections.
    weight_between_fulls: usize,
    /// Backing-store bytes allocated since the last collection, against its budget.
    external_since_collection: usize,
    external_collection_budget: usize,
    /// The slot and record property owners the last full sweep listed, which size the next
    /// sweep's lists so they do not regrow from empty (and reallocate) every collection.
    property_owner_counts: (usize, usize),
    old_live: usize,
    /// The `allocated` weight at which the next collection is due: the last collection's
    /// mark plus the young budget pacing planned (`set_young_budget`).
    /// Where `allocated` makes a collection due; a shared heap may move it earlier (`request_collection`).
    collect_at: std::cell::Cell<usize>,
    collections: u64,
    /// A process-unique id of this heap's record layout, drawn again by a snapshot restore (which resets `collections` and the record blocks): part of `native_layout_epoch`, so equal values are never another heap's or an earlier layout.
    layout_id: u64,
    /// The prototypes `mark_prototype` marked last, most recent first, which keep their
    /// mark for life: an allocation with one of them needs no lookup. A collection may free
    /// one and reuse its handle, so each one forgets them.
    marked_prototypes: [Option<Value>; MARKED_PROTOTYPE_WAYS],
    /// Every object ever marked as a prototype, held weakly: a collection advances
    /// `prototype_layout_epoch` only when one of them died, since only a dead prototype's
    /// record can be reused under a proof that names it (`retire_dead_prototypes`). Each is
    /// held with its subject in the dependency table (`Dependencies::watch`).
    prototypes: Vec<(Value, u32)>,
    /// The proofs that rely on prototypes' layouts, each retired when one it read changes.
    dependencies: dependencies::Dependencies,
    /// The published code whose generated code embeds heap identities, which every collection roots.
    folded: crate::folded_roots::FoldedRegistry,
    peak_live: usize,
    /// The last collection's emptied mark stack, the next one's root buffer: a fresh one was
    /// mapped and faulted in by every collection (dotnet-aot-wasm: 2 MB each, 95K faults).
    mark_stack: Vec<Value>,
    peak_survivors: usize,
    max_threshold: usize,
    collection_state: CollectionState,
    properties: ValueArena,
    roots: RootTable,
    sparse_arrays: Option<Box<FxHashMap<Value, SparseElements>>>,
    /// The typed arrays' view records by slot (`Heap::typed_view_directory`); declared after `slots`, whose cells' records drop first.
    typed_views: TypedViewTable,
    #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
    gc_profile: GcProfile,
    #[cfg(feature = "profile-memory")]
    memory_profile: memory_profile::MemoryProfile,
    /// Whether a concurrent cycle is marking (`concurrent_mark`): never set yet.
    satb_active: bool,
    /// Whether plain objects' property runs take the narrow form (`value_vec::NARROW_FORM`) at
    /// promotion and compaction: only the interpreter reads one in place, so only a run without
    /// native tiers (`Heap::set_narrow_runs`) makes them.
    narrow_runs: bool,
    /// Whether a function record took a young run since the last collection
    /// (`finished_run_object`): only then does a minor sweep visit live functions to list
    /// their young survivors (`YoungOwner::Function`).
    young_function_runs: bool,
    /// The arena length at the last narrowing pass and the fulls since it: the pass reads every
    /// live run, so it reruns only once the arena grew an eighth or eight fulls went by.
    narrow_pass: (usize, u32),
    /// Values the cycle's weak reads handed out (`Heap::shade`), for its mark: a value read
    /// through a weak reference may be one its snapshot does not reach.
    satb_shaded: std::cell::RefCell<Vec<Value>>,
    /// Whether the last collection kept the marks a finished cycle made, so its own trace
    /// reached only what the cycle had not (`Heap::last_collection_kept_cycle_marks`).
    last_collection_premarked: bool,
    /// The last finished cycle's polls, the allocation weight and wall time it ran over, and
    /// the values its remark marked, for the trace.
    last_cycle_extent: concurrent_mark::CycleExtent,
    /// The cycles' allocation weight, averaged (`CycleExtent::weight`): how early the next
    /// starts.
    cycle_weight_average: usize,
    /// The allocation count at which the running or next cycle's remark is due: past it, the
    /// interval's budget is spent, and the mutator assists the marker.
    cycle_due_at: usize,
    /// What decides whether the next full collection runs as a cycle when cycles are adaptive
    /// (`concurrent_mark::CycleGate`).
    cycle_gate: concurrent_mark::CycleGate,
    /// Whether old records keep reference counts (`rc`, `QUENCH_DEV=rc-gc`).
    rc_active: bool,
    /// Old records whose count is 0, to try freeing at the next minor collection, and the
    /// ones this minor collection made candidates (`Heap::rc_pre_mark`).
    rc_zero: Vec<Value>,
    rc_candidates: Vec<Value>,
    /// Whether the running mark is a minor collection's under counts: the aged object and
    /// array records it marks count their children as it pushes them (`Heap::mark_value`),
    /// and are counted in `rc_counted`.
    rc_marking: bool,
    rc_counted: usize,
    /// The records old as the running full collection started, per family
    /// (`Heap::rc_full_start`).
    rc_old: Vec<Vec<u64>>,
    /// Under `QUENCH_DEV=rc-verify`, the candidates counts would have freed, kept instead
    /// until a full collection's trace checks that none is reachable.
    rc_suspects: Vec<Value>,
    /// The last minor collection's counting (`QUENCH_TRACE=gc`): the microseconds before
    /// and after its mark, its candidates, the zero-count records it carried, its young
    /// survivors, and the candidates it freed.
    rc_trace: [u64; 9],
    /// Bumped when counts restart (`set_rc`), which drops every pin (`rc_pin`).
    rc_pin_epoch: u64,
    /// Marks on the parallel pool whatever the heap's size (`Heap::parallel_mark`), so unit
    /// tests reach the parallel path with small heaps.
    #[cfg(test)]
    parallel_mark_forced: bool,
    /// The promotion age tests run minor collections at, over the policy's
    /// (`Heap::promotion_age`).
    #[cfg(test)]
    promotion_age_for_test: Option<PromotionAge>,
}

#[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
#[derive(Clone, Copy)]
pub(crate) struct GcProfile {
    #[cfg(feature = "profile-aggregate")]
    pub allocated_kinds: [u64; CellKind::COUNT],
    #[cfg(feature = "profile-aggregate")]
    pub allocated_payload_bytes: [u64; CellKind::COUNT],
    #[cfg(feature = "profile-aggregate")]
    pub allocated_size_buckets: [[u64; crate::profile::SIZE_BUCKETS]; CellKind::COUNT],
    #[cfg(feature = "profile-aggregate")]
    pub roots: u64,
    #[cfg(feature = "profile-aggregate")]
    pub work_items: u64,
    #[cfg(feature = "profile-aggregate")]
    pub max_worklist: u64,
    #[cfg(feature = "profile-aggregate")]
    pub marked: u64,
    #[cfg(feature = "profile-aggregate")]
    pub freed: u64,
    #[cfg(feature = "profile-aggregate")]
    pub sweep_slots: u64,
    pub mark_nanos: u64,
    pub full_mark_nanos: u64,
    pub minor_mark_nanos: u64,
    pub sweep_nanos: u64,
    /// The sweep's phases: slot cells, direct records, and property-arena compaction.
    pub sweep_phase_nanos: [u64; 3],
    pub mark_clear_nanos: u64,
    /// VM-side collection phases: owner edges, root gathering, shape-trace setup, and
    /// the work after the heap collection.
    pub vm_phase_nanos: [u64; 4],
    pub full_collections: u64,
    pub minor_collections: u64,
    #[cfg(feature = "profile-aggregate")]
    pub remembered_owner_scans: u64,
    #[cfg(feature = "profile-aggregate")]
    pub young_cells_swept: u64,
    #[cfg(feature = "profile-aggregate")]
    pub allocations_between_collections_total: u64,
    #[cfg(feature = "profile-aggregate")]
    pub allocations_between_collections_min: u64,
    #[cfg(feature = "profile-aggregate")]
    pub allocations_between_collections_max: u64,
    #[cfg(feature = "profile-aggregate")]
    pub survivors_total: u64,
    #[cfg(feature = "profile-aggregate")]
    pub survivors_min: u64,
    #[cfg(feature = "profile-aggregate")]
    pub survivors_max: u64,
    #[cfg(feature = "profile-aggregate")]
    pub marked_kinds: [u64; CellKind::COUNT],
}
#[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
impl Default for GcProfile {
    fn default() -> Self {
        Self {
            #[cfg(feature = "profile-aggregate")]
            allocated_kinds: [0; CellKind::COUNT],
            #[cfg(feature = "profile-aggregate")]
            allocated_payload_bytes: [0; CellKind::COUNT],
            #[cfg(feature = "profile-aggregate")]
            allocated_size_buckets: [[0; crate::profile::SIZE_BUCKETS]; CellKind::COUNT],
            #[cfg(feature = "profile-aggregate")]
            roots: 0,
            #[cfg(feature = "profile-aggregate")]
            work_items: 0,
            #[cfg(feature = "profile-aggregate")]
            max_worklist: 0,
            #[cfg(feature = "profile-aggregate")]
            marked: 0,
            #[cfg(feature = "profile-aggregate")]
            freed: 0,
            #[cfg(feature = "profile-aggregate")]
            sweep_slots: 0,
            mark_nanos: 0,
            full_mark_nanos: 0,
            minor_mark_nanos: 0,
            sweep_nanos: 0,
            sweep_phase_nanos: [0; 3],
            mark_clear_nanos: 0,
            vm_phase_nanos: [0; 4],
            full_collections: 0,
            minor_collections: 0,
            #[cfg(feature = "profile-aggregate")]
            remembered_owner_scans: 0,
            #[cfg(feature = "profile-aggregate")]
            young_cells_swept: 0,
            #[cfg(feature = "profile-aggregate")]
            allocations_between_collections_total: 0,
            #[cfg(feature = "profile-aggregate")]
            allocations_between_collections_min: u64::MAX,
            #[cfg(feature = "profile-aggregate")]
            allocations_between_collections_max: 0,
            #[cfg(feature = "profile-aggregate")]
            survivors_total: 0,
            #[cfg(feature = "profile-aggregate")]
            survivors_min: u64::MAX,
            #[cfg(feature = "profile-aggregate")]
            survivors_max: 0,
            #[cfg(feature = "profile-aggregate")]
            marked_kinds: [0; CellKind::COUNT],
        }
    }
}

#[repr(usize)]
#[derive(Clone, Copy)]
pub(crate) enum CellKind {
    Object,
    Array,
    Map,
    Set,
    Iterator,
    WeakMap,
    WeakSet,
    WeakRef,
    Function,
    Environment,
    /// String records; only the allocation profile counts them by kind.
    #[cfg_attr(not(feature = "profile-aggregate"), allow(dead_code))]
    String,
    BigInt,
    Symbol,
    Date,
    Error,
    RegExp,
    ArrayFromAsync,
    AwaitState,
    TemporalDuration,
    TemporalPlainDate,
    TemporalPlainDateTime,
    TemporalPlainMonthDay,
    TemporalPlainYearMonth,
    TemporalZonedDateTime,
    TemporalInstant,
    WasmGlobal,
    WasmMemory,
    WasmTable,
    WasmElements,
    WasmBits64,
    WasmHostFunction,
    WasmV128,
    WasmGc,
    WasmExtern,
    WasmTag,
    WasmException,
}
impl CellKind {
    pub(crate) const COUNT: usize = Self::WasmException as usize + 1;
    #[cfg(feature = "profile-memory")]
    pub(crate) const NAMES: [&'static str; Self::COUNT] = [
        "object",
        "array",
        "map",
        "set",
        "iterator",
        "weak_map",
        "weak_set",
        "weak_ref",
        "function",
        "environment",
        "string",
        "bigint",
        "symbol",
        "date",
        "error",
        "regexp",
        "array_from_async",
        "await_state",
        "temporal_duration",
        "temporal_plain_date",
        "temporal_plain_date_time",
        "temporal_plain_month_day",
        "temporal_plain_year_month",
        "temporal_zoned_date_time",
        "temporal_instant",
        "wasm_global",
        "wasm_memory",
        "wasm_table",
        "wasm_elements",
        "wasm_bits64",
        "wasm_host_function",
        "wasm_v128",
        "wasm_gc",
        "wasm_extern",
        "wasm_tag",
        "wasm_exception",
    ];
}
#[derive(Default)]
struct SparseElements {
    values: FxHashMap<usize, Value>,
    length: usize,
}
/// How much the live set grows past what the last environment-trimming full collection kept
/// before another trims (`Heap::live_at_environment_trim`): one live set, so what closures
/// over-capture since can at most double what a trimmed heap holds.
const ENVIRONMENT_TRIM_GROWTH: pacing::Ratio = pacing::Ratio::new(1, 1);

impl Heap {
    pub fn new() -> Self {
        Self {
            slots: SlotArena::with_small_capacity(),
            marks: Vec::with_capacity(12),
            remembered_marks: Vec::with_capacity(12),
            remembered: Vec::with_capacity(384),
            carried_owners: Vec::new(),
            reclaimable_program_bytes_since_full: 0,
            reclaimable_program_bytes_after_full: 0,
            young: YoungSlots::default(),
            free_by_slab: Vec::with_capacity(12),
            free_slabs: Vec::with_capacity(12),
            bump_bits: 0,
            bump_base: 0,
            free_slots: 0,
            layout_id: Self::next_layout_id(),
            collect_at: std::cell::Cell::new(MemoryPolicy::default().pacing().minimum_young),
            max_threshold: MemoryPolicy::default().pacing().minimum_young,
            external_collection_budget: MemoryPolicy::default().pacing().minimum_external_budget,
            ..Self::default()
        }
    }
    /// Selects the memory policy at startup, before the first collection: the next
    /// collection runs after the policy's young minimum, and pacing follows its table.
    pub(crate) fn set_memory_policy(&mut self, policy: MemoryPolicy) {
        debug_assert_eq!(self.collections, 0, "the memory policy is chosen at startup");
        self.memory_policy = policy;
        self.max_threshold = 0;
        self.set_young_budget(self.pacing().minimum_young);
        self.external_collection_budget = self.pacing().minimum_external_budget;
    }
    #[inline] pub(crate) fn memory_policy(&self) -> MemoryPolicy {
        self.memory_policy
    }
    fn pacing(&self) -> pacing::GenerationalPacing {
        self.memory_policy.pacing().sized(&self.pacing_state)
    }
    /// The minor collections a young cell or record survives before it is old: the policy's
    /// (`GenerationalPacing::promotion_age`). It may change between collections: every minor
    /// collection frees or promotes the aged survivors it finds.
    pub(crate) fn promotion_age(&self) -> PromotionAge {
        #[cfg(test)]
        if let Some(age) = self.promotion_age_for_test {
            return age;
        }
        if self.rc_active {
            // Under counts every survivor of a minor collection is old, and counted, at once.
            return PromotionAge::First;
        }
        self.pacing_state.promotion_age(&self.pacing())
    }
    #[cfg(test)]
    pub(crate) fn set_promotion_age_for_test(&mut self, age: PromotionAge) {
        self.promotion_age_for_test = Some(age);
    }
    /// Keep the complete allocation census available to ownership tests.
    /// Explicit collection remains available; only threshold collection stops.
    #[cfg(test)]
    pub(crate) fn retain_allocations_for_test(&mut self) {
        self.collect_at.set(usize::MAX);
    }

    #[cfg(test)]
    pub(crate) fn environment_count_for_test(&self) -> usize {
        self.slots
            .iter()
            .filter(|slot| matches!(slot.cell, Some(Cell::Environment { .. })))
            .count()
    }

    #[cfg(test)]
    pub(crate) fn occupied_cell_count_for_test(&self) -> usize {
        self.slots.iter().filter(|slot| slot.cell.is_some()).count()
    }

    #[cfg(test)]
    pub(crate) fn root_count_for_test(&self) -> usize {
        self.roots.values().count()
    }

    /// Direct records exist. They are swept only serially, and a minor collection traces
    /// them only serially; a full collection may mark them in parallel (`parallel_mark`).
    fn has_direct_records(&self) -> bool {
        self.record_families()
            .iter()
            .any(|(family, _)| family.live_count() > 0)
    }

    /// How this collection marks in parallel, if it does: a full collection, or a minor one
    /// of a heap without records, once the live slots and records reach
    /// `PARALLEL_MARK_MINIMUM_CELLS`, on a host whose pool has workers.
    fn parallel_mark<'a>(
        &self,
        object_roots: Option<&'a ParallelObjectRootTrace<'a>>,
    ) -> Option<parallel_mark::ParallelMark<'a>> {
        let traces_records_in_parallel = !self.rc_marking
            && (self.collection_state == CollectionState::FullMarking || !self.has_direct_records());
        let forced = self.parallel_mark_forced();
        if !traces_records_in_parallel || !forced && self.live_slots() < PARALLEL_MARK_MINIMUM_CELLS {
            return None;
        }
        let round_minimum = if forced { 0 } else { PARALLEL_MARK_SPLIT_THRESHOLD };
        parallel_mark::parallel_mark_pool().map(|pool| parallel_mark::ParallelMark {
            pool,
            object_roots,
            round_minimum,
        })
    }

    /// Makes every collection whose kind may mark in parallel (`parallel_mark`) do so, in
    /// rounds of any size, whatever the heap's size: small test heaps then cover the rounds
    /// and the work workers leave to the collector.
    #[cfg(test)]
    pub(crate) fn force_parallel_mark_for_test(&mut self) {
        self.parallel_mark_forced = true;
    }

    fn parallel_mark_forced(&self) -> bool {
        #[cfg(test)]
        return self.parallel_mark_forced;
        #[cfg(not(test))]
        false
    }

    /// Every record family, with the `Value` constructor naming its records.
    fn record_families(&self) -> [(&dyn RecordFamily, fn(RecordId) -> Value); RECORD_FAMILIES] {
        [
            (&self.object_records, Value::direct_object),
            (&self.array_records, Value::direct_array),
            (&self.function_records, Value::direct_function),
            (&self.promise_records, Value::direct_promise),
            (&self.await_records, Value::direct_await),
            (&self.iterator_records, Value::direct_iterator),
            (&self.string_records, Value::direct_string),
            (&self.small_strings, small_strings::small_string_value),
            (&self.v128_records, Value::direct_v128),
        ]
    }

    /// Record and payload bytes occupied now: live plus unswept garbage before a sweep.
    fn occupied_resident_bytes(&mut self) -> usize {
        let payload = self.payload_bytes;
        self.record_families_mut().iter().map(|(family, _)| family.occupied_bytes()).sum::<usize>() + payload
    }

    /// `record_families`, mutably.
    fn record_families_mut(
        &mut self,
    ) -> [(&mut dyn RecordFamily, fn(RecordId) -> Value); RECORD_FAMILIES] {
        [
            (&mut self.object_records, Value::direct_object),
            (&mut self.array_records, Value::direct_array),
            (&mut self.function_records, Value::direct_function),
            (&mut self.promise_records, Value::direct_promise),
            (&mut self.await_records, Value::direct_await),
            (&mut self.iterator_records, Value::direct_iterator),
            (&mut self.string_records, Value::direct_string),
            (&mut self.small_strings, small_strings::small_string_value),
            (&mut self.v128_records, Value::direct_v128),
        ]
    }

    /// Each program a live cell or function can still run or resolve names in
    /// (`Cell::program_references`).
    pub(crate) fn for_each_program_reference(&mut self, mut visit: impl FnMut(crate::vm::ProgramId)) {
        self.function_records
            .for_each_live(|_, function| function.program().into_iter().for_each(&mut visit));
        self.await_records
            .for_each_live(|_, state| state.program_references(&mut visit));
        self.iterator_records
            .for_each_live(|_, iterator| iterator.program_references(&mut visit));
        self.for_each_cell(|cell| cell.program_references(&mut visit));
    }

    /// Every live await state, in its record or in a slot.
    #[cfg(test)]
    pub(crate) fn for_each_await_state(&mut self, mut visit: impl FnMut(&AwaitState)) {
        self.await_records.for_each_live(|_, state| visit(state));
        self.for_each_cell(|cell| {
            if let Cell::AwaitState(state) = cell {
                visit(state);
            }
        });
    }

    /// Visits every cell in the slot heap.
    pub(crate) fn for_each_cell(&mut self, mut visit: impl FnMut(&Cell)) {
        for (_, slab) in self.slots.slabs_mut() {
            for slot in slab.iter() {
                if let Some(cell) = &slot.cell {
                    visit(cell);
                }
            }
        }
    }

    /// Every live legacy cell, without needing `&mut` (census and diagnostics).
    #[cfg(feature = "profile-gc-time")]
    pub(crate) fn for_each_live_cell(&self, mut visit: impl FnMut(&Cell)) {
        for slot in self.slots.iter() {
            if let Some(cell) = &slot.cell {
                visit(cell);
            }
        }
    }

    pub(crate) fn live_slots(&self) -> usize {
        self.slots.len() - self.free_slots
            + self
                .record_families()
                .iter()
                .map(|(family, _)| family.live_count())
                .sum::<usize>()
    }

    #[cfg(feature = "profile-memory")]
    pub(crate) fn slot_slab_census(
        &self,
        page_size: Option<usize>,
    ) -> (usize, usize, usize, Option<usize>) {
        let complete_slabs = self.slots.len() / SLOTS_PER_SLAB;
        let fully_free_slabs = self
            .free_by_slab
            .iter()
            .take(complete_slabs)
            .filter(|free| free.len() == SLOTS_PER_SLAB)
            .count();
        let partial_slabs = self
            .free_by_slab
            .iter()
            .take(complete_slabs)
            .filter(|free| !free.is_empty() && free.len() < SLOTS_PER_SLAB)
            .count();
        let fully_empty_pages =
            page_size.map(|page_size| self.slots.fully_empty_page_count(page_size));
        (
            complete_slabs,
            fully_free_slabs,
            partial_slabs,
            fully_empty_pages,
        )
    }

    /// The registered prototypes, direct objects, of exact shape `shape`; none where the
    /// registry does not know every prototype.
    pub(crate) fn prototypes_of_shape(&self, shape: u32) -> impl Iterator<Item = Value> + '_ {
        self.prototypes.iter().map(|(prototype, _)| *prototype).filter(move |prototype| {
            prototype.direct_object_index().is_some() && self.object(*prototype).is_some_and(|object| object.exact_shape() == shape)
        })
    }

    #[inline] pub(crate) fn mark_prototype(&mut self, prototype: Value) {
        if !self.marked_prototypes.contains(&Some(prototype)) {
            self.mark_new_prototype(prototype);
        }
    }

    /// `mark_prototype` of a prototype other than the one it marked last.
    #[inline(never)]
    fn mark_new_prototype(&mut self, prototype: Value) {
        // A prototype marked and watched before, as most allocations' prototypes are, only
        // takes the recent entry when its run is still wide: no exclusive access, which
        // widens and barriers.
        if self.object(prototype).is_some_and(|object| {
            object.was_used_as_prototype() && object.watched_subject().is_some() && !object.properties().is_narrow()
        }) {
            self.marked_prototypes.rotate_right(1);
            self.marked_prototypes[0] = Some(prototype);
            return;
        }
        let Some(unwatched) = self.prototype_object_mut(prototype).map(|object| object.watched_subject().is_none()) else {
            return;
        };
        // A prototype marked without a subject (newly, or in a restored heap, whose
        // prototypes watch nothing) takes one, which its extras keep.
        let watch = unwatched.then(|| self.dependencies.watch());
        let subject = watch.as_ref().and_then(dependencies::Watch::subject);
        let object = self.prototype_object_mut(prototype).expect("the prototype just read");
        object.mark_as_prototype();
        if let Some(watch) = watch {
            object.watch_as(watch);
        }
        if let Some(subject) = subject {
            self.prototypes.push((prototype, subject));
        }
        self.marked_prototypes.rotate_right(1);
        self.marked_prototypes[0] = Some(prototype);
    }

    /// The object `mark_prototype` marks: a record's, or a cell's.
    #[inline]
    fn prototype_object_mut(&mut self, prototype: Value) -> Option<&mut Object> {
        // A flag, not an edge: no write barrier for record prototypes.
        match prototype.direct_object_index() {
            // SAFETY: direct-object Values name live records; `&mut self` is exclusive.
            Some(record_id) => {
                let object = unsafe { self.object_records.get_live_mut_unbarriered(record_id) };
                // A prototype takes extras (`Object::mark_as_prototype`), which hold wide runs.
                access::widen_narrow_run(object, &mut self.properties);
                Some(object)
            }
            None => self.object_mut(prototype),
        }
    }

    /// The proofs that rely on prototypes' layouts (`dependencies`).
    #[inline(always)]
    pub(crate) fn dependencies(&self) -> &dependencies::Dependencies {
        &self.dependencies
    }

    #[inline(always)]
    pub(crate) fn dependencies_mut(&mut self) -> &mut dependencies::Dependencies {
        &mut self.dependencies
    }

    /// Retires the proofs that read `array`'s elements, when it is a watched prototype.
    fn note_prototype_elements(&self, array: Value) {
        if let Some(object) = self.object(array) {
            object.note_prototype_elements_change();
        }
    }

    /// Allocates `cell`. An array, whose kind a call site's literal cell fixes, takes its
    /// record directly (`alloc_array`) without the dispatch on the cell's kind.
    #[inline(always)]
    pub fn alloc(&mut self, cell: Cell) -> Value {
        match cell {
            Cell::Array { object } if !cfg!(feature = "profile-aggregate") => self.alloc_array(object),
            cell => self.alloc_cell(cell),
        }
    }

    /// `alloc(Cell::Array { object })` of a fresh array (`ArrayObject::fresh`) of `prototype`
    /// and `elements`, with the prototype as given rather than decoded back from the header.
    #[inline(always)]
    pub(crate) fn alloc_fresh_array(&mut self, prototype: Value, elements: ArrayElements) -> Value {
        let object = ArrayObject::fresh(prototype, elements);
        if cfg!(feature = "profile-aggregate") {
            return self.alloc(Cell::Array { object });
        }
        self.alloc_array_of(object, prototype)
    }

    /// `alloc(Cell::Array { object })`: an array record while one is free, charged as any
    /// allocation (`charge_allocation`) for its element storage.
    fn alloc_array(&mut self, object: ArrayObject) -> Value {
        let prototype = object.proto();
        self.alloc_array_of(object, prototype)
    }

    /// `alloc_array` of `object`, whose prototype is `prototype`.
    #[inline(always)]
    fn alloc_array_of(&mut self, object: ArrayObject, prototype: Value) -> Value {
        self.admit_object_contents(&object);
        self.mark_prototype(prototype);
        self.charge_payload_allocation(array_payload_bytes(&object));
        match self.array_records.try_insert(object) {
            Ok(record_id) => {
                let value = Value::direct_array(record_id);
                #[cfg(feature = "profile-memory")]
                self.profile_allocation(value);
                value
            }
            // Only a record's sweep shows a young run's survival (`ArrayElements::young`).
            Err(mut object) => {
                object.leave_nursery();
                self.alloc_in_slot(Cell::Array { object }, self.tracks_young_cells())
            }
        }
    }

    /// A young run for the `len` elements of an array record the caller makes before any
    /// collection (`ValueArena::young_elements`, `ArrayElements::young`).
    #[inline(always)]
    pub(crate) fn young_elements(&mut self, len: usize) -> Option<std::ptr::NonNull<Value>> {
        self.properties.young_elements(len)
    }

    /// A new array of `prototype`, with no properties, of the `len` elements `element` gives
    /// for each index, which a young run holds where it can (`ValueArena::young_elements`).
    #[inline(always)]
    pub(crate) fn alloc_literal_array(&mut self, prototype: Value, len: usize, element: impl FnMut(usize) -> Value) -> Value {
        let elements = match self.properties.young_elements(len) {
            // SAFETY: the run is the nursery's and nothing else's; the record follows at once.
            Some(run) => unsafe { ArrayElements::young(run, len, element) },
            None => ArrayElements::from_fn(len, element),
        };
        let object = ArrayObject::fresh(prototype, elements);
        if cfg!(feature = "profile-aggregate") {
            return self.alloc(Cell::Array { object });
        }
        self.alloc_array_of(object, prototype)
    }

    /// The allocation ledger's charge for one cell owning `payload` bytes beyond its slot, of
    /// which `external` are external buffers.
    #[inline(always)]
    fn charge_allocation(&mut self, payload: usize, external: usize) {
        self.charge_payload_allocation(payload);
        self.charge_external_growth(external);
    }

    /// `charge_allocation` of a cell that owns no external buffer, as an array.
    #[inline(always)]
    fn charge_payload_allocation(&mut self, payload: usize) {
        self.charge_materialized_text();
        let weight = 1 + payload / payload_unit();
        self.payload_bytes = self.payload_bytes.saturating_add(payload);
        self.allocated += weight;
    }

    fn alloc_cell(&mut self, cell: Cell) -> Value {
        if let Some(object) = cell.object() {
            self.admit_object_contents(object);
            self.mark_prototype(object.proto());
        }
        #[cfg(feature = "profile-aggregate")]
        {
            let kind = Self::cell_kind(&cell) as usize;
            let bytes = Self::cell_payload_bytes(&cell);
            self.gc_profile.allocated_kinds[kind] += 1;
            *self
                .allocated_variants
                .entry(cell.profile_variant_name())
                .or_default() += 1;
            self.gc_profile.allocated_payload_bytes[kind] += bytes as u64;
            self.gc_profile.allocated_size_buckets[kind][crate::profile::size_bucket(bytes)] += 1;
        }
        self.charge_allocation(cell.payload_bytes(), cell.external_bytes());
        let track_young = self.tracks_young_cells();
        let cell = match cell {
            Cell::Object(object) => match self.object_records.try_insert(object) {
                Ok(record_id) => {
                    let value = Value::direct_object(record_id);
                    #[cfg(feature = "profile-memory")]
                    self.profile_allocation(value);
                    return value;
                }
                Err(object) => Cell::Object(object),
            },
            Cell::Array { object } => match self.array_records.try_insert(object) {
                Ok(record_id) => {
                    let value = Value::direct_array(record_id);
                    #[cfg(feature = "profile-memory")]
                    self.profile_allocation(value);
                    return value;
                }
                Err(mut object) => {
                    object.leave_nursery();
                    Cell::Array { object }
                }
            },
            Cell::Promise(promise) => match self.promise_records.try_insert(*promise) {
                Ok(record_id) => {
                    let value = Value::direct_promise(record_id);
                    #[cfg(feature = "profile-memory")]
                    self.profile_allocation(value);
                    return value;
                }
                Err(promise) => Cell::Promise(Box::new(promise)),
            },
            Cell::Iterator(iterator) => match self.iterator_records.try_insert(*iterator) {
                Ok(record_id) => {
                    let value = Value::direct_iterator(record_id);
                    #[cfg(feature = "profile-memory")]
                    self.profile_allocation(value);
                    return value;
                }
                Err(iterator) => Cell::Iterator(Box::new(iterator)),
            },
            Cell::AwaitState(state) => match self.await_records.try_insert(*state) {
                Ok(record_id) => return Value::direct_await(record_id),
                Err(state) => Cell::AwaitState(Box::new(state)),
            },
            Cell::Function(function) => match self.function_records.try_insert(*function) {
                Ok(record_id) => {
                    let value = Value::direct_function(record_id);
                    #[cfg(feature = "profile-memory")]
                    self.profile_allocation(value);
                    return value;
                }
                Err(function) => Cell::Function(Box::new(function)),
            },
            cell => cell,
        };
        self.alloc_slotted(cell, track_young)
    }

    /// `cell` in a legacy slot; a typed array also joins the table native code reads its views from.
    fn alloc_slotted(&mut self, cell: Cell, track_young: bool) -> Value {
        let typed = matches!(cell, Cell::TypedArray { .. });
        let value = self.alloc_in_slot(cell, track_young);
        if typed {
            self.register_typed_view(value);
        }
        value
    }

    /// Names the record of the typed array just allocated at `value` in the table native code reads.
    fn register_typed_view(&mut self, value: Value) {
        let Some(index) = value.legacy_heap_index() else { return };
        if let Some(Cell::TypedArray { object, .. }) = self.slots.get_mut(index as usize).and_then(|slot| slot.cell.as_mut()) {
            self.typed_views.register(index, &mut object.native);
        }
    }

    /// The directory of the typed array view record pages by legacy slot and its page count
    /// (`NATIVE_TYPED_ARRAY_LAYOUT`): read an entry from a current live typed array's slot
    /// index, finish before any VM safepoint, which an allocation may move the directory at.
    pub(crate) fn typed_view_directory(&self) -> (*const usize, usize) {
        self.typed_views.directory()
    }

    /// `alloc(Cell::BigInt(value))` without the dispatch on the cell's kind: a BigInt has no
    /// prototype, payload or external bytes, so its weight is one allocation.
    #[inline(always)]
    pub(crate) fn alloc_bigint(&mut self, value: crate::bigint::JsBigInt) -> Value {
        if cfg!(feature = "profile-aggregate") {
            return self.alloc(Cell::BigInt(value));
        }
        self.allocated += 1;
        // A BigInt holds no weak entries.
        self.alloc_in_slot_with(self.tracks_young_cells(), false, || Cell::BigInt(value))
    }

    /// `alloc(Cell::Environment { .. })` without the dispatch on the cell's kind, with the
    /// address of the environment's slot storage, which stays put while it lives. An
    /// environment's weight is one allocation: it has no prototype, payload or external bytes.
    pub(crate) fn alloc_environment(
        &mut self,
        parent: Value,
        function: u32,
        slots: EnvironmentSlots,
        scope: EnvironmentScopeRef,
    ) -> (Value, *const EnvironmentSlots) {
        let cell = Cell::Environment { parent, function, slots, scope };
        let environment = if cfg!(feature = "profile-aggregate") {
            self.alloc(cell)
        } else {
            self.allocated += 1;
            // An environment holds no weak entries (`Cell::has_weak_entries`).
            let track_young = self.tracks_young_cells();
            self.alloc_in_slot_with(track_young, false, || cell)
        };
        let storage = match self.get(environment) {
            Some(Cell::Environment { slots, .. }) => slots as *const EnvironmentSlots,
            _ => unreachable!("an environment is allocated in a slot"),
        };
        (environment, storage)
    }

    /// `alloc(Cell::Object(object))` without the dispatch on the cell's kind, which an
    /// ordinary object's construction knows: a direct record while one is free.
    #[inline(always)]
    pub(crate) fn alloc_object(&mut self, object: Object) -> Value {
        let prototype = object.proto();
        self.alloc_object_of(object, prototype)
    }

    /// `alloc_object_with_properties` of a planned object: `header`, derived once for
    /// `prototype` and an inline shape (`InlineObjectHeader::of`), and its `inline` values.
    #[inline(always)]
    pub(crate) fn alloc_object_from_header(
        &mut self,
        header: InlineObjectHeader,
        prototype: Value,
        inline: [Value; INLINE_PROPERTY_COUNT],
    ) -> Value {
        let Some(object) = Object::from_inline_header(header, inline) else {
            return self.alloc_object_with_properties_of_header(header, prototype, inline);
        };
        debug_assert!(object.proto() == prototype);
        self.alloc_object_of(object, prototype)
    }

    /// `alloc_object_with_properties` of an object whose values outgrow its inline slots, or
    /// that they cannot hold: `header`, derived for `prototype` and an arena shape
    /// (`InlineObjectHeader::of_run`), over a new run of the first `len` of `values`.
    #[inline(always)]
    pub(crate) fn alloc_object_from_run_header<const N: usize>(
        &mut self,
        header: InlineObjectHeader,
        prototype: Value,
        values: &[Value; N],
        len: usize,
    ) -> Value {
        self.properties.assert_shape_length(header.shape(), len);
        if cfg!(feature = "profile-aggregate") {
            return self.alloc_object_with_properties(prototype, header.shape(), &values[..len]);
        }
        match self.properties.young_run_padded(values, len, header.shape()) {
            Some(start) => self.alloc_object_of(Object::from_run_header(header, start), prototype),
            None => self.alloc_object_with_properties(prototype, header.shape(), &values[..len]),
        }
    }

    /// `alloc_object_from_header` of values the payload word cannot hold: arena storage.
    #[cold]
    fn alloc_object_with_properties_of_header(
        &mut self,
        header: InlineObjectHeader,
        prototype: Value,
        inline: [Value; INLINE_PROPERTY_COUNT],
    ) -> Value {
        let shape = header.shape();
        let length = self.properties.len(ValueVec::inline_property_storage(shape));
        self.alloc_object_with_properties(prototype, shape, &inline[..length])
    }

    /// `alloc_object` of `object`, whose prototype is `prototype`.
    #[inline(always)]
    fn alloc_object_of(&mut self, object: Object, prototype: Value) -> Value {
        if cfg!(feature = "profile-aggregate") {
            return self.alloc(Cell::Object(object));
        }
        self.mark_prototype(prototype);
        self.insert_object_record(object)
    }

    /// `alloc_object` of an object of `header` (inline storage), whose prototype is already
    /// flagged as one (`Object::was_used_as_prototype`), as a recorded closure's are
    /// (`Vm::closure_of_template`), so it needs no marking, and whose slots hold `undefined` until stores fill them (`fill_new_object_slot`), which
    /// record their values' representations: the placeholders record none.
    #[inline(always)]
    pub(crate) fn alloc_unfilled_object_under_flagged_prototype(&mut self, header: InlineObjectHeader) -> Value {
        let object = Object::from_inline_header(header, [Value::UNDEFINED; INLINE_PROPERTY_COUNT])
            .expect("undefined is narrow");
        debug_assert!(self.prototype_flagged(object.proto()));
        if cfg!(feature = "profile-aggregate") {
            return self.alloc(Cell::Object(object));
        }
        self.insert_admitted_object_record(object)
    }

    /// `alloc_unfilled_object_under_flagged_prototype` of an object whose first slot holds
    /// `first` from the start, admitted; `None` where the header cannot hold it inline.
    #[inline(always)]
    pub(crate) fn alloc_object_under_flagged_prototype_with_first(&mut self, header: InlineObjectHeader, first: Value) -> Option<Value> {
        if cfg!(feature = "profile-aggregate") {
            return None;
        }
        let mut inline = [Value::UNDEFINED; INLINE_PROPERTY_COUNT];
        inline[0] = first;
        let object = Object::from_inline_header(header, inline)?;
        debug_assert!(self.prototype_flagged(object.proto()));
        Some(self.insert_object_record(object))
    }

    /// A function record claimed before its function exists, as the value that names it, for
    /// a closure whose `.prototype` names the closure from its creation: written by
    /// `fill_claimed_function_record` before any collection. `None` where the claim would
    /// take the record space's slow path.
    #[inline(always)]
    pub(crate) fn claim_function_record(&mut self) -> Option<Value> {
        if cfg!(feature = "profile-aggregate") {
            return None;
        }
        self.function_records.claim_uninit_quick().map(Value::direct_function)
    }

    /// `alloc_function_under_flagged_prototype` of `function` into the record `claimed`
    /// (`claim_function_record`) names.
    #[inline(always)]
    pub(crate) fn fill_claimed_function_record(&mut self, claimed: Value, function: FunctionRecord) {
        debug_assert!(self.prototype_flagged(function.object.proto()));
        let id = claimed.direct_function_index().expect("a claimed function record");
        self.admit_object_contents(&function.object);
        self.allocated += 1;
        // SAFETY: the claim made the id occupied with its record uninitialized, written once here.
        unsafe { self.function_records.record_ptr(id).write(function) };
        #[cfg(feature = "profile-memory")]
        self.profile_allocation(claimed);
    }

    /// An object of the run header `header` (`InlineObjectHeader::of_run`) over a new young
    /// run of the first `len` of `values` (the rest `undefined`), for a function record
    /// (`YoungOwner::Function`), not yet allocated; `None` when the nursery has no run for it.
    #[inline(always)]
    pub(crate) fn finished_run_object<const N: usize>(
        &mut self,
        header: InlineObjectHeader,
        values: &[Value; N],
        len: usize,
    ) -> Option<Object> {
        self.properties.assert_shape_length(header.shape(), len);
        if cfg!(feature = "profile-aggregate") {
            return None;
        }
        let start = self.properties.young_run_padded(values, len, header.shape())?;
        self.young_function_runs = true;
        Some(Object::from_run_header(header, start))
    }

    /// `property_set` of slot `slot` of `object`, an object record allocated with inline
    /// storage since the last allocation that could collect, whose slot holds `undefined`:
    /// the barrier and the slot's recorded representation as for any store, and the narrow
    /// store into the payload word without the general path's tests.
    #[inline(always)]
    pub(crate) fn fill_new_object_slot(&mut self, object: Value, slot: usize, value: Value) {
        let Some(record_id) = object.direct_object_index() else {
            return self.property_set(object, slot, value);
        };
        let Self { object_records, properties, .. } = &mut *self;
        // SAFETY: direct-object Values name live records; `&mut self` is exclusive.
        let data = unsafe { object_records.get_live_mut_storing(record_id, value, properties) };
        if !data.properties().has_inline_property_storage() || !data.store_inline_narrow(slot, value) {
            return self.property_set(object, slot, value);
        }
        if properties.tracks() {
            let shape = data.exact_shape();
            if shape == data.shape() {
                properties.note_slot_value(shape, slot, value);
            }
        }
    }

    /// Whether `prototype` names no object or one flagged as a prototype, as
    /// `mark_new_prototype` leaves it.
    fn prototype_flagged(&self, prototype: Value) -> bool {
        self.object(prototype).is_none_or(Object::was_used_as_prototype)
    }

    /// An object record of `object`, else a slot: one allocation, with no external bytes.
    #[inline(always)]
    fn insert_object_record(&mut self, object: Object) -> Value {
        self.admit_object_contents(&object);
        self.insert_admitted_object_record(object)
    }
    /// `insert_object_record` of an object whose initial values the caller already admitted.
    #[inline(always)]
    fn insert_admitted_object_record(&mut self, object: Object) -> Value {
        self.allocated += 1;
        match self.object_records.try_insert(object) {
            Ok(record_id) => {
                let value = Value::direct_object(record_id);
                #[cfg(feature = "profile-memory")]
                self.profile_allocation(value);
                value
            }
            Err(mut object) => {
                // A young run's owner is a record (`YoungOwner`): an object that takes a legacy slot takes an ordinary run first.
                if self.properties.holds_young_run(object.properties()) {
                    let storage = self.properties.duplicate(object.properties());
                    object.replace_property_storage(storage, [Value::UNDEFINED; INLINE_PROPERTY_COUNT], &mut self.properties);
                }
                self.alloc_in_slot(Cell::Object(object), self.tracks_young_cells())
            }
        }
    }

    /// Admit initial named properties before any object-bearing cell or record
    /// becomes visible. Generic and dedicated publishers share the live-slot reader.
    #[inline(always)]
    fn admit_object_contents(&mut self, object: &Object) {
        if self.properties.tracks() {
            join_object_slot_values(object, &mut self.properties);
            self.recode_new_object(object);
        }
    }

    /// `alloc(Cell::Function(..))` without boxing the function first: a function record
    /// while one is free.
    pub(crate) fn alloc_function(&mut self, function: FunctionRecord) -> Value {
        if cfg!(feature = "profile-aggregate") {
            return self.alloc(Cell::Function(Box::new(function)));
        }
        self.mark_prototype(function.object.proto());
        self.insert_function_record(function)
    }

    /// `alloc_function` of `function` whose prototype is already flagged as one, as
    /// `alloc_unfilled_object_under_flagged_prototype` allocates an object.
    pub(crate) fn alloc_function_under_flagged_prototype(&mut self, function: FunctionRecord) -> Value {
        debug_assert!(self.prototype_flagged(function.object.proto()));
        if cfg!(feature = "profile-aggregate") {
            return self.alloc(Cell::Function(Box::new(function)));
        }
        self.insert_function_record(function)
    }

    /// A function record of `function`, else a slot: one allocation, with no payload or
    /// external bytes.
    #[inline(always)]
    fn insert_function_record(&mut self, function: FunctionRecord) -> Value {
        self.admit_object_contents(&function.object);
        self.allocated += 1;
        match self.function_records.try_insert(function) {
            Ok(record_id) => {
                let value = Value::direct_function(record_id);
                #[cfg(feature = "profile-memory")]
                self.profile_allocation(value);
                value
            }
            Err(mut function) => {
                // A young run's owner is a record (`YoungOwner`): a function that takes a
                // legacy slot takes an ordinary run first.
                if self.properties.holds_young_run(function.object.properties()) {
                    let storage = self.properties.duplicate(function.object.properties());
                    function.object.replace_property_storage(storage, [Value::UNDEFINED; INLINE_PROPERTY_COUNT], &mut self.properties);
                }
                self.alloc_in_slot(Cell::Function(Box::new(function)), self.tracks_young_cells())
            }
        }
    }

    /// `alloc(Cell::Promise(..))` without boxing the promise first: a promise record while
    /// one is free.
    /// `alloc(Cell::AwaitState(..))` without boxing the state first: an await record while
    /// one is free. An await state is internal: no prototype, payload or external bytes.
    pub(crate) fn alloc_await_state(&mut self, state: AwaitState) -> Value {
        if cfg!(feature = "profile-aggregate") {
            return self.alloc(Cell::AwaitState(Box::new(state)));
        }
        self.allocated += 1;
        match self.await_records.try_insert(state) {
            Ok(record_id) => Value::direct_await(record_id),
            Err(state) => {
                self.alloc_in_slot(Cell::AwaitState(Box::new(state)), self.tracks_young_cells())
            }
        }
    }

    /// A Wasm vector of `bytes`: a vector record while one is free, else a slot. A vector is
    /// an immutable leaf with no prototype, payload or external bytes.
    #[inline]
    pub(crate) fn alloc_v128(&mut self, bytes: [u8; crate::wasm::V128_BYTES]) -> Value {
        if cfg!(feature = "profile-aggregate") {
            return self.alloc(Cell::WasmV128(bytes));
        }
        self.allocated += 1;
        match self.v128_records.try_insert(bytes) {
            Ok(record_id) => Value::direct_v128(record_id),
            Err(bytes) => self.alloc_in_slot(Cell::WasmV128(bytes), self.tracks_young_cells()),
        }
    }

    /// A Wasm vector's bytes, in its record or in a slot.
    #[inline]
    pub(crate) fn v128(&self, value: Value) -> Option<[u8; crate::wasm::V128_BYTES]> {
        if let Some(record_id) = value.direct_v128_index() {
            // SAFETY: a direct-v128 Value names an initialized record.
            return Some(*unsafe { self.v128_records.get_live(record_id) });
        }
        match self.get(value)? {
            Cell::WasmV128(bytes) => Some(*bytes),
            _ => None,
        }
    }

    /// `alloc(Cell::Iterator(..))` without boxing the iterator first: an iterator record
    /// while one is free.
    pub(crate) fn alloc_iterator(&mut self, iterator: IteratorRecord) -> Value {
        if cfg!(feature = "profile-aggregate") {
            return self.alloc(Cell::Iterator(Box::new(iterator)));
        }
        self.admit_object_contents(&iterator.object);
        self.mark_prototype(iterator.object.proto());
        // An iterator's weight is one allocation, and it holds no payload or external bytes.
        self.allocated += 1;
        match self.iterator_records.try_insert(iterator) {
            Ok(record_id) => {
                let value = Value::direct_iterator(record_id);
                #[cfg(feature = "profile-memory")]
                self.profile_allocation(value);
                value
            }
            Err(iterator) => {
                self.alloc_in_slot(Cell::Iterator(Box::new(iterator)), self.tracks_young_cells())
            }
        }
    }

    /// Allocations per cell variant.
    #[cfg(feature = "profile-aggregate")]
    pub(crate) fn allocated_variants(&self) -> &std::collections::BTreeMap<&'static str, u64> {
        &self.allocated_variants
    }

    pub(crate) fn alloc_promise(&mut self, promise: PromiseCell) -> Value {
        if cfg!(feature = "profile-aggregate") {
            return self.alloc(Cell::Promise(Box::new(promise)));
        }
        self.admit_object_contents(&promise.object);
        self.mark_prototype(promise.object.proto());
        // A promise's weight is one allocation, and it holds no payload or external bytes.
        self.allocated += 1;
        match self.promise_records.try_insert(promise) {
            Ok(record_id) => {
                let value = Value::direct_promise(record_id);
                #[cfg(feature = "profile-memory")]
                self.profile_allocation(value);
                value
            }
            Err(promise) => {
                self.alloc_in_slot(Cell::Promise(Box::new(promise)), self.tracks_young_cells())
            }
        }
    }

    /// The slot part of `alloc`, for a cell no record space takes.
    fn alloc_in_slot(&mut self, cell: Cell, track_young: bool) -> Value {
        let weak = cell.has_weak_entries();
        self.alloc_in_slot_with(track_young, weak, || cell)
    }

    /// `alloc_in_slot` of the cell `cell` makes, which a reused slot receives in place.
    #[inline(always)]
    fn alloc_in_slot_with(&mut self, track_young: bool, weak: bool, cell: impl FnOnce() -> Cell) -> Value {
        let Some(slot) = self.take_free_slot() else {
            return self.alloc_in_new_slot(cell(), track_young, weak);
        };
        let index = slot as u32;
        self.slots.get_mut(slot).unwrap().cell = Some(cell());
        // A claimed slot is young already (`refill_bump`).
        if !track_young {
            self.young.remove_word(slot / u64::BITS as usize, 1 << (slot % u64::BITS as usize));
        }
        if weak {
            self.young_weak.push(index);
        }
        if self.satb_active {
            self.blacken_slot(index as usize);
        }
        #[cfg(feature = "profile-memory")]
        self.profile_allocation(Value::heap(index));
        Value::heap(index)
    }

    /// A free slot's index, claimed and young: the highest of the claimed word's
    /// (`bump_bits`), else of a word newly claimed.
    #[inline(always)]
    fn take_free_slot(&mut self) -> Option<usize> {
        let bits = self.bump_bits;
        if bits != 0 {
            let bit = (u64::BITS - 1 - bits.leading_zeros()) as usize;
            self.bump_bits = bits & !(1 << bit);
            self.free_slots -= 1;
            return Some(self.bump_base + bit);
        }
        self.refill_bump()
    }

    /// Claims the free slots of the highest word of the last slab with one, makes them young
    /// in one word operation, and takes the highest. Allocations then cost a bit scan, not a
    /// free-list pop and a young insert each (BigInt sums, closures, environments).
    #[inline(never)]
    fn refill_bump(&mut self) -> Option<usize> {
        while let Some(slab) = self.free_slabs.last().copied() {
            let slab_index = slab as usize;
            let Some((word, bits)) = self.free_by_slab[slab_index].pop_word() else {
                self.free_slabs.pop();
                continue;
            };
            // Claimed slots stay counted free until taken.
            self.free_slots -= 1;
            if self.free_by_slab[slab_index].is_empty() {
                self.free_slabs.pop();
            }
            let base = slab_index * SLOTS_PER_SLAB + word * u64::BITS as usize;
            self.young.insert_word(base / u64::BITS as usize, bits);
            let bit = (u64::BITS - 1 - bits.leading_zeros()) as usize;
            self.bump_bits = bits & !(1 << bit);
            self.bump_base = base;
            return Some(base + bit);
        }
        None
    }

    /// Hands the claimed slots no allocation took back to the free list, not young, before a
    /// collection marks or sweeps: a sweep must not count free slots as young deaths.
    pub(super) fn retire_bump(&mut self) {
        let bits = std::mem::take(&mut self.bump_bits);
        if bits == 0 {
            return;
        }
        self.young.remove_word(self.bump_base / u64::BITS as usize, bits);
        let slab = self.bump_base / SLOTS_PER_SLAB;
        if self.free_by_slab[slab].is_empty() {
            self.free_slabs.push(slab as u32);
        }
        self.free_by_slab[slab].insert_word((self.bump_base % SLOTS_PER_SLAB) / u64::BITS as usize, bits);
    }

    /// `alloc_in_slot` when no slot is free: a new one past the last.
    #[inline(never)]
    fn alloc_in_new_slot(&mut self, cell: Cell, track_young: bool, weak: bool) -> Value {
        let index = self.slots.len();
        self.slots.push(Slot { cell: Some(cell) });
        let slab = index / SLOTS_PER_SLAB;
        if self.free_by_slab.len() == slab {
            self.free_by_slab.push(FreeSlotBitmap::default());
        }
        #[cfg(feature = "profile-memory")]
        self.profile_allocation(Value::heap(index as u32));
        if index / 64 == self.marks.len() {
            self.marks.push(AtomicU64::new(0));
        }
        if track_young {
            self.young.insert(index);
        }
        if weak {
            self.young_weak.push(index as u32);
        }
        if self.satb_active {
            self.blacken_slot(index);
        }
        Value::heap(index as u32)
    }
    /// Inline into the literal-pair paths (`Vm::object_pair`): as a call, its frame cost ~21
    /// Ir of the ~120 an allocation took.
    #[inline(always)]
    pub(crate) fn alloc_object_pair(
        &mut self,
        proto: Value,
        shape: u32,
        first: Value,
        second: Value,
    ) -> Value {
        if let Some(object) = Object::with_inline_values(proto, shape, &[first, second]) {
            return self.alloc_object(object);
        }
        // Values the inline slots cannot hold take a run of the smallest class, as a planned
        // receiver's do (`alloc_object_from_run_header`).
        let padded = [first, second, Value::UNDEFINED, Value::UNDEFINED];
        self.alloc_object_from_run_header(InlineObjectHeader::of_run(proto, shape), proto, &padded, INLINE_PROPERTY_COUNT)
    }
    pub(crate) fn alloc_object_with_properties(
        &mut self,
        proto: Value,
        shape: u32,
        values: &[Value],
    ) -> Value {
        self.properties.assert_shape_length(shape, values.len());
        if values.len() <= INLINE_PROPERTY_COUNT
            && let Some(object) = Object::with_inline_values(proto, shape, values)
        {
            return self.alloc_object(object);
        }
        let properties = self.properties.with_values(shape, values);
        self.alloc_object(Object::with_property_storage(
            proto,
            properties,
            [Value::UNDEFINED; INLINE_PROPERTY_COUNT],
        ))
    }
    /// `alloc(Cell::Array { object: array })` of a fresh array, given its first named
    /// properties `values` under `shape` first: the storage steps
    /// `initialize_object_properties` takes on the published cell (arena storage, as an
    /// array keeps its element backing in the payload; the replaced storage's integrity
    /// carried over and its arena words released), taken before the array is published. No
    /// collection runs in between, so the arena holds the values for their owner exactly as
    /// after the two-step path.
    pub(crate) fn alloc_array_with_properties(
        &mut self,
        mut array: ArrayObject,
        shape: u32,
        values: &[Value],
    ) -> Value {
        let (properties, inline_properties) = self.initial_object_storage(shape, values, false);
        let previous = array.replace_property_storage(properties, inline_properties, &mut self.properties);
        self.properties.release(previous);
        let value = self.alloc(Cell::Array { object: array });
        #[cfg(feature = "profile-memory")]
        self.note_object_slots(value, values.len());
        value
    }

    pub(crate) fn initialize_object_properties(
        &mut self,
        owner: Value,
        shape: u32,
        values: &[Value],
    ) {
        // An array's payload holds its element backing, so its properties live in the arena.
        let inline = !self
            .object(owner)
            .expect("property owner is an object")
            .is_array();
        self.properties.note_shape_values(shape, values);
        self.settle_widened();
        let (properties, inline_properties) = self.initial_object_storage(shape, values, inline);
        let previous = {
            let (object, arena) = self.object_and_arena_mut(owner).expect("property owner is an object");
            object.replace_property_storage(properties, inline_properties, arena)
        };
        self.properties.release(previous);
        #[cfg(feature = "profile-memory")]
        self.note_object_slots(owner, values.len());
    }

    /// Storage for an object's first property `values`: inline where they fit and `inline`
    /// allows it, else the arena.
    pub(crate) fn initial_object_storage(
        &mut self,
        shape: u32,
        values: &[Value],
        inline: bool,
    ) -> (ValueVec, [Value; INLINE_PROPERTY_COUNT]) {
        self.properties.assert_shape_length(shape, values.len());
        if inline && values.len() <= INLINE_PROPERTY_COUNT && Object::inline_values_narrow(values) {
            // At most `INLINE_PROPERTY_COUNT` values: a loop over the fixed bound unrolls to
            // element moves, where one over `values` becomes a `memcpy` call.
            let inline = std::array::from_fn(|slot| values.get(slot).copied().unwrap_or(Value::UNDEFINED));
            (ValueVec::inline_property_storage(shape), inline)
        } else {
            (
                self.properties.with_values(shape, values),
                [Value::UNDEFINED; INLINE_PROPERTY_COUNT],
            )
        }
    }
    /// Registers `shape`, of `length` slots, made from `parent` and its length, whose slot
    /// owners it extends (`ValueArena::register_derived_shape`).
    pub(crate) fn register_property_shape(&mut self, shape: u32, length: usize, parent: Option<(u32, usize)>) {
        self.properties.register_derived_shape(shape, length, parent);
    }
    /// Moves every live object to its compacted shape, and registers the compacted table:
    /// each shape's length and parent (a parent precedes its children), its slot owners, and
    /// their representations joined anew from the values live objects hold.
    pub(crate) fn remap_live_object_shapes(&mut self, mapping: &[u32], layouts: &[(usize, Option<(u32, usize)>)]) {
        self.dependencies_mut().retire_all(); // renumbered ids; shape readers reset below
        self.object_records.for_each_live_mut(|_, object| {
            let old_shape = object.shape() as usize;
            object.renumber_shape(mapping[old_shape]);
        });
        self.array_records.for_each_live_mut(|_, array| {
            let old_shape = array.shape() as usize;
            array.renumber_shape(mapping[old_shape]);
        });
        self.function_records.for_each_live_mut(|_, function| {
            let old_shape = function.object.shape() as usize;
            function.object.renumber_shape(mapping[old_shape]);
        });
        self.promise_records.for_each_live_mut(|_, promise| {
            let old_shape = promise.object.shape() as usize;
            promise.object.renumber_shape(mapping[old_shape]);
        });
        self.iterator_records.for_each_live_mut(|_, iterator| {
            let old_shape = iterator.object.shape() as usize;
            iterator.object.renumber_shape(mapping[old_shape]);
        });
        for slot in self.slots.iter_mut() {
            let Some(object) = slot.cell.as_mut().and_then(Cell::object_mut) else {
                continue;
            };
            let old_shape = object.shape() as usize;
            object.renumber_shape(mapping[old_shape]);
        }
        self.register_shape_layouts(layouts);
    }

    /// Registers a whole shape table, each shape's length and parent (a parent precedes its
    /// children), and, where representations are kept, their slot owners and their
    /// representations joined anew from the values live objects hold.
    fn register_shape_layouts(&mut self, layouts: &[(usize, Option<(u32, usize)>)]) {
        let tracking = self.properties.tracks();
        self.properties.reset_shapes();
        if tracking { self.properties.track(); }
        for (shape, (length, parent)) in layouts.iter().copied().enumerate() {
            debug_assert!(parent.is_none_or(|(parent, _)| (parent as usize) < shape), "a parent precedes its children");
            self.properties.register_derived_shape(shape as u32, length, parent);
        }
        self.join_live_slot_values();
        self.recode_all_slots();
    }

    /// Keeps slot representations from now on (`ValueArena::track`), over the shape table
    /// `layouts` (`register_shape_layouts`), from what live objects hold.
    pub(crate) fn track_slot_representations(&mut self, layouts: &[(usize, Option<(u32, usize)>)]) {
        self.properties.track();
        self.register_shape_layouts(layouts);
    }

    /// Joins named properties across every live object-bearing record and legacy cell.
    fn join_live_slot_values(&mut self) {
        if !self.properties.tracks() { return; }
        self.visit_live_objects(join_object_slot_values);
    }
    /// Moves the surviving young runs of `owners`, live direct objects and functions
    /// (`YoungOwner`), out of the nursery's young range, in the order of their starts, and
    /// ends the young cycle (`ValueArena::finish_young_cycle`).
    fn evacuate_young_runs(&mut self, mut owners: Vec<(usize, YoungOwner)>) {
        owners.sort_unstable_by_key(|&(start, _)| start);
        for (_, owner) in owners {
            // SAFETY: the owners were listed from records the sweep kept.
            let object = match owner {
                YoungOwner::Object(record_id) => unsafe { self.object_records.get_live_mut_unbarriered(record_id) },
                YoungOwner::Function(record_id) => &mut unsafe { self.function_records.get_live_mut_unbarriered(record_id) }.object,
            };
            let mut vector = object.properties();
            // A promoted plain object's values may take a narrow run, as at compaction.
            let narrows = self.narrow_runs && !object.has_extras();
            self.properties.evacuate_young(&mut vector, narrows);
            object.set_properties(vector, &mut self.properties);
        }
        self.properties.finish_young_cycle();
    }

    /// The property arena words from which a full collection that does not compact narrows
    /// live runs (`narrow_live_runs`).
    const NARROW_PASS_MIN_ARENA_WORDS: usize = 1 << 20;

    /// Moves each live plain object's ordinary run whose values all have a narrow form into a
    /// released range of the narrow class (`ValueArena::narrow_into_released`), at a full
    /// collection that does not compact: objects that grew after promotion.
    fn narrow_live_runs(&mut self) {
        let Self { object_records, properties, .. } = self;
        object_records.for_each_live_mut(|_, object| {
            if object.has_extras() {
                return;
            }
            let vector = object.properties();
            if vector.is_narrow() || vector.is_dictionary() || vector.has_inline_property_storage() || properties.holds_young_run(vector) {
                return;
            }
            if let Some(narrow) = properties.narrow_into_released(vector) {
                object.set_properties(narrow, properties);
            }
        });
    }

    /// Slide every live arena range down over released ones, in offset order.
    /// `objects` holds each live arena owner packed with its range offset.
    fn compact_property_arena(&mut self, slot_owners: Vec<u64>, direct_owners: Vec<(usize, Value)>) {
        if !self.properties.compaction_due() {
            return;
        }
        #[cfg(feature = "profile-memory")]
        let before = self.properties.stats();
        let owners = compaction_order(slot_owners, direct_owners);
        let mut target = 0;
        let Self { object_records, array_records, function_records, promise_records, iterator_records, slots, properties, narrow_runs, .. } = self;
        let mut space = PropertyOwners { object_records, array_records, function_records, promise_records, iterator_records, slots };
        for (_, owner) in owners {
            let object = space.object_mut(owner);
            let mut vector = object.properties();
            // A plain object's values may take a narrow run (`value_vec::NARROW_FORM`).
            let narrows = *narrow_runs && owner.direct_object_index().is_some() && !object.has_extras() && !object.is_array();
            target += properties.compact_vector(&mut vector, target, narrows);
            object.set_properties(vector, properties);
        }
        properties.finish_compaction(target);
        #[cfg(feature = "profile-memory")]
        if crate::trace::Channel::Memory.on() {
            let after = properties.stats();
            eprintln!(
                "{{\"kind\":\"quench-property-compaction\",\"values_before\":{},\"values_after\":{},\"capacity_before\":{},\"capacity_after\":{},\"free_ranges_before\":{},\"free_ranges_after\":{}}}",
                before.0, after.0, before.1, after.1, before.2, after.2
            );
        }
    }

    pub(crate) fn reset(&mut self) {
        self.abandon_cycle();
        self.reset_record_spaces();
        self.reset_accounting();
        self.reset_collection_state();
    }

    /// Empties the record spaces and the free lists over them: the discarded heap's cells.
    fn reset_record_spaces(&mut self) {
        self.slots.clear();
        self.object_records = ObjectRecordSpace::default();
        self.array_records = ArrayRecordSpace::default();
        self.function_records = FunctionRecordSpace::default();
        self.promise_records = PromiseRecordSpace::default();
        self.await_records = AwaitRecordSpace::default();
        self.iterator_records = IteratorRecordSpace::default();
        self.string_records = StringRecordSpace::default();
        self.small_strings = small_strings::SmallStringSpace::default();
        self.small_views = small_strings::SmallViews::default();
        self.small_templates = small_strings::SmallTemplates::default();
        self.clear_string_atoms();
        self.v128_records = V128RecordSpace::default();
        self.marks.clear();
        self.aged.clear();
        self.free_by_slab.clear();
        self.free_slabs.clear();
        self.bump_bits = 0;
        self.free_slots = 0;
        buffer_bytes::bump_view_epoch();
    }

    /// Restarts the byte, allocation, and pacing counters a fresh heap starts from.
    fn reset_accounting(&mut self) {
        self.external_bytes = 0;
        self.external_since_collection = 0;
        self.external_collection_budget = self.pacing().minimum_external_budget;
        self.payload_bytes = 0;
        self.allocated = 0;
        self.allocated_at_collection = 0;
        self.allocated_at_full = 0;
        self.reclaimable_program_bytes_since_full = 0;
        self.reclaimable_program_bytes_after_full = 0;
        self.live_after_full = 0;
        self.live_at_environment_trim = None;
        self.full_due = false;
        self.pacing_state = pacing::PacingState::default();
        self.live_weight_after_previous = 0;
        self.live_weight_after_full = 0;
        self.resident_at_full_start = 0;
        self.resident_after_full = 0;
        self.cycle_gate = concurrent_mark::CycleGate::default();
        self.rc_zero.clear();
        self.rc_candidates.clear();
        self.rc_suspects.clear();
        // The fresh record spaces keep counts as the old ones did.
        if self.rc_active {
            self.set_rc(true);
        }
    }

    /// Restarts the collection counters, the remembered sets and the weak tables of a fresh heap.
    fn reset_collection_state(&mut self) {
        self.weight_between_fulls = 0;
        self.old_live = 0;
        self.collections = 0;
        self.layout_id = Self::next_layout_id();
        self.peak_live = 0;
        self.peak_survivors = 0;
        self.max_threshold = 0;
        self.set_young_budget(self.pacing().minimum_young);
        self.properties.reset();
        self.remembered_marks.clear();
        self.remembered.clear();
        self.carried_owners.clear();
        self.young.clear();
        self.young_weak.clear();
        self.roots.clear(); self.folded = Default::default(); // the discarded heap's identities root nothing
        self.sparse_arrays = None;
        // The shared unit strings name cells of the discarded heap.
        self.ascii_unit_strings = None;
        self.collection_state = CollectionState::Idle;
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        { self.gc_profile = GcProfile::default(); }
        #[cfg(feature = "profile-memory")]
        { self.memory_profile = memory_profile::MemoryProfile::default(); }
    }
    /// Counts collections; a slot index names the same cell for as long as this is unchanged.
    #[inline(always)]
    pub(crate) fn collection_epoch(&self) -> u64 {
        self.collections
    }
    /// Charges payload a live cell grew outside `alloc` (array elements) toward the next
    /// collection, at the weight `alloc` gives a cell's own payload.
    pub(crate) fn charge_payload_growth(&mut self, bytes: usize) {
        self.payload_bytes = self.payload_bytes.saturating_add(bytes);
        let pending = self.uncharged_payload_bytes.saturating_add(bytes);
        let weight = pending / payload_unit();
        self.uncharged_payload_bytes = pending % payload_unit();
        self.allocated += weight;
    }
    /// Accounts new backing-store bytes; past the external budget a collection is due.
    pub(crate) fn charge_external_growth(&mut self, bytes: usize) {
        self.external_bytes += bytes;
        self.external_since_collection = self.external_since_collection.saturating_add(bytes);
        if self.external_since_collection >= self.external_collection_budget {
            self.request_collection();
        }
    }
    /// Charges text made on demand (`wtf16::take_materialized_text`) as payload allocation.
    /// The string operations that make it allocate or return through the places that call
    /// this (generic allocation, native and host calls, each collection); safepoints, which
    /// run far more often, never pay for it.
    #[inline(always)]
    pub(crate) fn charge_materialized_text(&mut self) {
        let text = crate::vm::wtf16::take_materialized_text();
        if text != crate::vm::wtf16::MaterializedText::default() {
            self.charge_text(text);
        }
    }

    #[cold]
    #[inline(never)]
    fn charge_text(&mut self, text: crate::vm::wtf16::MaterializedText) {
        // A flattened string's units join the payload ledger (`payload_bytes`), which its
        // sweep releases them from (`JsString::flat_unit_bytes`); text beside them is
        // allocation weight only, as no sweep releases it from the ledger.
        self.charge_payload_growth(text.units);
        self.allocated += text.beside / payload_unit();
    }

    /// Makes a collection due at the next safepoint, through a shared heap.
    pub(super) fn request_collection(&self) {
        self.collect_at.set(self.collect_at.get().min(self.allocated));
    }
    #[inline] pub fn should_collect(&self) -> bool {
        self.allocated >= self.collect_at.get()
    }
    /// The weight charged since the last collection.
    fn allocated_since_collection(&self) -> usize {
        self.allocated - self.allocated_at_collection
    }
    /// The weight charged since the last full collection.
    fn allocated_since_full(&self) -> usize {
        self.allocated - self.allocated_at_full
    }
    /// Makes the next collection due once `budget` more weight than at the last collection
    /// has been charged, and records the largest budget planned (`Heap::stats`).
    pub(super) fn set_young_budget(&mut self, budget: usize) {
        self.collect_at.set(self.allocated_at_collection.saturating_add(budget));
        self.max_threshold = self.max_threshold.max(budget);
    }
    /// The host contexts of promises freed since the last call.
    pub(crate) fn take_abandoned_contexts(&mut self) -> AbandonedHostContexts {
        std::mem::take(&mut self.abandoned_contexts)
    }
    pub(crate) fn should_full_collect(&self) -> bool {
        // Under counts, minor collections free old garbage: the backup trace runs when
        // `Heap::rc_plan` finds the live records grown, not at an allocation cap.
        self.full_due
            || !self.rc_active && self.pacing().allocation_cap_reached(
                self.live_after_full,
                self.allocated_since_full(),
                self.pacing_state.cap_backoff,
            )
            || self.pacing().program_growth_reached(
                self.reclaimable_program_bytes_after_full,
                self.reclaimable_program_bytes_since_full,
            )
            || self.pacing().arena_growth_reached(self.properties.settled_values(), self.properties.occupied_values())
    }
    /// Charges a reclaimable program (dynamic code: `eval`, `new Function`, scripts) of
    /// `bytes` (`ProgramFootprint`) as old growth of the programs the last full collection
    /// kept: only a full collection finds a program unreferenced and reclaims it, and minor
    /// collections, which free the heap garbage that made it, never would. Without it, a loop
    /// of `new Function` under Throughput ran two full collections in 80K programs and held
    /// every program. Measured against the programs that stay (`ProgramStore::
    /// settled_reclaimable_bytes`), a large program a process keeps raises its own threshold,
    /// while a stream of short-lived ones keeps the minimum.
    pub(crate) fn charge_reclaimable_program(&mut self, bytes: usize) {
        /// The least program footprint since the last full collection that makes one due.
        const MINIMUM_PROGRAM_GROWTH: usize = 256 << 10;
        self.reclaimable_program_bytes_since_full = self.reclaimable_program_bytes_since_full.saturating_add(bytes);
        // Programs die without allocating on the heap: once as much program as the last full
        // collection kept has come since, a full collection is due by itself, or a code-load
        // loop keeps every dead program until heap allocation happens to make one
        // (octane-code-load: no full collection in a run). Against what was kept, programs
        // that all stay (one live function per eval) make it due ever less often.
        if self.reclaimable_program_bytes_since_full >= self.reclaimable_program_bytes_after_full.max(MINIMUM_PROGRAM_GROWTH) {
            self.full_due = true;
            self.request_collection();
        }
    }
    /// Records the footprints of the reclaimable programs that stay, after a full collection.
    pub(crate) fn set_reclaimable_programs_after_full(&mut self, bytes: usize) {
        self.reclaimable_program_bytes_after_full = bytes;
    }
    /// Every collection may be minor, so the legacy young list and barrier are always kept.
    #[inline]
    fn tracks_young_cells(&self) -> bool {
        true
    }
    pub(crate) fn is_old(&self, value: Value) -> bool {
        match value.heap_reference() {
            Some(HeapReference::LegacySlot(index)) => {
                self.get(value).is_some() && Self::marked(&self.marks, index as usize)
            }
            Some(_) => mark_bits!(self).is_marked(value),
            None => false,
        }
    }
    /// Make a collection due at the next safepoint.
    #[cfg(test)]
    pub(crate) fn make_collection_due_for_test(&mut self) {
        self.collect_at.set(self.allocated);
    }
    /// Leave room until the next charged payload growth, for native recovery tests.
    #[cfg(all(test, not(quench_profiled)))]
    pub(crate) fn make_collection_due_after_growth_for_test(&mut self) {
        self.collect_at.set(self.allocated.saturating_add(1));
        self.uncharged_payload_bytes = 0;
    }
    pub(crate) fn root(&mut self, value: Value) -> RootId {
        self.roots.insert(value)
    }
    pub(crate) fn update_root(&mut self, root: RootId, value: Value) -> bool {
        self.roots.update(root, value)
    }
    pub(crate) fn release_root(&mut self, root: RootId) -> bool {
        self.roots.remove(root)
    }
    /// A root that lasts until its native scope ends (`RootTable::insert_scoped`).
    pub(crate) fn root_scoped(&mut self, value: Value) -> RootId {
        self.roots.insert_scoped(value)
    }
    pub(crate) fn scoped_root_height(&self) -> usize {
        self.roots.scoped_height()
    }
    pub(crate) fn release_root_scope(&mut self, height: usize) {
        self.roots.release_scope(height);
    }
    pub(crate) fn release_root_scope_keeping(&mut self, height: usize, keep: &[RootId]) {
        self.roots.release_scope_keeping(height, keep);
    }
    #[cfg(test)]
    pub fn collect(&mut self, roots: impl IntoIterator<Item = Value>) -> Vec<(Value, Value)> {
        self.collect_with_object_roots(roots, |_, _, _, _, _| {}, None, None, true)
    }
    /// Whether a collection requested as `full` traces and sweeps the whole heap. Records
    /// and legacy cells share one generation rule: survivors of their `promotion_age`-th
    /// minor collection, or of a full one, stay marked (old), the write barriers log old
    /// owners they write, and a minor traces roots, that log and the old owners of what an
    /// earlier minor left young (`carried_owners`) only.
    /// Whether this full collection traces closure environments through their reads
    /// (`live_at_environment_trim`).
    fn environment_trim_due(&self) -> bool {
        self.live_at_environment_trim.is_none_or(|trimmed| {
            self.live_after_full >= trimmed.saturating_add(ENVIRONMENT_TRIM_GROWTH.of(trimmed))
        })
    }
    /// After a sweep: a prototype it freed may have its record reused by an object no proof
    /// names, so the proofs made under the current `prototype_layout_epoch` retire. A dead
    /// object record's id is withheld from reuse instead, until `WITHHELD_PROTOTYPE_IDS` are;
    /// a sweep that freed no other prototype keeps the proofs.
    fn retire_dead_prototypes(&mut self) {
        let mut died_subjects = Vec::new();
        let mut died_unrecorded = false;
        let mut withheld = 0;
        let mut prototypes = std::mem::take(&mut self.prototypes);
        prototypes.retain(|&(prototype, subject)| {
            let live = self.is_live(prototype);
            if !live {
                died_subjects.push(subject);
                // A dead object record's id is withheld instead: no object takes its
                // Value, so no proof can name another object by it, as a class
                // declared in a hot function leaves one dead prototype per call.
                match prototype.direct_object_index() {
                    Some(id) => withheld = self.object_records.withhold(id),
                    None => died_unrecorded = true,
                }
            }
            live
        });
        for subject in died_subjects {
            self.dependencies.release(subject);
        }
        self.prototypes = prototypes;
        let retire = died_unrecorded || withheld > WITHHELD_PROTOTYPE_IDS;
        if retire {
            // Every proof made under the old epoch retires, so the withheld ids are free again.
            advance_prototype_layout_epoch();
            self.object_records.release_withheld();
        }
    }

    pub(crate) fn collection_is_full(&self, full: bool) -> bool {
        // A collection while a cycle runs is its remark, a full one.
        full || self.cycle.is_some()
    }

    /// `collection_is_full`, noting a full collection's start for the old-generation sizing
    /// (`GenerationalPacing::old_sizing`).
    fn start_collection(&mut self, full: bool) -> bool {
        self.retire_bump();
        let full = self.collection_is_full(full);
        if full {
            self.pacing_state.old_generation.start_full();
            self.resident_at_full_start = self.occupied_resident_bytes();
        }
        full
    }

    /// The emptied buffer a collection's roots go into, kept from the last collection.
    pub(crate) fn take_mark_stack(&mut self) -> Vec<Value> { std::mem::take(&mut self.mark_stack) }

    /// Roots `code`'s folded identities at every collection while it is published.
    pub(crate) fn hold_folded_roots(&mut self, code: &std::rc::Rc<dyn crate::folded_roots::RootedCode>) {
        self.folded.register(code);
    }

    /// Appends the identities every published code of this heap embeds to a collection's `roots`.
    pub(crate) fn extend_folded_roots(&mut self, roots: &mut Vec<Value>) { self.folded.extend_roots(roots); }

    /// Marks everything `work` reaches, with the closure environments it reaches through
    /// `environments` and the weak map values through `ephemerons`. With `parallel`, work
    /// larger than its `round_minimum` is marked in parallel rounds (`mark_work_in_rounds`);
    /// smaller work is marked on this thread, value by value (`drain_work`), as is all of it
    /// without. Either way marking ends when no work and no environment edge is left.
    // Kept out of `collect_with_object_roots`: inlined there, the visit loop shares registers
    // with the whole collection, and cachegrind counted spills in every visit.
    /// `mark_work` with a parallel mark: drains work until a round is due, runs the round,
    /// and visits the environments its workers left before draining again.
    // Kept out of line: `collect_with_object_roots` then inlines one serial visit
    // (`mark_value`), and with it the children walks, as before parallel rounds existed.
    #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
    pub(crate) fn record_vm_gc_phase(&mut self, phase: usize, started: std::time::Instant) {
        self.gc_profile.vm_phase_nanos[phase] += started.elapsed().as_nanos() as u64;
    }
    #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
    pub(crate) const fn gc_profile(&self) -> GcProfile {
        self.gc_profile
    }
    pub(crate) fn sparse_get(&self, array: Value, index: usize) -> Option<Value> {
        self.sparse_arrays
            .as_ref()?
            .get(&array)?
            .values
            .get(&index)
            .copied()
    }
    /// The length an array's sparse elements give it, if it has any. Inline where no array
    /// has any, which every `length` read meets first.
    #[inline(always)]
    pub(crate) fn sparse_length(&self, array: Value) -> Option<usize> {
        let arrays = self.sparse_arrays.as_ref()?;
        Self::sparse_entry_length(arrays, array)
    }
    #[inline(never)]
    fn sparse_entry_length(arrays: &FxHashMap<Value, SparseElements>, array: Value) -> Option<usize> {
        arrays.get(&array).map(|elements| elements.length)
    }
    /// The write barrier for an edge stored outside `owner`'s own storage (sparse elements,
    /// owner-attached VM tables): `owner` is logged as if it had been written, or, while a
    /// concurrent cycle marks, a record owner is snapshotted (`RecordSpace::barrier`).
    pub(crate) fn write_barrier(&mut self, owner: Value) {
        match owner.heap_reference() {
            Some(HeapReference::ObjectRecord(record_id)) => self.object_records.barrier(record_id, &self.properties),
            Some(HeapReference::ArrayRecord(record_id)) => self.array_records.barrier(record_id, &self.properties),
            Some(HeapReference::FunctionRecord(record_id)) => {
                self.function_records.barrier(record_id, &self.properties)
            }
            Some(HeapReference::PromiseRecord(record_id)) => self.promise_records.barrier(record_id, &self.properties),
            Some(HeapReference::AwaitRecord(record_id)) => self.await_records.barrier(record_id, &self.properties),
            Some(HeapReference::IteratorRecord(record_id)) => {
                self.iterator_records.barrier(record_id, &self.properties)
            }
            Some(HeapReference::LegacySlot(index)) => self.remember(index as usize),
            // Strings and vectors are immutable and hold no edges, so nothing is ever stored
            // into them.
            Some(HeapReference::StringRecord(_) | HeapReference::V128Record(_)) | None => {}
        }
    }
    pub(crate) fn sparse_set(&mut self, array: Value, index: usize, value: Value) {
        self.note_prototype_elements(array);
        self.write_barrier(array);
        if let Some(object) = self.object_mut(array) {
            object.mark_sparse_elements();
        }
        let arrays = self
            .sparse_arrays
            .get_or_insert_with(|| Box::new(FxHashMap::default()));
        let elements = arrays.entry(array).or_default();
        elements.values.insert(index, value);
        elements.length = elements.length.max(index.saturating_add(1));
        let (length, sparse_count) = (elements.length, elements.values.len());
        // Dense storage of the whole length takes no more words than the elements hold. Holes
        // in the dense prefix are not elements; counting them would promote an array whose
        // writes only grow its length, doubling its storage at every far write.
        let fills = |count: usize| dense_storage_fits(count.saturating_add(sparse_count), length);
        // Counting the dense prefix's elements is linear, so it runs as the sparse count
        // doubles: a promotion comes at most twice as late, and a write stays O(1) amortized.
        let dense = self.array_elements(array);
        let dense_enough = sparse_count.is_power_of_two()
            && fills(dense.map_or(0, |dense| dense.len()))
            && fills(dense.map_or(0, |dense| dense.iter().filter(|value| !value.is_deleted()).count()));
        if length <= MAX_DENSE_ARRAY_HOLE_LENGTH || dense_enough {
            self.materialize_sparse_array(array, length);
        }
    }
    pub(crate) fn sparse_set_length(&mut self, array: Value, length: usize) {
        self.note_prototype_elements(array);
        let Some(elements) = self.array_elements(array) else {
            return;
        };
        let dense_length = elements.len();
        let sparse_length = self.sparse_length(array);
        if sparse_length.is_none() && length == dense_length {
            return;
        }
        if length <= MAX_DENSE_ARRAY_HOLE_LENGTH || length <= dense_length {
            self.materialize_sparse_array(array, length);
            return;
        }
        if let Some(object) = self.object_mut(array) {
            object.mark_sparse_elements();
        }
        let arrays = self
            .sparse_arrays
            .get_or_insert_with(|| Box::new(FxHashMap::default()));
        let elements = arrays.entry(array).or_default();
        if length < elements.length {
            elements.values.retain(|index, _| *index < length);
        }
        elements.length = length;
    }
    fn materialize_sparse_array(&mut self, array: Value, length: usize) {
        let sparse = self
            .sparse_arrays
            .as_mut()
            .and_then(|arrays| arrays.remove(&array));
        if let Some(arrays) = &mut self.sparse_arrays
            && arrays.is_empty()
        {
            self.sparse_arrays = None;
        }
        self.grow_array_elements(array, |elements| {
            let dense_length = elements.len();
            elements.resize(length, Value::DELETED);
            for (index, value) in sparse.into_iter().flat_map(|elements| elements.values) {
                if index < length && (index >= dense_length || elements[index].is_deleted()) {
                    elements.set(index, value);
                }
            }
        });
    }
    #[inline(always)] pub(crate) fn property_get(&self, object: &Object, slot: usize) -> Option<Value> {
        let value = match object.properties().has_inline_property_storage() {
            true if slot < INLINE_PROPERTY_COUNT => object.inline_property(slot),
            true => None,
            false => self.properties.get(object.properties(), slot),
        }?;
        (!value.is_deleted()).then_some(value)
    }
    /// `property_get_unchecked` that leaves a dictionary's storage to the caller's slower path
    /// (`ValueArena::get_shaped`), so a cache hit makes no call.
    ///
    /// # Safety
    /// As for `property_get_unchecked`.
    #[inline(always)]
    pub(crate) unsafe fn property_get_shaped(&self, object: &Object, slot: usize) -> Option<Value> {
        // As `property_get_flat`, for an object outside dictionary storage.
        if let Some(start) = object.plain_shaped_start() {
            // SAFETY: the caller proves that the property slot exists.
            return Some(unsafe { self.properties.get_flat_at(start, slot) });
        }
        if let Some(value) = object.inline_property(slot) {
            Some(value)
        } else {
            // SAFETY: the caller proves that the property slot exists.
            unsafe { self.properties.get_shaped(object.properties(), slot) }
        }
    }
    /// `property_get_unchecked` of an object outside dictionary storage, as an exact shape
    /// match proves (`Object::exact_shape`).
    ///
    /// # Safety
    /// The slot exists and the object is not in dictionary storage.
    #[inline(always)]
    pub(crate) unsafe fn property_get_flat(&self, object: &Object, slot: usize) -> Value {
        // SAFETY: the caller proves the exact slot exists.
        unsafe { read_exact_slot(object, &self.properties, slot) }
    }
    /// The own field a hint (`shape`, `slot`) proves of `object`, `None` on another exact shape.
    /// # Safety
    /// As for `property_get_flat`, where the shape matches.
    #[inline(always)]
    pub(crate) unsafe fn own_field_hit(&self, object: &Object, shape: u32, slot: u16) -> Option<Value> {
        // The common form first, as `GuardShape, LoadSlot` reads it: a plain object's arena run,
        // whose start is the payload itself, so neither the extras tag nor the start mask is
        // decoded. Packed inline values and extras take the staged reading.
        // An exact shape has no form bits, so the whole auxiliary word equals it only in the
        // arena form, whose payload is the start (`Object::formed_plain_start`).
        if let Some(start) = object.formed_plain_start(shape) {
            // SAFETY: the caller's: the shape matched, so its run holds `slot`.
            let value = unsafe { *self.properties.values_base().add(start + usize::from(slot)) };
            debug_assert_eq!(Some(value), unsafe { crate::staged::concrete_own_field_hit(object, self.properties.values_base(), shape, slot) });
            return Some(value);
        }
        unsafe { crate::staged::concrete_own_field_hit(object, self.properties.values_base(), shape, slot) }
    }
    /// `own_field_hit` through a lane record's own-field hint (`Object::hint_shape`): `None`
    /// on another exact shape or storage form.
    /// # Safety
    /// As for `own_field_hit`.
    #[inline(always)]
    pub(crate) unsafe fn own_field_hint_hit<const FORM: u32>(&self, object: &Object, hint: u32, slot: u16) -> Option<Value> {
        unsafe { crate::staged::concrete_hinted_own_field_hit::<FORM>(object, self.properties.values_base(), hint, slot) }
    }
    /// `property_get_flat` of a prototype holder, whose values usually live behind extras
    /// (`Object::extras_storage_start`), read without first testing the forms a holder
    /// seldom has.
    ///
    /// # Safety
    /// As for `property_get_flat`.
    #[inline(always)]
    pub(crate) unsafe fn holder_property_get_flat(&self, holder: &Object, slot: usize) -> Value {
        if let Some(start) = holder.extras_storage_start() {
            debug_assert!(!holder.properties().is_dictionary(), "the caller matched an exact shape");
            // SAFETY: the caller proves the slot exists outside dictionary storage.
            return unsafe { self.properties.get_flat_at(start, slot) };
        }
        // SAFETY: as the caller guarantees.
        unsafe { self.property_get_flat(holder, slot) }
    }
    /// The arena's value base, which the interpreter's staged reads address runs from
    /// (`staged::Table::PropertyValues`).
    #[cfg(test)]
    pub(crate) fn property_values_base(&self) -> *const Value {
        self.properties.values_base()
    }
    #[inline(always)]
    pub(crate) unsafe fn property_get_unchecked(&self, object: &Object, slot: usize) -> Value {
        if let Some(value) = object.inline_property(slot) {
            value
        } else {
            // SAFETY: the caller proves that the property slot exists.
            unsafe { self.properties.get_unchecked(object.properties(), slot) }
        }
    }

    /// Pointer to an initialized flat property slot. Inline and dictionary-backed objects
    /// decline because their slots require representation conversion or a table update.
    ///
    /// # Safety
    /// The object's exact shape and slot were guarded by the caller. The caller must keep the
    /// object rooted and prevent allocation, collection, reentry, storage changes, or type
    /// re-admission while using the pointer. Every stored value must fit `writes`. Direct stores must not install
    /// a heap reference without the write barrier.
    #[inline(always)]
    pub(crate) unsafe fn flat_property_mut_ptr_unchecked(
        &mut self,
        object: Value,
        slot: usize,
        writes: crate::typed_ir::Type,
    ) -> Option<*mut Value> {
        // A raw writer passes no snapshot barrier, so none is handed out while a cycle marks.
        if self.satb_active {
            return None;
        }
        let data = self.object(object)?;
        let vector = data.properties();
        let shape = data.shape();
        if vector.has_inline_property_storage() || vector.is_dictionary() || vector.is_narrow() || slot >= self.properties.len(vector) {
            return None;
        }
        // A widening can retire an active caller. Decline before exposing a raw
        // writer so its existing call/OSR miss leaves before any guest effect.
        if !self.admit_slot_writes(shape, slot, writes) { return None; }
        // SAFETY: shape/slot and storage lifetime are the caller's obligations above.
        Some(unsafe { self.properties.flat_mut_ptr_unchecked(vector, slot) })
    }

    /// `property_set` of each of `values` at slot `0..INLINE_PROPERTY_COUNT` of `object`,
    /// whose shape has those slots, with one lookup and one write barrier: one payload store
    /// when they pack there (`Object::store_packed_inline_values`), the arena's slots when
    /// they live there, else `property_set` of each.
    pub(crate) fn set_leading_properties(&mut self, object: Value, values: [Value; INLINE_PROPERTY_COUNT]) {
        for (slot, value) in values.into_iter().enumerate() {
            self.note_value_slot(object, slot, value);
        }
        // The barrier remembers the owner of any reference stored; one test covers each.
        let stored = values.into_iter().find(|value| value.heap_index().is_some()).unwrap_or(Value::UNDEFINED);
        let data = self.object_store_mut(object, stored).expect("property owner is an object");
        if data.store_packed_inline_values(values) {
            return;
        }
        let vector = data.properties();
        if vector.has_inline_property_storage() || vector.is_narrow() {
            for (slot, value) in values.into_iter().enumerate() {
                self.property_set(object, slot, value);
            }
            return;
        }
        for (slot, value) in values.into_iter().enumerate() {
            self.properties.set(vector, slot, value);
        }
    }

    /// Lets promotion and compaction narrow plain objects' property runs: a run whose code
    /// only the interpreter executes (`ExecutionMode::Interpreted`).
    pub(crate) fn set_narrow_runs(&mut self, narrow: bool) {
        self.narrow_runs = narrow;
    }

    /// Observes the value before writing the named object's slot.
    #[inline(always)]
    fn note_value_slot(&mut self, object: Value, slot: usize, value: Value) {
        if !self.properties.tracks() { return; }
        if let Some(data) = self.object(object) {
            let shape = data.exact_shape();
            if shape == data.shape() && slot < self.properties.shape_length(shape) {
                self.properties.note_slot_value(shape, slot, value);
            }
        }
        self.settle_widened();
    }

    /// `property_set` of a primitive `value` over a primitive in `slot` of a cell object
    /// (`HeapReference::LegacySlot`, as a RegExp's `lastIndex`): with no reference stored or
    /// overwritten no barrier applies, so the cell is found by one slot read rather than the
    /// record kinds' tests, twice. The caller proves both values primitive.
    pub(crate) fn primitive_cell_property_set(&mut self, object: Value, slot: usize, value: Value) {
        debug_assert!(value.heap_index().is_none());
        let Some(HeapReference::LegacySlot(index)) = object.heap_reference() else {
            return self.property_set(object, slot, value);
        };
        let Self { slots, properties, .. } = self;
        let Some(data) = slots.get_mut(index as usize).and_then(|entry| entry.cell.as_mut()).and_then(Cell::object_mut) else {
            return self.property_set(object, slot, value);
        };
        debug_assert!(data.properties().has_inline_property_storage() || properties.get(data.properties(), slot).is_none_or(|old| old.heap_index().is_none()));
        if properties.tracks() {
            let shape = data.exact_shape();
            if shape == data.shape() && slot < properties.shape_length(shape) {
                properties.note_slot_value(shape, slot, value);
            }
        }
        data.note_value_write(slot);
        let vector = data.properties();
        if !vector.has_inline_property_storage() {
            properties.set(vector, slot, value);
        } else if !data.store_inline_narrow(slot, value) {
            self.property_set(object, slot, value);
        }
    }

    pub(crate) fn property_set(&mut self, object: Value, slot: usize, value: Value) {
        self.note_value_slot(object, slot, value);
        let vector = {
            let data = self
                .object_store_mut(object, value)
                .expect("property owner is an object");
            data.note_value_write(slot);
            let vector = data.properties();
            if !vector.has_inline_property_storage() {
                Some(vector)
            } else if data.store_inline_narrow(slot, value) {
                return;
            } else {
                None
            }
        };
        match vector {
            Some(vector) if vector.is_narrow() => self.narrow_run_set(object, vector, slot, value),
            Some(vector) => self.properties.set(vector, slot, value),
            None => self.widen_inline_property(object, slot, value),
        }
    }

    /// Stores `value` at `slot` of `object`'s narrow run `vector`: in place when the value has a
    /// narrow form, else into the run widened first (`object_mut`). The caller passed the
    /// write barrier.
    #[inline(never)]
    fn narrow_run_set(&mut self, object: Value, vector: ValueVec, slot: usize, value: Value) {
        assert!(slot < self.properties.len(vector), "a property store names a slot of its owner");
        if let Some(narrow) = value.to_narrow() {
            // SAFETY: the slot exists (above).
            unsafe { self.properties.narrow_set(vector, slot, narrow) };
            return;
        }
        let vector = self.object_mut(object).expect("property owner is an object").properties();
        self.properties.set(vector, slot, value);
    }

    /// Stores `value` at inline `slot` of `object`, whose payload word cannot hold it: its
    /// values move to the arena. The caller has applied the write barrier.
    #[inline(always)]
    pub(crate) unsafe fn property_set_unchecked(
        &mut self,
        object: Value,
        slot: usize,
        value: Value,
    ) {
        self.note_value_slot(object, slot, value);
        let vector = {
            let data = self
                .object_store_mut(object, value)
                .expect("property owner is an object");
            data.note_value_write(slot);
            let vector = data.properties();
            if !vector.has_inline_property_storage() {
                Some(vector)
            } else if data.store_inline_narrow(slot, value) {
                return;
            } else {
                None
            }
        };
        match vector {
            Some(vector) if vector.is_narrow() => self.narrow_run_set(object, vector, slot, value),
            // SAFETY: the caller proves that the property slot exists.
            Some(vector) => unsafe { self.properties.set_unchecked(vector, slot, value) },
            None => self.widen_inline_property(object, slot, value),
        }
    }
    /// `property_set_unchecked` for the direct object `record_id` names, decoded once: its
    /// values are inline or a flat arena run. Without `LOGS`, `false` leaves a store the
    /// write barrier logs to the caller.
    ///
    /// # Safety
    /// `record_id` names a live object record outside dictionary storage (its exact shape
    /// matched a recorded one) holding a property at `slot`.
    #[inline(always)]
    pub(crate) unsafe fn record_property_set_flat<const LOGS: bool>(
        &mut self,
        record_id: u32,
        slot: usize,
        value: Value,
        layout: PropertyStoreLayout,
    ) -> bool {
        // Without `LOGS`, a store the barrier would log is left to the caller, so this one
        // makes no call; the barrier does nothing for any other.
        if !LOGS && self.object_records.store_logs(record_id, value) {
            return false;
        }
        // SAFETY: the caller proves the record live; `&mut self` is exclusive.
        let data = unsafe {
            if LOGS {
                self.object_records.get_live_mut_storing(record_id, value, &self.properties)
            } else {
                self.object_records.get_live_mut_unbarriered(record_id)
            }
        };
        let stored = store_exact_slot(data, &mut self.properties, slot, value, layout);
        self.settle_widened();
        stored
    }

    /// Whether the arena keeps slot representations, which observe every store (`ValueArena::tracks`).
    pub(crate) fn tracks_slot_representations(&self) -> bool {
        self.properties.tracks()
    }
    /// `record_property_set_flat` with the write barrier for an object in a slot cell (a
    /// RegExp, Map, Date or any other object without a record) whose exact shape is `shape`:
    /// one owner lookup; `false` leaves any other value to the caller unchanged.
    ///
    /// # Safety
    /// `shape` is a shape outside dictionary storage holding a property at `slot`.
    #[inline(always)]
    pub(crate) unsafe fn cell_property_set_exact(&mut self, object: Value, shape: u32, slot: usize, value: Value, layout: PropertyStoreLayout) -> bool {
        let Some(HeapReference::LegacySlot(index)) = object.heap_reference() else {
            return false;
        };
        let index = index as usize;
        let Self { slots, properties, .. } = &mut *self;
        let Some(data) = slots.get_mut(index).and_then(|slot| slot.cell.as_mut()).and_then(Cell::object_mut) else {
            return false;
        };
        if data.exact_shape() != shape { // Carries the exotic bit: a match proves it ordinary.
            return false;
        }
        let stored = store_exact_slot(data, properties, slot, value, layout);
        self.settle_widened();
        if !stored {
            return false;
        }
        if value.heap_index().is_some() {
            self.remember(index);
        }
        true
    }

    /// Appends `value` as the next property of `object`, which moves to `shape`, whose
    /// storage length is one more than the object's: inline while the values fit, otherwise
    /// in the arena, into which the add that outgrows the inline values moves them. One owner
    /// lookup, which passes the write barrier only for a stored reference; the arena borrow
    /// is disjoint from the owner.
    #[inline(always)]
    pub(crate) fn property_add(&mut self, object: Value, value: Value, shape: u32) {
        let _slot = if let Some(record_id) = object.direct_object_index() {
            let Self {
                object_records,
                properties,
                ..
            } = &mut *self;
            // SAFETY: direct-object Values name live records; `&mut self` is exclusive.
            let data = unsafe { object_records.get_live_mut_storing(record_id, value, properties) };
            let slot = properties.len(data.properties());
            append_property(data, properties, slot, value, shape);
            slot
        } else {
            let (data, properties) = self
                .object_and_arena_mut(object)
                .expect("property owner is an object");
            let slot = properties.len(data.properties());
            append_property(data, properties, slot, value, shape);
            slot
        };
        self.settle_widened();
        #[cfg(feature = "profile-memory")]
        self.note_object_slots(object, _slot + 1);
    }

    /// `property_add` to a direct object's record at `slot`, which the caller proved its
    /// storage length (a cached add's recorded slot), without deriving it again.
    #[inline(always)]
    pub(crate) fn record_property_add_at(&mut self, object: Value, slot: usize, value: Value, shape: u32) {
        let Some(record_id) = object.direct_object_index() else {
            return self.property_add(object, value, shape);
        };
        let Self { object_records, properties, .. } = &mut *self;
        // SAFETY: direct-object Values name live records; `&mut self` is exclusive.
        let data = unsafe { object_records.get_live_mut_storing(record_id, value, properties) };
        debug_assert_eq!(slot, properties.len(data.properties()));
        append_property(data, properties, slot, value, shape);
        self.settle_widened();
        #[cfg(feature = "profile-memory")]
        self.note_object_slots(object, slot + 1);
    }

    /// `record_property_add_at` out of line, for an add whose storage moves or whose store
    /// the write barrier logs, so a caller's plain appends keep their code call-free.
    #[inline(never)]
    pub(crate) fn record_property_add_moving(&mut self, object: Value, slot: usize, value: Value, shape: u32) {
        self.record_property_add_at(object, slot, value, shape);
    }

    /// `record_property_add_at` of an add the record admits as a plain append
    /// (`Object::admits_plain_append`): the store into storage that does not move. A store
    /// the write barrier would log is left to the caller (`false`, nothing changed), as for
    /// `record_property_set_flat`, so this one makes no call.
    #[inline(always)]
    pub(crate) fn record_property_append_plain(&mut self, record_id: u32, slot: usize, value: Value, shape: u32) -> bool {
        if self.object_records.store_logs(record_id, value) {
            return false;
        }
        let Self { object_records, properties, .. } = &mut *self;
        // SAFETY: the caller read the record from a direct-object Value; `&mut self` is
        // exclusive, and the barrier logs nothing for this store.
        let data = unsafe { object_records.get_live_mut_unbarriered(record_id) };
        data.append_plain(properties, slot, value, shape);
        self.settle_widened();
        #[cfg(feature = "profile-memory")]
        self.note_object_slots(Value::direct_object(record_id), slot + 1);
        true
    }

    #[cfg(feature = "profile-memory")]
    pub(crate) fn set_memory_allocation_site(&mut self, program: u32, function: u32, pc: usize) {
        self.memory_profile
            .set_allocation_site(memory_profile::AllocationSite {
                program,
                function,
                pc,
            });
    }

    #[cfg(feature = "profile-memory")]
    fn profile_allocation(&mut self, identity: Value) {
        let Self {
            object_records,
            array_records,
            function_records,
            promise_records,
            iterator_records,
            slots,
            properties,
            memory_profile,
            ..
        } = self;
        if let Some(record_id) = identity.direct_object_index() {
            // SAFETY: called immediately after insertion into the direct space.
            let object = unsafe { object_records.get_live(record_id) };
            memory_profile.allocated_object(identity, object, properties.len(object.properties()));
            return;
        }
        if let Some(record_id) = identity.direct_array_index() {
            // SAFETY: profile runs immediately after inserting this array.
            let array = unsafe { array_records.get_live(record_id) };
            memory_profile.allocated_array(identity, array, properties.len(array.properties()));
            return;
        }
        if let Some(record_id) = identity.direct_function_index() {
            // SAFETY: profile runs immediately after inserting this function.
            let function = unsafe { function_records.get_live(record_id) };
            let slots = properties.len(function.object.properties());
            memory_profile.allocated_function(identity, function, slots);
            return;
        }
        if let Some(record_id) = identity.direct_promise_index() {
            // SAFETY: profile runs immediately after inserting this promise.
            let promise = unsafe { promise_records.get_live(record_id) };
            let slots = properties.len(promise.object.properties());
            memory_profile.allocated_promise(identity, promise, slots);
            return;
        }
        if let Some(record_id) = identity.direct_iterator_index() {
            // SAFETY: profile runs immediately after inserting this iterator.
            let iterator = unsafe { iterator_records.get_live(record_id) };
            let slots = properties.len(iterator.object.properties());
            memory_profile.allocated_iterator(identity, iterator, slots);
            return;
        }
        let index = identity
            .legacy_heap_index()
            .expect("legacy allocation has a slot index") as usize;
        let cell = slots
            .get_mut(index)
            .and_then(|slot| slot.cell.as_ref())
            .expect("allocated slot contains its Cell");
        let object_slots = cell
            .object()
            .map(|object| properties.len(object.properties()));
        memory_profile.allocated(identity, cell, object_slots);
    }

    #[cfg(feature = "profile-memory")]
    pub(crate) fn note_object_slots(&mut self, object: Value, slots: usize) {
        self.memory_profile.object_slots_changed(object, slots);
    }
}
#[cfg(test)]
mod tests;

/// Bits per radix pass when ordering property owners by arena offset. Each
/// pass counts into a table of `2^bits` buckets, which every compacting
/// collection allocates; a byte keeps it small.
#[cfg(test)]
const OFFSET_RADIX_BITS: u32 = u8::BITS;
/// An owner key keeps the arena offset in the high half and the slot index in the low half.
const OWNER_KEY_HALF_BITS: u32 = u32::BITS;
/// Radix passes that order the whole offset half.
#[cfg(test)]
const OFFSET_RADIX_PASSES: u32 = OWNER_KEY_HALF_BITS / OFFSET_RADIX_BITS;

fn pack_offset_owner(offset: usize, owner: usize) -> u64 {
    debug_assert!(offset <= u32::MAX as usize && owner <= u32::MAX as usize);
    ((offset as u64) << OWNER_KEY_HALF_BITS) | owner as u64
}

fn owner_of_packed(key: u64) -> usize {
    (key & u64::from(u32::MAX)) as usize
}

/// Orders packed owner keys by arena offset in linear time. Offsets are distinct arena
/// positions, so stable passes over the offset half give a full ordering.
#[cfg(test)]
fn sort_by_unique_offset(keys: &mut Vec<u64>) {
    let buckets = 1usize << OFFSET_RADIX_BITS;
    let mask = (buckets - 1) as u64;
    let mut scratch = vec![0; keys.len()];
    let mut counts = vec![0usize; buckets + 1];
    for pass in 0..OFFSET_RADIX_PASSES {
        let shift = OWNER_KEY_HALF_BITS + pass * OFFSET_RADIX_BITS;
        counts.fill(0);
        for key in keys.iter() {
            counts[((key >> shift) & mask) as usize + 1] += 1;
        }
        for bucket in 0..buckets {
            counts[bucket + 1] += counts[bucket];
        }
        for key in keys.iter() {
            let bucket = ((key >> shift) & mask) as usize;
            scratch[counts[bucket]] = *key;
            counts[bucket] += 1;
        }
        std::mem::swap(keys, &mut scratch);
    }
}

/// Lists a young survivor's run for `Heap::evacuate_young_runs`, out of line: survivors are
/// few, and the sweep's visitor stays small enough to inline.
#[inline(never)]
fn note_young_owner(owners: &mut Vec<(usize, YoungOwner)>, start: usize, owner: YoungOwner) {
    owners.push((start, owner));
}

/// The record that owns a young run (`ValueArena::young_run_padded`): a direct object, or a
/// function, whose own properties a recorded closure allocates finished.
#[derive(Clone, Copy)]
enum YoungOwner {
    Object(RecordId),
    Function(RecordId),
}


/// `Heap::property_add` on the owner's storage: writes `value` at `slot`, the owner's
/// storage length, and moves the owner to `shape`.
#[inline(always)]
fn append_property(data: &mut Object, properties: &mut ValueArena, slot: usize, value: Value, shape: u32) {
    properties.note_shape_value(shape, slot, value);
    let mut vector = data.properties();
    if vector.is_narrow() {
        // An add grows the run, which only a wide run does (`ValueArena::push`).
        vector = properties.widen(vector);
        data.set_properties(vector, properties);
    }
    if !vector.has_inline_property_storage() {
        let before = vector.into_parts();
        properties.push(&mut vector, value, shape);
        // An append within its range leaves the storage as it was; `set_shape` below
        // records the grown shape.
        if vector.into_parts() != before {
            data.set_properties(vector, properties);
        }
    } else if slot < INLINE_PROPERTY_COUNT {
        if !data.store_inline_narrow(slot, value) {
            widen_inline_storage(data, properties, shape, slot + 1, slot, value);
        }
    } else {
        debug_assert_eq!(slot, INLINE_PROPERTY_COUNT);
        // One arena range sized for the grown shape, not one for the inline values that the
        // add then outgrows at once.
        let inline = data.inline_properties().expect("inline storage");
        let mut grown = [value; INLINE_PROPERTY_COUNT + 1];
        grown[..INLINE_PROPERTY_COUNT].copy_from_slice(&inline);
        let moved = properties.with_values(shape, &grown);
        data.replace_property_storage(moved, [Value::UNDEFINED; INLINE_PROPERTY_COUNT], properties);
    }
    data.set_shape(shape, properties);
}

/// Join an initialized object's contents at publication or a new live-heap census.
/// Dictionary properties do not participate in shape-owned slot observations.
fn join_object_slot_values(object: &Object, properties: &mut ValueArena) {
    let shape = object.exact_shape();
    if shape != object.shape() { return; }
    for slot in 0..properties.shape_length(shape) {
        // SAFETY: the initialized object's exact, non-dictionary shape holds the slot.
        let value = unsafe { read_exact_slot(object, properties, slot) };
        if !value.is_deleted() { properties.note_slot_value(shape, slot, value); }
    }
}

/// Reads a live slot outside dictionary storage, shared by execution and type admission.
///
/// # Safety
/// The slot exists and the object is not in dictionary storage.
#[inline(always)]
unsafe fn read_exact_slot(object: &Object, properties: &ValueArena, slot: usize) -> Value {
    // SAFETY: the caller proves the exact slot exists; the arena holds the object's run.
    unsafe { crate::staged::concrete_own_read(object, properties.values_base(), slot) }
}

/// Whether a store that `widened` its slot's owner is left to the caller: a native-preserving
/// store declines it, and so does one where Number slots hold raw doubles, whose widening may change
/// the encoding of the words of other objects, which the caller's generic path recodes
/// (`Heap::settle_widened`) before it stores.
#[inline(always)]
fn declines_widening(widened: bool, layout: PropertyStoreLayout, properties: &ValueArena) -> bool {
    widened && (layout == PropertyStoreLayout::PreserveNative || properties.float_slots())
}

/// Stores `value` at `slot` of `data`, whose exact shape outside dictionary storage holds it:
/// arena values without extras need no other decoding (as `property_get_flat`), and inline
/// values that cannot take `value` move to the arena only under MayWiden.
/// Preserve declines before changing storage or values.
#[inline(always)]
fn store_exact_slot(data: &mut Object, properties: &mut ValueArena, slot: usize, value: Value, layout: PropertyStoreLayout) -> bool {
    // The caller matched the exact shape, which holds the slot outside dictionary storage.
    // SAFETY: the caller matched the exact shape, which holds the slot.
    let (widened, ty) = unsafe { properties.note_exact_slot_word(data.shape(), slot, value) };
    if declines_widening(widened, layout, properties) { return false; }
    // A plain arena run first, by one payload compare (`Object::plain_arena_start`): the
    // extras-tag and flag tests of `exact_storage` were six instructions per store.
    if let Some(start) = data.plain_arena_start() {
        debug_assert!(matches!(data.exact_storage(), ExactStorage::Plain(plain) if plain == start));
        // SAFETY: as below.
        unsafe { properties.set_flat_at(start, slot, value, ty) };
        return true;
    }
    store_by_storage(data, properties, slot, value, ty, layout)
}

/// `store_exact_slot` past the plain arena run: the store by the object's storage form.
#[inline(always)]
fn store_by_storage(data: &mut Object, properties: &mut ValueArena, slot: usize, value: Value, ty: crate::typed_ir::SlotType, layout: PropertyStoreLayout) -> bool {
    match data.exact_storage() {
        // SAFETY: as above.
        ExactStorage::Plain(start) => unsafe { properties.set_flat_at(start, slot, value, ty) },
        // An object without extras was never a prototype: its store moves no epoch.
        ExactStorage::PackedInline => {
            if !data.store_packed_narrow(slot, value.to_narrow()) {
                if layout != PropertyStoreLayout::MayWiden {
                    return false;
                }
                let length = properties.len(data.properties());
                widen_inline_storage(data, properties, data.shape(), length, slot, value);
            }
        }
        // A prototype's store of the value it holds changes nothing its readers took. SAFETY: as above.
        ExactStorage::Extended { .. } if data.was_used_as_prototype() && unsafe { read_exact_slot(data, properties, slot) } == value => {}
        ExactStorage::Extended { inline } => {
            data.note_value_write(slot);
            // SAFETY: as above.
            unsafe { store_extended_slot(data, properties, (slot, ty), value, inline) };
        }
        // A narrow run takes a narrow value in place; another value widens it first.
        ExactStorage::Narrow => match value.to_narrow() {
            // SAFETY: as above.
            Some(narrow) => unsafe { properties.narrow_set(data.properties(), slot, narrow) },
            None if layout != PropertyStoreLayout::MayWiden => return false,
            None => {
                let wide = properties.widen(data.properties());
                data.set_properties(wide, properties);
                // SAFETY: as above; the widened run holds every slot.
                unsafe { properties.set_unchecked(data.properties(), slot, value) };
            }
        },
    }
    true
}

/// `store_exact_slot` of an `Extended` object, without the prototype epoch: into the inline
/// values its extras hold, or its arena run.
///
/// # Safety
/// As `store_exact_slot`.
#[inline(always)]
unsafe fn store_extended_slot(data: &mut Object, properties: &mut ValueArena, (slot, ty): (usize, crate::typed_ir::SlotType), value: Value, inline: bool) {
    if inline {
        data.store_extended_inline(slot, value);
    } else {
        // SAFETY: the caller proves that the property slot exists outside dictionary storage.
        unsafe { properties.set_flat_unchecked(data.properties(), slot, value, ty) };
    }
}

#[cfg(test)]
mod offset_order_tests;

/// The prototypes a walk remembers pushing: the two newest.
const PUSHED_PROTOS: usize = 2;

/// The prototypes a mark walk pushed last (`Heap::object_children`): two, as a walk of plain
/// objects alternates between theirs and their arrays' (V8-v7 Splay's leaves: one entry
/// missed about twice per leaf, a third of the pops of a full collection).
#[derive(Clone, Copy)]
pub(crate) struct PushedProtos([Value; PUSHED_PROTOS], ProtoLink);

impl Default for PushedProtos {
    fn default() -> Self {
        Self([Value::UNDEFINED; PUSHED_PROTOS], ProtoLink::NULL)
    }
}

impl PushedProtos {
    /// Whether `link` is not the prototype link this walk tested last; it then becomes the
    /// last. A prototype this walk tested was pushed then (or was not a heap value), and a
    /// push need happen once per collection, so a run of objects that share a prototype
    /// decodes it once.
    #[inline(always)]
    pub(crate) fn first_link(&mut self, link: ProtoLink) -> bool {
        if link == self.1 {
            return false;
        }
        self.1 = link;
        true
    }

    /// Whether `proto` is not one of the two pushed last; it then becomes the newest.
    #[inline(always)]
    pub(crate) fn first_push(&mut self, proto: Value) -> bool {
        if proto == self.0[0] || proto == self.0[1] {
            return false;
        }
        self.0 = [proto, self.0[0]];
        true
    }
}
