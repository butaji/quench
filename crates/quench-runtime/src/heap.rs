use crate::value::{HeapReference, Value};
use crate::vm::wtf16::JsString;
use crate::value_vec::{INLINE_PROPERTY_COUNT, ValueArena, ValueVec};
use crate::staged::wasm_instance::Storage as WasmStorage;
use rayon::prelude::*;
use rayon::ThreadPool;
use rustc_hash::FxHashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use rc_drop::{StoppedCellDrop, cell_rc_drop_lock};

/// Every record space, listed once, in `Heap`'s field order. Each row is
/// `Family = field: Space [Elem] view`: `Family` names the space's `RecordSpaceId`
/// variant (whose index is the family's number, in this order); `field: Space` is the space's
/// `Heap` field and type; `Elem` is the type its records hold, whose `RecordValue` names them
/// (`RecordFamily::value_of`); and `view` is its field in `ParallelMarkView`. `$m` is a callback called as `$m! { $pre ; rows }`, where `$pre`
/// is what the caller passes (nothing, or the heap for `mark_bits!`). The `Heap` struct still
/// declares these fields by hand, since a macro cannot expand into a struct body that also holds
/// other fields: a new space is added there as well.
macro_rules! for_each_record_space {
    ($m:ident, $($pre:tt)*) => {
        $m! {
            $($pre)* ;
            Object = object_records: ObjectRecordSpace [OrdinaryRecord] objects,
            Array = array_records: ArrayRecordSpace [ArrayObject] arrays,
            Function = function_records: FunctionRecordSpace [FunctionRecord] functions,
            Promise = promise_records: PromiseRecordSpace [PromiseCell] promises,
            Await = await_records: AwaitRecordSpace [AwaitState] awaits,
            Iterator = iterator_records: IteratorRecordSpace [IteratorRecord] iterators,
            Collection = collection_records: CollectionRecordSpace [CollectionRecord] collections,
            String = string_records: StringRecordSpace [JsString] strings,
            SmallString = small_strings: small_strings::SmallStringSpace [SmallParts] small_strings,
            V128 = v128_records: V128RecordSpace [[u8; crate::wasm::V128_BYTES]] v128s,
            Environment = environment_records: EnvironmentRecordSpace [EnvironmentRecord] environments,
            Symbol = symbol_records: SymbolRecordSpace [SymbolData] symbols,
            WasmGc = wasm_gc_records: WasmGcRecordSpace [WasmGcRecord] wasm_gcs,
        }
    };
}

/// The record arms of a per-family test (`MarkBits::is_marked`, `Heap::is_live`): `$method` asked
/// of the space that holds each record reference. Strings (split between small and large) and
/// legacy slots are the caller's, matched before this; `$otherwise` answers what is left.
macro_rules! record_reference_test {
    ($s:expr, $reference:expr, $method:ident, $otherwise:expr) => {
        match $reference {
            // A live class cell is its own record, in its region; a direct object otherwise names its stub
            // granule, and its record is the stub header's hop.
            Some(HeapReference::ObjectRecord(granule)) => match $s.class_space.live_class(granule) {
                Some(_) => $s.class_space.$method(granule),
                None => $s.object_records.$method($s.stub_record(granule)),
            },
            Some(HeapReference::ArrayRecord(id)) => $s.array_records.$method(id),
            Some(HeapReference::FunctionRecord(id)) => $s.function_records.$method(id),
            Some(HeapReference::PromiseRecord(id)) => $s.promise_records.$method(id),
            Some(HeapReference::AwaitRecord(id)) => $s.await_records.$method(id),
            Some(HeapReference::IteratorRecord(id)) => $s.iterator_records.$method(id),
            Some(HeapReference::CollectionRecord(id)) => $s.collection_records.$method(id),
            Some(HeapReference::V128Record(id)) => $s.v128_records.$method(id),
            Some(HeapReference::EnvironmentRecord(id)) => $s.environment_records.$method(id),
            Some(HeapReference::SymbolRecord(id)) => $s.symbol_records.$method(id),
            Some(HeapReference::WasmGcRecord(id)) => $s.wasm_gc_records.$method(id),
            _ => $otherwise,
        }
    };
}

/// `MarkBits`: every record family's mark bits, and the legacy slots' mark words.
macro_rules! mark_bits_struct {
    (; $($family:ident = $field:ident : $space:ty [$elem:ty] $view:ident,)*) => {
        /// Every family's mark bits, borrowed apart from the slot a trace holds (`mark_bits!`).
        #[derive(Clone, Copy)]
        pub(super) struct MarkBits<'a> {
            $($field: &'a $space,)*
            marks: &'a [AtomicU64],
            /// The class space's base, which reads a direct object's stub (`stub_record_at`).
            class_base: usize,
        }
    };
}

/// The `MarkBits` literal over `$heap`'s mark fields (`mark_bits!`).
macro_rules! mark_bits_literal {
    ($heap:tt ; $($family:ident = $field:ident : $space:ty [$elem:ty] $view:ident,)*) => {
        MarkBits {
            $($field: &$heap.$field,)*
            marks: &$heap.marks,
            class_base: $heap.class_space.base,
        }
    };
}

/// `record_families` and `record_families_mut`, over every record space.
macro_rules! record_families_fns {
    (; $($family:ident = $field:ident : $space:ty [$elem:ty] $view:ident,)*) => {
        /// Every record family, in `RecordSpaceId` order, then the class cell regions (`ClassSpace::cell_families`):
        /// a cell's family index is past `RECORD_FAMILIES`, so every per-family pass covers cells too.
        fn record_families(&self) -> [&dyn RecordFamily; RECORD_FAMILIES + class_space::CELL_REGIONS] {
            let [c1, c2, c3, c4, c5] = self.class_space.cell_families();
            [$(&self.$field as &dyn RecordFamily,)* c1, c2, c3, c4, c5]
        }

        /// `record_families`, mutably.
        fn record_families_mut(&mut self) -> [&mut dyn RecordFamily; RECORD_FAMILIES + class_space::CELL_REGIONS] {
            let [c1, c2, c3, c4, c5] = self.class_space.cell_families_mut();
            [$(&mut self.$field as &mut dyn RecordFamily,)* c1, c2, c3, c4, c5]
        }
    };
}

/// `RecordValue` of each family that names its records by id: the family's `Value` constructor.
macro_rules! record_values_by_id {
    ($($elem:ty => $value:path),* $(,)?) => {$(
        impl RecordValue for $elem {
            fn value_of<const CAPACITY: usize, const HOLE: usize>(_: &RecordSpace<Self, CAPACITY, HOLE>, id: RecordId) -> Value {
                $value(id)
            }
        }
    )*};
}

record_values_by_id! {
    ArrayObject => Value::direct_array,
    FunctionRecord => Value::direct_function,
    PromiseCell => Value::direct_promise,
    AwaitState => Value::direct_await,
    IteratorRecord => Value::direct_iterator,
    CollectionRecord => Value::direct_collection,
    JsString => Value::direct_string,
    SmallParts => small_strings::small_string_value,
    [u8; crate::wasm::V128_BYTES] => Value::direct_v128,
    EnvironmentRecord => Value::direct_environment,
    SymbolData => Value::direct_symbol,
    WasmGcRecord => Value::direct_wasm_gc,
}

impl RecordValue for OrdinaryRecord {
    /// An ordinary object's `Value` is its stub granule, which its record holds (`publish_object_record`).
    fn value_of<const CAPACITY: usize, const HOLE: usize>(space: &RecordSpace<Self, CAPACITY, HOLE>, id: RecordId) -> Value {
        // SAFETY: `RecordFamily::value_of` names a live record of `space`.
        Value::direct_object(unsafe { space.get_live(id) }.stub)
    }
}

/// `RecordSpaceId`, the family numbering every record-space lookup shares (`locate`, `with_space!`,
/// `record_families`), and `RECORD_FAMILIES`, its count. A variant's index is its row's position
/// in `for_each_record_space!`. Environments and symbols are numbered but never counted, so no
/// lookup names them.
macro_rules! record_space_ids {
    (; $($family:ident = $field:ident : $space:ty [$elem:ty] $view:ident,)*) => {
        /// A record space's family, named by its row in `for_each_record_space!`. Every variant
        /// is constructed by `RECORD_FAMILIES`.
        #[derive(Clone, Copy, PartialEq, Eq)]
        pub(super) enum RecordSpaceId {
            $($family,)*
        }

        /// The number of record families (`Heap::record_families`).
        const RECORD_FAMILIES: usize = [$(RecordSpaceId::$family),*].len();
    };
}

/// `Heap::reset`'s empty record spaces.
macro_rules! reset_record_spaces {
    (; $($family:ident = $field:ident : $space:ty [$elem:ty] $view:ident,)*) => {
        /// Replaces every record space with an empty one (`Heap::reset`).
        fn reset_record_spaces(&mut self) {
            $(self.$field = <$space>::default();)*
            self.class_space = class_space::ClassSpace::default();
        }
    };
}

mod access;
mod array_elements;
pub(crate) mod growth;
pub mod layouts;
mod dense_growth;
pub(crate) use dense_growth::dense_write_gap_limit;
mod store_gate;
mod typed_view;
pub use typed_view::{NATIVE_TYPED_ARRAY_LAYOUT, NO_KIND, NativeView};
use typed_view::TypedViewTable;
use layouts::{class_link, class_shape};
pub use array_elements::{ArrayElements, HEADER_VALUES as ARRAY_HEADER_VALUES, DOUBLE_LEN, ENCODED_LEN, LENGTH_FROM_VALUES, OTHERS_FROM_VALUES, OTHERS_UNKNOWN, UniqueElements};

/// The word a double backing holds for the Number `number`: its bits, NaN canonical, so a NaN
/// never reads back as the hole (`Elements::decode_double`).
#[inline(always)]
pub fn double_word(number: f64) -> u64 {
    if number.is_nan() { f64::NAN.to_bits() } else { number.to_bits() }
}

#[quench_macros::test_visible]
pub(crate) use array_elements::NARROW_LEN;
mod array_proofs;
#[allow(unused_imports)]
pub use array_proofs::{
    ArrayStorageProofs, LaneProofScope, NO_INDEX_FREE_CHAIN_LINK, armed_proofs_address, index_free_chain_link_address,
    retire_array_storage_proofs, set_index_free_chain_link,
};
mod cell;
mod dependencies;
pub use dependencies::{INHERITED_FIELD_CACHE_MAX_DEPTH, InheritedFieldAnswer, InheritedFieldCache};
pub(crate) use dependencies::{
    Dependencies, IndexFreeChainProof, InternedChains,
    NO_PROVED_LAYOUT, ProofKind, FactStore, ChainCell};
mod cell_access;
mod buffer_bytes;
mod view_holders;
mod collection_entries;
mod environment_trace;
pub(crate) use environment_trace::{ClosureReadSource, EnvironmentReads, EnvironmentTrace};
mod external;
mod slot_words;
mod inline_storage;
use inline_storage::widen_inline_storage;
mod memory_stats;
pub use memory_stats::HeapMemoryComposition;
#[cfg(feature = "profile-memory")]
mod memory_profile;
mod pacing;
mod parallel_mark; mod planned;
mod prototypes;
mod promise_record;
pub use promise_record::{AbandonedHostContexts, PromiseCell, PromiseReaction, PromiseRecord, PromiseRejectionPhase, PromiseRejectionState, PromiseState, StoredHostContext};
mod rc_drop;
mod records;
pub(crate) use records::RecordBlockRecordsExt;
mod class_space;
mod departure;
#[cfg(test)] mod class_space_tests;
mod object_view;
pub use object_view::{ObjectMut, ObjectRef};
mod root;
mod slots;
mod snapshot;
mod string_alloc;
mod string_atoms;
mod native_layout;
mod record_layouts;
#[allow(unused_imports)]
pub use record_layouts::{
    NATIVE_ARRAY_LAYOUT, NATIVE_COLLECTION_LAYOUT, NATIVE_ENVIRONMENT_LAYOUT, NATIVE_FUNCTION_LAYOUT, NATIVE_OBJECT_LAYOUT, NATIVE_RECORD_CURSOR_LAYOUT,
    PropertyStoreLayout, absent_environment_storage,
};
mod string_parts;
mod small_strings;
pub use small_strings::{FlatText, SMALL_STRING_ID_BASE, SMALL_STRING_ID_MASK, SmallParts, is_small_string};
mod thin_slice;
pub(crate) use thin_slice::ThinBoxSlice;
mod weak;
pub use cell::*;
pub use buffer_bytes::{BufferBytes, BufferRead, buffer_relocations_address, view_epoch};
pub use view_holders::{subscribe_typed_views, typed_views_stable};
pub(crate) use view_holders::live_view_changed;

/// The most fields a class cell holds: the capacity of the largest region (`class_space`).
pub(crate) const MAX_CELL_FIELDS: usize = crate::staged::object_encoding::region_field_capacity(crate::staged::object_encoding::CAPACITY_CLASSES as u32 - 1);

/// The class cell region of `fields` fields: the smallest of regions `1..CAPACITY_CLASSES` whose capacity
/// holds them (an empty object's is region 1). `None` past the largest region, where the object keeps its record.
pub(crate) fn cell_region_for(fields: usize) -> Option<u32> {
    (1..crate::staged::object_encoding::CAPACITY_CLASSES as u32)
        .find(|&region| fields <= crate::staged::object_encoding::region_field_capacity(region))
}

pub(crate) use collection_entries::{CollectionEntries, CollectionEntry, Entries, WeakMapEntries, WeakSetEntries, number_key_hash};
use external::array_payload_bytes;
use records::{
    ArrayRecordSpace, AwaitRecordSpace, EnvironmentRecordSpace, FunctionRecordSpace, CollectionRecordSpace, IteratorRecordSpace, SymbolRecordSpace, V128RecordSpace, WasmGcRecordSpace,
    ObjectRecordSpace, PromiseRecordSpace, RecordFamily, RecordSpace, RecordValue, StringRecordSpace, RecordId, RecordSweep,
};
pub use root::RootId;
pub use root::{RootTable, WeakHandle};
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
pub(crate) const MAX_DENSE_ARRAY_HOLE_LENGTH: usize = DENSE_ARRAY_HOLE_BUDGET_BYTES / size_of::<Value>();
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
pub enum GcTraceKind {
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

for_each_record_space!(record_space_ids,);

for_each_record_space!(mark_bits_struct,);

impl MarkBits<'_> {
    /// The record id of the object whose stub is `granule`.
    #[inline(always)]
    pub(super) fn stub_record(self, granule: u32) -> u32 {
        class_space::stub_record_at(self.class_base, granule)
    }

    /// Whether `value` is a heap value marked so far.
    pub(super) fn is_marked(self, value: Value) -> bool {
        match value.heap_reference() {
            Some(HeapReference::StringRecord(id)) => match small_strings::small_string_local(id) {
                Some(local) => self.small_strings.is_marked(local),
                None => self.string_records.is_marked(id),
            },
            Some(HeapReference::LegacySlot(index)) => {
                let index = index as usize;
                self.marks.get(index / u64::BITS as usize).is_some_and(|word| {
                    word.load(Ordering::Relaxed) & (1 << (index % u64::BITS as usize)) != 0
                })
            }
            reference => record_reference_test!(self, reference, is_marked, false),
        }
    }
}

/// `MarkBits` of a heap, borrowing only its mark fields.
macro_rules! mark_bits {
    ($heap:tt) => {
        for_each_record_space!(mark_bits_literal, $heap)
    };
}
use mark_bits;

/// The bookkeeping that ends the serial visit of a value newly marked: its ephemerons are
/// told (`EphemeronWork::newly_marked`), with `$cell` its legacy cell when it has one. Only
/// the mark fields are borrowed (`mark_bits!`), so `$cell` may borrow from `slots`.
macro_rules! ephemeron_tail {
    ($heap:expr, $ctx:ident, $value:expr, $cell:expr) => {{
        let mark_bits = mark_bits!($heap);
        $ctx.ephemerons.newly_marked($value, $cell, |target| mark_bits.is_marked(target), $ctx.work);
    }};
}

/// The ASCII code units, one past the last (`Heap::ascii_unit_strings`).
pub(crate) const ASCII_UNITS: usize = 0x80;
/// The Latin-1 code units, one past the last: `Heap::ascii_unit_strings` holds a string for
/// each, the ASCII ones first (the table's readers index it by ASCII unit), then the rest
/// that a byte decode (`String.fromCharCode(byte)`) makes.
pub(crate) const UNIT_STRINGS: usize = 0x100;

/// Dead prototype record ids a heap withholds from reuse (`Heap::retire_dead_prototypes`)
/// before it advances `prototype_layout_epoch` and retires its chain cells once and frees them
/// all. A withheld id costs one record's space; each advance retires every cache and proof the
/// VM holds.
const WITHHELD_PROTOTYPE_IDS: usize = 4096;
/// A full collection that frees more payload than this returns the allocator's free memory to
/// the kernel (`release_freed_allocator_memory`).
const FREED_TRIM_BYTES: usize = 16 << 20;
/// A full collection that frees more than a quarter of the payload it swept returns it too, once
/// it frees at least this much: a small heap's share is not worth a trim.
const FREED_TRIM_SHARE_MIN_BYTES: usize = 1 << 20;

/// Returns the allocator's free memory to the kernel. Quench's small-object allocator, where it
/// is the global allocator, releases its empty pages and ages its kept large blocks
/// (`quench_alloc::release_empty_pages`; paced by its own purge delay). Glibc's free heap is
/// trimmed too (`malloc_trim`): it serves every block past the small classes, which the
/// allocator's purge does not reach.
fn release_freed_allocator_memory() {
    quench_alloc::release_empty_pages();
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    // SAFETY: `malloc_trim` only advises the kernel about free allocator memory.
    unsafe {
        libc::malloc_trim(0);
    }
}

#[derive(Default)]
pub struct Heap {
    slots: SlotArena,
    /// Class cells (`class_space`): empty until a class cell is first allocated.
    #[allow(dead_code)]
    class_space: class_space::ClassSpace,
    object_records: ObjectRecordSpace,
    array_records: ArrayRecordSpace,
    function_records: FunctionRecordSpace,
    promise_records: PromiseRecordSpace,
    await_records: AwaitRecordSpace,
    iterator_records: IteratorRecordSpace,
    /// Maps and Sets: an object and its entries, with no legacy-slot form (`alloc_map`).
    collection_records: CollectionRecordSpace,
    /// Strings: immutable leaves, never prototypes, with no legacy-slot form.
    string_records: StringRecordSpace,
    /// Small strings (`small_strings`): ids of the string records' hole, and the texts read.
    small_strings: small_strings::SmallStringSpace,
    small_views: small_strings::SmallViews,
    small_templates: small_strings::SmallTemplates,
    string_atoms: string_atoms::StringAtoms,
    /// Wasm vectors: immutable leaves (`Heap::alloc_v128`), in a slot when this is full.
    v128_records: V128RecordSpace,
    environment_records: EnvironmentRecordSpace,
    /// Symbols (`alloc_symbol`): immutable leaves that hold no edges, never prototypes.
    symbol_records: SymbolRecordSpace,
    /// Wasm GC objects (`alloc_wasm_gc`): records whose head holds the payload's address and length.
    wasm_gc_records: WasmGcRecordSpace,
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
    /// Every cell with weak entries allocated since its last full collection or kept by it:
    /// a full collection prunes only these. Walking every slot to find them cost each live
    /// environment ~55 Ir per full collection (a decode and a match per slot). An index whose
    /// cell a minor collection freed may stay until then, naming whatever reused it, which
    /// the prune's match passes over.
    weak_cells: Vec<u32>,
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
    /// Whether a byte charge (external buffers, payload growth, materialized text, reclaimable
    /// programs) made a collection due and none has run since: the one fact the polls read
    /// (`Heap::byte_due`), so cell debt never collects at a poll.
    byte_due: bool,
    collections: u64,
    /// A process-unique id of this heap's record layout, drawn again by a snapshot restore (which resets `collections` and the record blocks): part of `native_layout_epoch`, so equal values are never another heap's or an earlier layout.
    layout_id: u64,
    /// The prototypes `mark_prototype` marked last, most recent first, which keep their
    /// mark for life: an allocation with one of them needs no lookup. A collection may free
    /// one and reuse its handle, so each one forgets them.
    marked_prototypes: [Option<Value>; MARKED_PROTOTYPE_WAYS],
    /// Every object ever marked as a prototype, held weakly: a collection releases the subject
    /// of each that died, which retires the proofs and cells that read it, and advances
    /// `prototype_layout_epoch` only when a dead prototype's record may be reused under a proof
    /// that names it (`retire_dead_prototypes`). Each is held with its subject in the dependency
    /// table (`Dependencies::watch`).
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
    /// Whether plain objects' property runs take the narrow form (`value_vec::NARROW_FORM`) at
    /// promotion and compaction: only the interpreter reads one in place, so only a run without
    /// native tiers (`Heap::set_narrow_runs`) makes them.
    narrow_runs: bool,
    /// Whether a function record took a young run since the last collection
    /// (`finished_run_object`): only then does a minor sweep visit live functions to list
    /// their young survivors (`YoungOwner::Function`).
    young_function_runs: bool,
    /// The least arena length since the last narrowing pass and the fulls since it: the pass
    /// reads every live run, so it reruns only once the arena grew an eighth over that low or
    /// eight fulls went by. The low, not the pass's own length, so that an arena that shrank
    /// and grows again narrows on the way back up.
    narrow_pass: (usize, u32),
    /// Whether any store gate input is on: a concurrent cycle's snapshot barrier, reference
    /// counts, or the arena's slot observation (`Heap::refresh_store_watch`). Off, a store of a
    /// value naming no heap record passes the gate on this byte and the value's tag alone.
    store_watched: bool,
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

/// Every cell kind, once, in discriminant order: `Variant = "name"` rows, each with its attributes.
/// `CellKind`, its `COUNT` and its `NAMES` are generated from these rows.
macro_rules! cell_kinds {
    ($($(#[$meta:meta])* $kind:ident = $name:literal,)*) => {
        #[repr(usize)]
        #[derive(Clone, Copy)]
        pub enum CellKind {
            $($(#[$meta])* $kind,)*
        }
        impl CellKind {
            pub(crate) const COUNT: usize = [$(stringify!($kind)),*].len();
            #[cfg(feature = "profile-memory")]
            pub(crate) const NAMES: [&'static str; Self::COUNT] = [$($name,)*];
        }
    };
}

cell_kinds! {
    Object = "object",
    Array = "array",
    Map = "map",
    Set = "set",
    Iterator = "iterator",
    WeakMap = "weak_map",
    WeakSet = "weak_set",
    WeakRef = "weak_ref",
    Function = "function",
    Environment = "environment",
    /// String records; only the allocation profile counts them by kind.
    #[cfg_attr(not(feature = "profile-aggregate"), allow(dead_code))]
    String = "string",
    BigInt = "bigint",
    Symbol = "symbol",
    Date = "date",
    Error = "error",
    RegExp = "regexp",
    ArrayFromAsync = "array_from_async",
    AwaitState = "await_state",
    TemporalDuration = "temporal_duration",
    TemporalPlainDate = "temporal_plain_date",
    TemporalPlainDateTime = "temporal_plain_date_time",
    TemporalPlainMonthDay = "temporal_plain_month_day",
    TemporalPlainYearMonth = "temporal_plain_year_month",
    TemporalZonedDateTime = "temporal_zoned_date_time",
    TemporalInstant = "temporal_instant",
    WasmGlobal = "wasm_global",
    WasmMemory = "wasm_memory",
    WasmTable = "wasm_table",
    WasmElements = "wasm_elements",
    WasmBits64 = "wasm_bits64",
    WasmHostFunction = "wasm_host_function",
    WasmV128 = "wasm_v128",
    WasmGc = "wasm_gc",
    WasmExtern = "wasm_extern",
    WasmTag = "wasm_tag",
    WasmException = "wasm_exception",
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

/// The state a serial mark shares down its visit (`mark_work`, `mark_work_in_rounds`,
/// `drain_work`, `mark_value`), built once by `mark_work`: the worklist, the object-root
/// callback, the ephemeron work, the trace kind, the environment trace, and the protos the
/// last visit pushed.
struct MarkCtx<'a, 's, R> {
    work: &'a mut Vec<Value>,
    object_roots: &'a mut R,
    ephemerons: &'a mut weak::EphemeronWork,
    trace_kind: GcTraceKind,
    environments: &'a mut EnvironmentTrace<'s>,
    last_proto: PushedProtos,
}

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
            weak_cells: Vec::new(),
            free_by_slab: Vec::with_capacity(12),
            free_slabs: Vec::with_capacity(12),
            bump_bits: 0,
            bump_base: 0,
            free_slots: 0,
            layout_id: Self::next_layout_id(),
            collect_at: std::cell::Cell::new(MemoryPolicy::default().pacing().minimum_young),
            byte_due: false,
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
    #[inline] pub fn memory_policy(&self) -> MemoryPolicy {
        self.memory_policy
    }
    fn pacing(&self) -> pacing::GenerationalPacing {
        self.memory_policy.pacing()
    }
    /// The minor collections a young cell or record survives before it is old: the policy's
    /// (`GenerationalPacing::promotion_age`). It may change between collections: every minor
    /// collection frees or promotes the aged survivors it finds.
    pub(crate) fn promotion_age(&self) -> PromotionAge {
        #[cfg(test)]
        if let Some(age) = self.promotion_age_for_test {
            return age;
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
    pub(crate) fn alloc_environment_record(&mut self, environment: EnvironmentRecord) -> Value {
        let EnvironmentRecord { parent, function, slots, scope } = environment;
        self.alloc_environment(parent, function, slots, scope).0
    }

    #[cfg(test)]
    pub(crate) fn environment_count_for_test(&self) -> usize {
        self.environment_records.live_count()
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
            .any(|family| family.live_count() > 0)
    }

    /// How this collection marks in parallel, if it does: a full collection, or a minor one
    /// of a heap without records, once the live slots and records reach
    /// `PARALLEL_MARK_MINIMUM_CELLS`, on a host whose pool has workers.
    fn parallel_mark<'a>(
        &self,
        object_roots: Option<&'a ParallelObjectRootTrace<'a>>,
    ) -> Option<parallel_mark::ParallelMark<'a>> {
        let traces_records_in_parallel = self.collection_state == CollectionState::FullMarking || !self.has_direct_records();
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

    for_each_record_space!(record_families_fns,);

    /// Each program a live cell or function can still run or resolve names in
    /// (`Cell::program_references`).
    pub(crate) fn for_each_program_reference(&mut self, mut visit: impl FnMut(crate::vm::ProgramId)) {
        self.function_records
            .for_each_live(|_, function| function.program().into_iter().for_each(&mut visit));
        self.await_records
            .for_each_live(|_, state| state.program_references(&mut visit));
        self.iterator_records
            .for_each_live(|_, iterator| iterator.program_references(&mut visit));
        self.environment_records.for_each_live(|_, environment| {
            if let Some(program) = environment.scope.program() {
                visit(crate::vm::ProgramId::from_raw(program));
            }
        });
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

    /// Visits the elements of every Wasm table in the slot heap, mutably: the same walk as
    /// `for_each_cell`, for the writers that rewrite rows (`WasmTableElements::refresh_bodies`).
    pub fn for_each_wasm_table_mut(&mut self, mut visit: impl FnMut(&mut WasmTableElements)) {
        for (_, slab) in self.slots.slabs_mut() {
            for slot in slab.iter_mut() {
                if let Some(Cell::WasmTable { elements, .. }) = &mut slot.cell {
                    visit(&mut **elements);
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
                .map(|family| family.live_count())
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

    /// Marks the object `object`'s values as held by code (`Object::mark_constant`): its next
    /// value write or layout change retires the readers subscribed under its exact shape and
    /// clears the mark. No extras are made, so once written it takes every fast store again
    /// (a prototype mark's extras would keep it off them for good). `None` for a non-object.
    pub(crate) fn mark_own_constant(&mut self, object: Value) -> Option<()> {
        self.object_mut(object)?.mark_constant();
        Some(())
    }

    #[inline] pub fn mark_prototype(&mut self, prototype: Value) {
        if !self.marked_prototypes.contains(&Some(prototype)) {
            self.mark_new_prototype(prototype);
        }
    }

    /// `mark_prototype` of a prototype other than the one it marked last.
    #[inline(never)]
    fn mark_new_prototype(&mut self, prototype: Value) {
        // A prototype is a record: a class cell departs before it is read or marked (R4-C4).
        self.depart_for_mutation(prototype);
        // A prototype marked and watched before, as most allocations' prototypes are, only
        // takes the recent entry when its run is still wide: no exclusive access, which
        // widens and barriers.
        if self.object(prototype).is_some_and(|object| {
            object.was_used_as_prototype()
                && object.watched_subject().is_some()
                && object.record_only().is_some_and(|record| !record.properties().is_narrow())
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

    /// The record `mark_prototype` marks. A prototype takes extras (`Object::mark_as_prototype`), so a live
    /// class cell departs first and its record is the object.
    #[inline]
    fn prototype_object_mut(&mut self, prototype: Value) -> Option<&mut Object> {
        self.depart_for_mutation(prototype);
        match prototype.direct_object_index() {
            // SAFETY: direct-object Values name live records; `&mut self` is exclusive.
            Some(object_ref) => {
                let record_id = self.record_id(object_ref)?;
                // A flag, not an edge: no write barrier for record prototypes.
                let object: &mut Object = unsafe { self.object_records.get_live_mut_unbarriered(record_id) };
                // A prototype takes extras (`Object::mark_as_prototype`), which hold wide runs.
                access::widen_narrow_run(object, &mut self.properties);
                Some(object)
            }
            None => self.object_mut(prototype)?.record_only_mut(),
        }
    }

    /// The proofs that rely on prototypes' layouts (`dependencies`).
    #[inline(always)]
    pub(crate) fn dependencies(&self) -> &dependencies::Dependencies {
        &self.dependencies
    }

    #[quench_macros::test_visible]
    #[inline(always)]
    pub(crate) fn dependencies_mut(&mut self) -> &mut dependencies::Dependencies {
        &mut self.dependencies
    }

    /// Retires the proofs that read `array`'s elements, when it is a watched prototype.
    fn note_prototype_elements(&self, array: Value) {
        if let Some(record) = self.object(array).and_then(|object| object.record_only()) {
            record.note_prototype_elements_change();
        }
    }

    /// Allocates `cell`. A cell is never an array: an array takes its record (`alloc_array`).
    #[inline(always)]
    pub fn alloc(&mut self, cell: Cell) -> Value {
        self.alloc_cell(cell)
    }

    /// A new array record of `object`, admitted and charged as any allocation (its element
    /// storage through `charge_payload_allocation`). The array space holds
    /// `ARRAY_RECORD_CAPACITY` (2^30) records, so a full space is a panic.
    #[inline(always)]
    pub fn alloc_array(&mut self, object: ArrayObject) -> Value {
        let prototype = object.proto();
        self.admit_object_contents(&object);
        self.alloc_slotless_array_of(object, prototype)
    }

    /// `alloc_array` of a fresh array (`ArrayObject::fresh`) of `prototype` and `elements`,
    /// with the prototype as given rather than decoded back from the header.
    #[inline(always)]
    pub(crate) fn alloc_fresh_array(&mut self, prototype: Value, elements: ArrayElements) -> Value {
        let object = ArrayObject::fresh(prototype, elements);
        self.alloc_slotless_array_of(object, prototype)
    }

    /// `alloc_array` of an array with no named property (`ArrayObject::fresh`: the empty
    /// shape, no property store), whose admission to the slot-type ledger
    /// (`admit_object_contents`) has no slot to note or recode.
    #[inline(always)]
    fn alloc_slotless_array_of(&mut self, object: ArrayObject, prototype: Value) -> Value {
        debug_assert!(object.shape() == 0);
        self.mark_prototype(prototype);
        self.charge_array_payload(array_payload_bytes(&object));
        #[cfg(feature = "profile-aggregate")]
        let bytes = object.allocated_extra_bytes() + object.elements().capacity() * size_of::<Value>();
        let Some(record_id) = self.array_records.bump_id(&mut self.allocated) else {
            panic!("array records exhausted: the array space holds 2^30 records");
        };
        // SAFETY: the id is claimed uninitialized and written whole here, before any safepoint.
        unsafe { self.array_records.record_ptr(record_id).write(object) };
        let value = Value::direct_array(record_id);
        #[cfg(feature = "profile-aggregate")]
        self.profile_record_allocation(CellKind::Array, "array", bytes);
        #[cfg(feature = "profile-memory")]
        self.profile_allocation(value);
        value
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
        self.alloc_slotless_array_of(object, prototype)
    }

    /// A new array of `prototype` holding `len` holes as a double backing
    /// (`ArrayElements::double_holes`): `new Array(len)`, whose Numbers then store and load as
    /// their own bits. A hole reads `undefined` still, and a value that is no Number widens
    /// the backing once (`ArrayElements::make_mut`).
    #[inline(always)]
    pub(crate) fn alloc_holes_array(&mut self, prototype: Value, len: usize) -> Value {
        let elements = ArrayElements::double_holes(len);
        let object = ArrayObject::fresh(prototype, elements);
        self.alloc_slotless_array_of(object, prototype)
    }

    /// A new array of `prototype`, with no properties, whose elements are the values of `parts`
    /// in order, copied once into a young run where one is free (`alloc_literal_array`), else
    /// into one allocation of the total length.
    #[inline(always)]
    pub(crate) fn alloc_literal_array_from_parts(&mut self, prototype: Value, parts: &[&[Value]]) -> Value {
        let len: usize = parts.iter().map(|part| part.len()).sum();
        let elements = match self.properties.young_elements(len) {
            // SAFETY: the run is the nursery's and nothing else's; the record follows at once.
            Some(run) => unsafe { ArrayElements::young_from_parts(run, parts) },
            None => ArrayElements::from_parts(parts),
        };
        let object = ArrayObject::fresh(prototype, elements);
        self.alloc_slotless_array_of(object, prototype)
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

    /// The payload charge of an array record: its record's own unit is charged with its run
    /// (`RecordSpace::bump_id`), so only the payload weight moves per allocation.
    #[inline(always)]
    fn charge_array_payload(&mut self, payload: usize) {
        self.charge_materialized_text();
        self.payload_bytes = self.payload_bytes.saturating_add(payload);
        self.allocated += payload / payload_unit();
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
            Cell::Promise(promise) => match self.promise_records.try_insert(*promise) {
                Ok(record_id) => {
                    let value = Value::direct_promise(record_id);
                    #[cfg(feature = "profile-memory")]
                    self.profile_allocation(value);
                    return value;
                }
                Err(promise) => Cell::Promise(Box::new(promise)),
            },
            Cell::AwaitState(state) => match self.await_records.try_insert(*state) {
                Ok(record_id) => return Value::direct_await(record_id),
                Err(state) => Cell::AwaitState(Box::new(state)),
            },
            cell => cell,
        };
        self.alloc_slotted(cell, track_young)
    }

    /// `cell` in a legacy slot; a typed array or a DataView also joins the table native code reads its views from.
    fn alloc_slotted(&mut self, cell: Cell, track_young: bool) -> Value {
        let typed = matches!(cell, Cell::TypedArray { .. } | Cell::DataView { .. });
        let value = self.alloc_in_slot(cell, track_young);
        if typed {
            self.register_typed_view(value);
        }
        value
    }

    /// Names the record of the typed array or DataView just allocated at `value` in the table native code reads.
    fn register_typed_view(&mut self, value: Value) {
        let Some(index) = value.legacy_heap_index() else { return };
        match self.slots.get_mut(index as usize).and_then(|slot| slot.cell.as_mut()) {
            Some(Cell::TypedArray { object, .. } | Cell::DataView { object, .. }) => self.typed_views.register(index, &mut object.native),
            _ => {}
        }
    }

    /// The directory of the typed array view record pages by legacy slot and its page count
    /// (`NATIVE_TYPED_ARRAY_LAYOUT`): read an entry from a current live typed array's slot
    /// index, finish before any VM safepoint, which an allocation may move the directory at.
    #[inline]
    pub fn typed_view_directory(&self) -> (*const usize, usize) {
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

    /// An environment record, with the address of its slot storage, which stays put while it
    /// lives. An environment's weight is one allocation: it has no prototype, payload or
    /// external bytes.
    pub fn alloc_environment(
        &mut self,
        parent: Value,
        function: u32,
        slots: EnvironmentSlots,
        scope: EnvironmentScopeRef,
    ) -> (Value, *const EnvironmentSlots) {
        let Some(record_id) = self.environment_records.bump_id(&mut self.allocated) else {
            panic!("environment records exhausted");
        };
        // SAFETY: the id is claimed uninitialized and written whole here, before any safepoint.
        unsafe { self.environment_records.record_ptr(record_id).write(EnvironmentRecord { parent, function, slots, scope }) };
        // SAFETY: just written; records never move while they live.
        let storage = unsafe { &self.environment_records.get_live(record_id).slots } as *const EnvironmentSlots;
        (Value::direct_environment(record_id), storage)
    }

    /// An ordinary object's allocation: its prototype marked, its values admitted, and its record
    /// inserted by `insert_object_record`, with no dispatch on a cell's kind (there is no object cell).
    #[inline(always)]
    pub fn alloc_object(&mut self, object: Object) -> Value {
        let prototype = object.proto();
        self.alloc_object_of(object, prototype)
    }

    /// `alloc_object` of `object` whose class cell holds at least `capacity` fields (a constructor's learned
    /// storage or union layout, `Vm::alloc_construct_receiver`).
    pub(crate) fn alloc_object_with_capacity(&mut self, object: Object, capacity: usize) -> Value {
        let prototype = object.proto();
        self.alloc_object_of_capacity(object, prototype, capacity)
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
        self.mark_prototype(prototype);
        self.admit_object_contents(&object);
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
        self.insert_object_record(object)
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
        self.admit_object_contents(&object);
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
        if let Some(id) = self.function_records.cursor_take() {
            return Some(Value::direct_function(id));
        }
        let id = self.function_records.claim_uninit_quick()?;
        self.allocated += 1;
        Some(Value::direct_function(id))
    }

    /// `alloc_function_under_flagged_prototype` of `function` into the record `claimed`
    /// (`claim_function_record`) names.
    #[inline(always)]
    pub(crate) fn fill_claimed_function_record(&mut self, claimed: Value, function: FunctionRecord) {
        debug_assert!(self.prototype_flagged(function.object.proto()));
        let id = claimed.direct_function_index().expect("a claimed function record");
        self.admit_object_contents(&function.object);
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
        let start = self.properties.young_run_padded(values, len)?;
        self.young_function_runs = true;
        Some(Object::from_run_header(header, start))
    }

    /// `property_set` of slot `slot` of `object`, an object record allocated with inline
    /// storage since the last allocation that could collect, whose slot holds `undefined`:
    /// the barrier and the slot's recorded representation as for any store, and the narrow
    /// store into the payload word without the general path's tests.
    #[inline(always)]
    pub(crate) fn fill_new_object_slot(&mut self, object: Value, slot: usize, value: Value) {
        let Some(object_ref) = object.direct_object_index() else {
            return self.property_set(object, slot, value);
        };
        // A live class cell has no inline record to fill: the generic store takes it.
        let Some(record_id) = self.record_id(object_ref) else {
            return self.property_set(object, slot, value);
        };
        let Self { object_records, properties, .. } = &mut *self;
        // SAFETY: direct-object Values name live records; `&mut self` is exclusive.
        let data = unsafe { object_records.get_live_mut_storing(record_id, value) };
        if !data.properties().has_inline_property_storage() || !data.store_inline_narrow(slot, value) {
            return self.property_set(object, slot, value);
        }
        if properties.tracks() {
            let shape = data.exact_shape();
            if shape == data.shape() {
                properties.note_slot_value(shape, slot, value);
            }
        }
        self.rewrite_widened_numbers();
    }

    /// Whether `prototype` names no object or one flagged as a prototype, as
    /// `mark_new_prototype` leaves it.
    fn prototype_flagged(&self, prototype: Value) -> bool {
        self.object(prototype).is_none_or(|object| object.was_used_as_prototype())
    }

    /// The allocation of ordinary object `object`: a class cell (`insert_object_as_cell`) where it has
    /// the cell form, else its record. The caller admits the object's initial values
    /// (`admit_object_contents`) and marks its prototype, or leaves them as they are where
    /// placeholders must record nothing.
    #[inline(always)]
    fn insert_object_record(&mut self, object: Object) -> Value {
        self.insert_object_with_capacity(object, 0)
    }

    /// `insert_object_record` where the object's cell holds at least `capacity` fields: the learned
    /// storage of its constructor, or the closed world's union layout of its class.
    #[inline(always)]
    pub(crate) fn insert_object_with_capacity(&mut self, object: Object, capacity: usize) -> Value {
        match self.insert_object_as_cell(object, capacity) {
            Ok(value) => value,
            Err(object) => self.insert_object_as_record(object),
        }
    }

    /// `object` as a class cell (R4-C2): its class is its shape and prototype link with no flags, its
    /// fields its values, its slack `Value::DELETED`, in the smallest region holding the larger of its
    /// fields and `capacity`. Its arena storage moves into the cell, so that storage is released.
    /// `Err(object)`, unchanged, where the object has a record's form (extras, an array, a class flag,
    /// more fields than a cell holds) or its region is full.
    fn insert_object_as_cell(&mut self, object: Object, capacity: usize) -> Result<Value, Object> {
        if cfg!(feature = "profile-aggregate")
            || object.is_array()
            || object.has_extras()
            || layouts::word(object.layout_id()) & layouts::CLASS_FLAG_MASK != 0
        {
            return Err(object);
        }
        let shape = object.shape();
        let length = self.properties.shape_length(shape);
        let Some(region) = cell_region_for(length.max(capacity)) else {
            return Err(object);
        };
        let mut fields = [Value::DELETED; MAX_CELL_FIELDS];
        for (slot, field) in fields.iter_mut().enumerate().take(length) {
            *field = self.property_get(&object, slot).unwrap_or(Value::UNDEFINED);
        }
        let class = layouts::intern_class(shape, object.proto_link(), 0);
        let Some(granule) = self.class_space.claim_cell(region, class, &fields[..length], &mut self.allocated) else {
            return Err(object);
        };
        object.release_storage(&mut self.properties);
        Ok(Value::direct_object(granule))
    }

    /// The record of `object`, an ordinary object's only allocation, charged as one allocation
    /// with no payload or external bytes (`allocated += 1` only). The object record space holds
    /// `OBJECT_RECORD_CAPACITY` (2^30) records, so a full space is a panic; no legacy slot takes
    /// an object. The record form, kept for the objects a cell cannot hold.
    #[inline(always)]
    fn insert_object_as_record(&mut self, object: Object) -> Value {
        // An object owns no payload or external bytes: its record is the whole charge, made
        // with its cursor run (`RecordSpace::bump_id`), with no `charge_allocation` on the hot path.
        #[cfg(feature = "profile-aggregate")]
        self.profile_record_allocation(CellKind::Object, "object", 0);
        let Some(record_id) = self.object_records.bump_id(&mut self.allocated) else {
            panic!("object records exhausted: the object space holds 2^30 records");
        };
        // SAFETY: the id is claimed uninitialized and written whole here, before any safepoint.
        unsafe { self.object_records.record_ptr(record_id).write(OrdinaryRecord { object, stub: 0 }) };
        let value = self.publish_object_record(record_id);
        #[cfg(feature = "profile-memory")]
        self.profile_allocation(value);
        value
    }

    /// Publishes object record `record_id`, whose record is written: claims its stub in region 0,
    /// writes the stub's header (the record id, `departed_header`), stores the granule in the record,
    /// and returns the direct object `Value` the granule names. A record claimed but not yet written
    /// may be published first: only its stub field is written here.
    #[inline(always)]
    pub(crate) fn publish_object_record(&mut self, record_id: u32) -> Value {
        let granule = self
            .class_space
            .claim_stub(record_id, &mut self.allocated)
            .expect("class space stubs exhausted: region 0 holds the stubs of every ordinary object");
        let record = self.object_records.record_ptr(record_id);
        // SAFETY: `record_id` is a claimed record in the object space; only its stub field is written.
        unsafe { std::ptr::addr_of_mut!((*record).stub).write(granule) };
        Value::direct_object(granule)
    }

    /// The direct object `Value` of the live object record `record_id`: its stub granule.
    #[inline(always)]
    pub(crate) fn object_value(&self, record_id: u32) -> Value {
        // SAFETY: the caller names a live object record, whose stub is set by `publish_object_record`.
        Value::direct_object(unsafe { self.object_records.get_live(record_id) }.stub)
    }

    /// Admit initial named properties before any object-bearing cell or record
    /// becomes visible. Generic and dedicated publishers share the live-slot reader.
    #[inline(always)]
    fn admit_object_contents(&mut self, object: &Object) {
        if self.properties.tracks() {
            join_object_slot_values(object, &mut self.properties);
        }
    }

    /// `alloc(Cell::RegExp { object, meta })` of a fresh RegExp, which owns no payload: the
    /// cell is built in the slot it takes rather than moved through `alloc_cell`'s match.
    #[inline(always)]
    pub(crate) fn alloc_regexp(&mut self, object: Object, meta: std::rc::Rc<RegExpMeta>) -> Value {
        if cfg!(feature = "profile-aggregate") {
            return self.alloc(Cell::RegExp { object, meta });
        }
        self.admit_object_contents(&object);
        self.mark_prototype(object.proto());
        self.charge_payload_allocation(0);
        self.alloc_in_slot_with(self.tracks_young_cells(), false, || Cell::RegExp { object, meta })
    }

    /// A new function record of `function`, with no payload or external bytes. The function
    /// space holds `FUNCTION_RECORD_CAPACITY` (2^30) records, so a full space is a panic.
    /// Stores `realm` as `function`'s creation realm override. A narrow property run is widened
    /// first: an object with extras may not keep one (`Object::extras_mut`).
    pub(crate) fn set_function_realm_override(&mut self, function: &mut FunctionRecord, realm: Value) {
        access::widen_narrow_run(&mut function.object, &mut self.properties);
        function.object.set_function_realm_override(realm);
    }

    pub(crate) fn alloc_function(&mut self, function: FunctionRecord) -> Value {
        self.mark_prototype(function.object.proto());
        self.insert_function_record(function)
    }

    /// `alloc_function` of `function` whose prototype is already flagged as one, as
    /// `alloc_unfilled_object_under_flagged_prototype` allocates an object.
    pub(crate) fn alloc_function_under_flagged_prototype(&mut self, function: FunctionRecord) -> Value {
        debug_assert!(self.prototype_flagged(function.object.proto()));
        self.insert_function_record(function)
    }

    /// A function record of `function`: one allocation, admitted and charged as an object
    /// record is (`alloc_object_record`).
    #[inline(always)]
    fn insert_function_record(&mut self, function: FunctionRecord) -> Value {
        #[cfg(feature = "profile-aggregate")]
        let bytes = function.object.allocated_extra_bytes() + size_of::<FunctionRecord>();
        self.admit_object_contents(&function.object);
        let Some(record_id) = self.function_records.bump_id(&mut self.allocated) else {
            panic!("function records exhausted: the function space holds 2^30 records");
        };
        // SAFETY: the id is claimed uninitialized and written whole here, before any safepoint.
        unsafe { self.function_records.record_ptr(record_id).write(function) };
        let value = Value::direct_function(record_id);
        #[cfg(feature = "profile-aggregate")]
        self.profile_record_allocation(CellKind::Function, "function", bytes);
        #[cfg(feature = "profile-memory")]
        self.profile_allocation(value);
        value
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
    pub fn alloc_v128(&mut self, bytes: [u8; crate::wasm::V128_BYTES]) -> Value {
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
    pub fn v128(&self, value: Value) -> Option<[u8; crate::wasm::V128_BYTES]> {
        if let Some(record_id) = value.direct_v128_index() {
            // SAFETY: a direct-v128 Value names an initialized record.
            return Some(*unsafe { self.v128_records.get_live(record_id) });
        }
        match self.get(value)? {
            Cell::WasmV128(bytes) => Some(*bytes),
            _ => None,
        }
    }

    /// A new iterator record, admitted and charged as an object record is (`alloc_object_record`).
    /// The iterator space holds `ITERATOR_RECORD_CAPACITY` (2^28) records, so a full space is a
    /// panic.
    pub fn alloc_iterator(&mut self, iterator: IteratorRecord) -> Value {
        #[cfg(feature = "profile-aggregate")]
        let bytes = iterator.object.allocated_extra_bytes();
        self.admit_object_contents(&iterator.object);
        self.mark_prototype(iterator.object.proto());
        self.allocated += 1;
        let Ok(record_id) = self.iterator_records.try_insert(iterator) else {
            panic!("iterator records exhausted: the iterator space holds 2^28 records");
        };
        let value = Value::direct_iterator(record_id);
        #[cfg(feature = "profile-aggregate")]
        self.profile_record_allocation(CellKind::Iterator, "iterator", bytes);
        #[cfg(feature = "profile-memory")]
        self.profile_allocation(value);
        value
    }

    /// The aggregate profile's counts for a record allocated without a cell, as `alloc_cell`
    /// counts a cell: its kind, its variant name, and the bytes it owns beyond its slot.
    #[cfg(feature = "profile-aggregate")]
    fn profile_record_allocation(&mut self, kind: CellKind, variant: &'static str, bytes: usize) {
        self.gc_profile.allocated_kinds[kind as usize] += 1;
        *self.allocated_variants.entry(variant).or_default() += 1;
        self.gc_profile.allocated_payload_bytes[kind as usize] += bytes as u64;
        self.gc_profile.allocated_size_buckets[kind as usize][crate::profile::size_bucket(bytes)] += 1;
    }

    /// Allocates `record`, an object kind with its own record space: admitted and charged as one
    /// allocation with no payload or external bytes, inserted by `insert` (its handle `direct`),
    /// or as the legacy `cell` where the space is full or allocations are being profiled.
    #[inline(always)]
    fn alloc_object_record<T>(&mut self, record: T, object: impl Fn(&T) -> &Object,
        insert: impl FnOnce(&mut Self, T) -> Result<u32, T>, direct: impl FnOnce(u32) -> Value, cell: impl FnOnce(T) -> Cell) -> Value {
        if cfg!(feature = "profile-aggregate") {
            return self.alloc(cell(record));
        }
        self.admit_object_contents(object(&record));
        self.mark_prototype(object(&record).proto());
        self.allocated += 1;
        match insert(self, record) {
            Ok(record_id) => {
                let value = direct(record_id);
                #[cfg(feature = "profile-memory")]
                self.profile_allocation(value);
                value
            }
            Err(record) => self.alloc_in_slot(cell(record), self.tracks_young_cells()),
        }
    }

    /// Allocations per cell variant.
    #[cfg(feature = "profile-aggregate")]
    pub(crate) fn allocated_variants(&self) -> &std::collections::BTreeMap<&'static str, u64> {
        &self.allocated_variants
    }

    pub(crate) fn alloc_promise(&mut self, promise: PromiseCell) -> Value {
        self.alloc_object_record(promise, |promise| &promise.object, |heap, promise| heap.promise_records.try_insert(promise),
            Value::direct_promise, |promise| Cell::Promise(Box::new(promise)))
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
            self.weak_cells.push(index);
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
            self.weak_cells.push(index as u32);
        }
        Value::heap(index as u32)
    }
    /// The literal pair of `Vm::object_pair`: `alloc_object_values` of its two values.
    #[inline]
    pub(crate) fn alloc_object_pair(
        &mut self,
        proto: Value,
        shape: u32,
        first: Value,
        second: Value,
    ) -> Value {
        self.alloc_object_values(proto, shape, &[first, second], 0)
    }

    /// `alloc_object` of an ordinary object of prototype `proto`, shape `shape` and fields `values`: a class
    /// cell (`claim_values_cell`) holding at least `capacity` fields, else the object's record.
    pub fn alloc_object_with_properties(
        &mut self,
        proto: Value,
        shape: u32,
        values: &[Value],
    ) -> Value {
        self.alloc_object_values(proto, shape, values, 0)
    }

    /// `alloc_object_with_properties` where the cell holds at least `capacity` fields.
    pub(crate) fn alloc_object_values(&mut self, proto: Value, shape: u32, values: &[Value], capacity: usize) -> Value {
        self.properties.assert_shape_length(shape, values.len());
        if let Some(granule) = self.claim_values_cell(proto, shape, values, capacity) {
            return Value::direct_object(granule);
        }
        let object = if values.len() <= INLINE_PROPERTY_COUNT
            && let Some(object) = Object::with_inline_values(proto, shape, values)
        {
            object
        } else {
            let properties = self.properties.with_values(shape, values);
            Object::with_property_storage(proto, properties, [Value::UNDEFINED; INLINE_PROPERTY_COUNT])
        };
        self.alloc_object_of_capacity(object, proto, capacity)
    }

    /// `alloc_object_of` where the object's cell holds at least `capacity` fields.
    #[inline(always)]
    pub(crate) fn alloc_object_of_capacity(&mut self, object: Object, prototype: Value, capacity: usize) -> Value {
        self.mark_prototype(prototype);
        self.admit_object_contents(&object);
        self.insert_object_with_capacity(object, capacity)
    }

    /// The class cell of an ordinary object of prototype `proto`, shape `shape` and fields `values` (the
    /// literal and empty-object allocations): its class is `ProtoLink::of(proto)`'s class of `shape`, which
    /// the object would take with no flags. The prototype is marked as `alloc_object` marks it, and the
    /// shape's values are noted as `initialize_object_properties` notes them. `None`, with nothing charged,
    /// where the values exceed a cell or the region is full: the caller takes the record.
    fn claim_values_cell(&mut self, proto: Value, shape: u32, values: &[Value], capacity: usize) -> Option<u32> {
        if cfg!(feature = "profile-aggregate") {
            return None;
        }
        let region = cell_region_for(values.len().max(capacity))?;
        let class = ProtoLink::of(proto).class(shape);
        self.mark_prototype(proto);
        self.properties.note_shape_values(shape, values);
        self.rewrite_widened_numbers();
        // The cell's fields are the arena words of `shape` (`stored_words`): a Number slot holds its raw double.
        let words = self.properties.stored_words(shape, values);
        self.class_space.claim_cell(region, class, &words, &mut self.allocated)
    }

    /// The class cell of a fixed class's instance (R4-F, decision 4.2): claimed from the region of `values.len()`,
    /// its header the class `intern_class(shape, link, 0)` and its fields `values` in slot order, `Value::DELETED`
    /// for an absent union field. `values.len()` is the length of `shape`. No record is made: the region holds
    /// its cells in its address span, so only exhaustion of that span panics, as the record space does.
    pub(crate) fn claim_fixed_cell(&mut self, shape: u32, link: u32, values: &[Value]) -> Value {
        debug_assert_eq!(self.properties.shape_length(shape), values.len(), "a fixed cell's fields are its shape's");
        let link = ProtoLink::from_bits(link);
        let region = cell_region_for(values.len()).expect("a fixed class's union fits a class cell");
        self.mark_prototype(link.prototype());
        self.properties.note_shape_values(shape, values);
        self.rewrite_widened_numbers();
        let words = self.properties.stored_words(shape, values);
        let granule = self
            .class_space
            .claim_cell(region, link.class(shape), &words, &mut self.allocated)
            .unwrap_or_else(|| panic!("class cells exhausted: the class space holds its address range"));
        Value::direct_object(granule)
    }

    /// `alloc_array` of a fresh array, given its first named
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
        let value = self.alloc_array(array);
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
        // A live class cell whose capacity holds the values takes them in place, under the class of
        // `shape` (`class_cell_initialize`): a preshaped receiver or a built object stays a cell.
        // Observe the values first: the cell's stores write the slots' words, which depend on the observed types.
        if let Some(granule) = self.live_class_cell(owner) {
            self.properties.note_shape_values(shape, values);
            self.rewrite_widened_numbers();
            if self.class_cell_initialize(granule, shape, values) {
                return;
            }
        }
        // Otherwise replacing storage is a record edit: the cell departs first, so the owner is a record.
        self.depart_for_mutation(owner);
        // An array's payload holds its element backing, so its properties live in the arena.
        let inline = !self
            .object(owner)
            .expect("property owner is an object")
            .is_array();
        self.properties.note_shape_values(shape, values);
        self.rewrite_widened_numbers();
        let (properties, inline_properties) = self.initial_object_storage(shape, values, inline);
        let previous = {
            let (object, arena) = self.object_and_arena_mut(owner).expect("property owner is an object");
            object.replace_property_storage(properties, inline_properties, arena)
        };
        self.properties.release(previous);
        #[cfg(feature = "profile-memory")]
        self.note_object_slots(owner, values.len());
    }

    #[quench_macros::test_visible]
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
    #[quench_macros::test_visible]
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
        self.collection_records.for_each_live_mut(|_, collection| {
            let old_shape = collection.object.shape() as usize;
            collection.object.renumber_shape(mapping[old_shape]);
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
    }

    #[quench_macros::test_visible]
    /// Keeps slot representations from now on (`ValueArena::track`), over the shape table
    /// `layouts` (`register_shape_layouts`), from what live objects hold.
    pub(crate) fn track_slot_representations(&mut self, layouts: &[(usize, Option<(u32, usize)>)]) {
        self.properties.track();
        self.refresh_store_watch();
        self.register_shape_layouts(layouts);
    }

    /// Visits every live object-bearing record and legacy cell with the property arena.
    fn visit_live_objects(&mut self, mut visit: impl FnMut(&Object, &mut ValueArena)) {
        let properties = &mut self.properties;
        self.object_records.for_each_live(|_, record| visit(&record.object, properties));
        self.array_records.for_each_live(|_, array| visit(array, properties));
        self.function_records.for_each_live(|_, function| visit(&function.object, properties));
        self.promise_records.for_each_live(|_, promise| visit(&promise.object, properties));
        self.iterator_records.for_each_live(|_, iterator| visit(&iterator.object, properties));
        self.collection_records.for_each_live(|_, collection| visit(&collection.object, properties));
        for slot in self.slots.iter() {
            if let Some(object) = slot.cell.as_ref().and_then(Cell::object) {
                visit(object, properties);
            }
        }
    }

    /// Joins named properties across every live object-bearing record and legacy cell.
    fn join_live_slot_values(&mut self) {
        if !self.properties.tracks() { return; }
        self.visit_live_objects(join_object_slot_values);
        self.rewrite_widened_numbers();
    }
    /// Moves the surviving young runs of `owners`, live direct objects and functions
    /// (`YoungOwner`), out of the nursery's young range, in the order of their starts, and
    /// ends the young cycle (`ValueArena::finish_young_cycle`).
    fn evacuate_young_runs(&mut self, mut owners: Vec<(usize, YoungOwner)>) {
        owners.sort_unstable_by_key(|&(start, _)| start);
        for (_, owner) in owners {
            // SAFETY: the owners were listed from records the sweep kept.
            let object = match owner {
                YoungOwner::Object(record_id) => &mut unsafe { self.object_records.get_live_mut_unbarriered(record_id) }.object,
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
        object_records.for_each_live_mut(|_, record| {
            let object = &mut record.object;
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

    /// The property-arena owners of the live objects, as the sweep lists them for a compacting
    /// full collection (`sweep_serial`, `sweep_disposing` closures): the packed legacy slot
    /// owners and the direct owners with their range offsets. After a full sweep every occupied
    /// slot holds a surviving cell and every record a surviving record, so the walk lists exactly
    /// what the sweep would have listed had it known.
    fn list_property_owners(&self) -> (Vec<u64>, Vec<(usize, Value)>) {
        let mut slot_owners = Vec::with_capacity(self.property_owner_counts.0);
        let mut direct_owners = Vec::with_capacity(self.property_owner_counts.1);
        let properties = &self.properties;
        for (index, slot) in self.slots.iter().enumerate() {
            if let Some(object) = slot.cell.as_ref().and_then(Cell::object)
                && properties.has_compact_range(object.properties())
            {
                slot_owners.push(pack_offset_owner(object.properties().start_offset(), index));
            }
        }
        self.object_records.for_each_live(|_, record| {
            if record.object.properties().has_compact_range() {
                direct_owners.push((record.object.properties().start_offset(), Value::direct_object(record.stub)));
            }
        });
        self.function_records.for_each_live(|record_id, function| {
            if function.object.properties().has_compact_range() {
                direct_owners.push((function.object.properties().start_offset(), Value::direct_function(record_id)));
            }
        });
        self.promise_records.for_each_live(|record_id, promise| {
            if promise.object.properties().has_compact_range() {
                direct_owners.push((promise.object.properties().start_offset(), Value::direct_promise(record_id)));
            }
        });
        self.iterator_records.for_each_live(|record_id, iterator| {
            if iterator.object.properties().has_compact_range() {
                direct_owners.push((iterator.object.properties().start_offset(), Value::direct_iterator(record_id)));
            }
        });
        self.collection_records.for_each_live(|record_id, collection| {
            if collection.object.properties().has_compact_range() {
                direct_owners.push((collection.object.properties().start_offset(), Value::direct_collection(record_id)));
            }
        });
        self.array_records.for_each_live(|record_id, array| {
            if array.properties().has_compact_range() {
                direct_owners.push((array.properties().start_offset(), Value::direct_array(record_id)));
            }
        });
        (slot_owners, direct_owners)
    }

    /// Slide every live arena range down over released ones, in offset order.
    /// `objects` holds each live arena owner packed with its range offset.
    fn compact_property_arena(
        &mut self,
        slot_owners: Vec<u64>,
        direct_owners: Vec<(usize, Value)>,
    ) {
        if !self.properties.compaction_due() {
            return;
        }
        #[cfg(feature = "profile-memory")]
        let before = self.properties.stats();
        let mut owners = Vec::with_capacity(slot_owners.len() + direct_owners.len());
        owners.extend(slot_owners.into_iter().map(|key| {
            let offset = (key >> u32::BITS) as usize;
            let owner = owner_of_packed(key) as u32;
            (offset, Value::heap(owner))
        }));
        owners.extend(direct_owners);
        owners.sort_unstable_by_key(|(offset, _)| *offset);
        let mut target = 0;
        let Self {
            class_space,
            object_records,
            array_records,
            function_records,
            promise_records,
            iterator_records,
            collection_records,
            slots,
            properties,
            narrow_runs,
            ..
        } = self;
        for (_, owner) in owners {
            let object = if let Some(object_ref) = owner.direct_object_index() {
                let record_id = class_space.record_of(object_ref.granule());
                // SAFETY: direct owners were collected from live marked records.
                Some(&mut unsafe { object_records.get_live_mut_unbarriered(record_id) }.object)
            } else if let Some(record_id) = owner.direct_array_index() {
                // SAFETY: array owners were collected from live array records.
                Some(&mut **unsafe { array_records.get_live_mut_unbarriered(record_id) })
            } else if let Some(record_id) = owner.direct_function_index() {
                // SAFETY: function owners were collected from live function records.
                Some(&mut unsafe { function_records.get_live_mut_unbarriered(record_id) }.object)
            } else if let Some(record_id) = owner.direct_promise_index() {
                // SAFETY: promise owners were collected from live promise records.
                Some(&mut unsafe { promise_records.get_live_mut_unbarriered(record_id) }.object)
            } else if let Some(record_id) = owner.direct_iterator_index() {
                // SAFETY: iterator owners were collected from live iterator records.
                Some(&mut unsafe { iterator_records.get_live_mut_unbarriered(record_id) }.object)
            } else if let Some(record_id) = owner.direct_collection_index() {
                // SAFETY: collection owners were collected from live collection records.
                Some(&mut unsafe { collection_records.get_live_mut_unbarriered(record_id) }.object)
            } else {
                owner
                    .legacy_heap_index()
                    .and_then(|index| slots.get_mut(index as usize))
                    .and_then(|slot| slot.cell.as_mut())
                    .and_then(Cell::object_mut)
            }
            .expect("property owner remains live during compaction");
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
    for_each_record_space!(reset_record_spaces,);

    pub(crate) fn reset(&mut self) {
        self.slots.clear();
        self.reset_record_spaces();
        self.small_views = small_strings::SmallViews::default();
        self.small_templates = small_strings::SmallTemplates::default();
        self.clear_string_atoms();
        self.marks.clear();
        self.aged.clear();
        self.free_by_slab.clear();
        self.free_slabs.clear();
        self.bump_bits = 0;
        self.free_slots = 0;
        buffer_bytes::bump_view_epoch();
        self.external_bytes = 0;
        self.external_since_collection = 0;
        self.byte_due = false;
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
        self.weak_cells.clear();
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
    pub fn collection_epoch(&self) -> u64 {
        self.collections
    }
    /// Charges payload a live cell grew outside `alloc` (array elements) toward the next
    /// collection, at the weight `alloc` gives a cell's own payload.
    pub fn charge_payload_growth(&mut self, bytes: usize) {
        self.payload_bytes = self.payload_bytes.saturating_add(bytes);
        let pending = self.uncharged_payload_bytes.saturating_add(bytes);
        let weight = pending / payload_unit();
        self.uncharged_payload_bytes = pending % payload_unit();
        self.allocated += weight;
        self.note_byte_due();
    }
    /// Accounts new backing-store bytes; past the external budget a collection is due.
    pub(crate) fn charge_external_growth(&mut self, bytes: usize) {
        self.external_bytes += bytes;
        self.external_since_collection = self.external_since_collection.saturating_add(bytes);
        if self.external_since_collection >= self.external_collection_budget {
            self.request_collection();
            self.note_byte_due();
        }
    }
    /// Sets `byte_due` when the byte charge just made a collection due.
    fn note_byte_due(&mut self) {
        if self.should_collect() {
            self.byte_due = true;
        }
    }
    /// Whether a byte charge has made a collection due since the last collection: what the
    /// polls test, so that only byte-charged debt is serviced at a poll.
    #[inline(always)]
    pub fn byte_due(&self) -> bool {
        self.byte_due
    }
    /// Charges text made on demand (`wtf16::take_materialized_text`) as payload allocation.
    /// The string operations that make it allocate or return through the places that call
    /// this (generic allocation, native and host calls, each collection); safepoints, which
    /// run far more often, never pay for it.
    #[inline(always)]
    pub fn charge_materialized_text(&mut self) {
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
        self.note_byte_due();
    }

    /// Makes a collection due at the next safepoint, through a shared heap.
    pub(super) fn request_collection(&self) {
        self.collect_at.set(self.collect_at.get().min(self.allocated));
    }
    /// The weight charged since the heap began: unchanged across a call that allocated nothing.
    #[inline(always)] pub fn allocated_weight(&self) -> usize { self.allocated }
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
        self.full_due
            || self.pacing().allocation_cap_reached(
                self.live_after_full,
                self.allocated_since_full(),
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
        /// The least program footprint since the last full collection that makes one due on
        /// charge. Distinct from `pacing::PACING_MINIMUM_PROGRAM_GROWTH`, the floor
        /// `should_full_collect` checks through the pacing table on each query.
        const CHARGE_MINIMUM_PROGRAM_GROWTH: usize = 256 << 10;
        self.reclaimable_program_bytes_since_full = self.reclaimable_program_bytes_since_full.saturating_add(bytes);
        // Programs die without allocating on the heap: once as much program as the last full
        // collection kept has come since, a full collection is due by itself, or a code-load
        // loop keeps every dead program until heap allocation happens to make one
        // (octane-code-load: no full collection in a run). Against what was kept, programs
        // that all stay (one live function per eval) make it due ever less often.
        if self.reclaimable_program_bytes_since_full >= self.reclaimable_program_bytes_after_full.max(CHARGE_MINIMUM_PROGRAM_GROWTH) {
            self.full_due = true;
            self.request_collection();
            self.note_byte_due();
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
    pub fn root(&mut self, value: Value) -> RootId {
        self.roots.insert(value)
    }
    pub(crate) fn update_root(&mut self, root: RootId, value: Value) -> bool {
        self.roots.update(root, value)
    }
    pub fn release_root(&mut self, root: RootId) -> bool {
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
                    Some(object_ref) => {
                        // A prototype departed its cell on first use, so its stub names its record.
                        let record_id = self.stub_record(object_ref.granule());
                        // Its stub is withheld with its record: the granule names no other object either.
                        self.class_space.withhold_stub(object_ref.granule());
                        withheld = self.object_records.withhold(record_id);
                    }
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
            // Every proof made under the old epoch, and every chain cell, retires, so the
            // withheld ids are free again: a proof may name a record (a receiver's prototype
            // link, a holder) its cell's subjects do not.
            advance_prototype_layout_epoch();
            self.dependencies.retire_cells();
            self.object_records.release_withheld();
            self.class_space.release_withheld_stubs();
        }
    }

    /// After a full collection that swept `payload_before` payload bytes: returns the empty record
    /// pages (`RecordSpace::return_free_pages`), the whole pages of the property arena's released
    /// runs (`ValueArena::return_free_pages`), and the allocator's free memory when the sweep
    /// freed a lot (`release_freed_allocator_memory`).
    fn return_freed_memory(&mut self, payload_before: usize) {
        for family in self.record_families_mut() {
            family.return_free_pages();
        }
        self.properties.return_free_pages();
        let freed = payload_before.saturating_sub(self.payload_bytes);
        let share_freed = freed >= FREED_TRIM_SHARE_MIN_BYTES && freed.saturating_mul(4) > payload_before;
        if freed > FREED_TRIM_BYTES || share_freed {
            release_freed_allocator_memory();
        }
    }

    /// Frees the unallocated rest of each hot space's cursor run and refunds its charge
    /// (`RecordSpace::retire_cursor`), before a collection reads occupancy or `allocated`.
    fn retire_record_cursors(&mut self) {
        let unspent = self.object_records.retire_cursor()
            + self.array_records.retire_cursor()
            + self.function_records.retire_cursor()
            + self.environment_records.retire_cursor()
            + self.class_space.retire_cursors();
        self.allocated = self.allocated.saturating_sub(unspent);
    }

    /// Notes a collection's start: the cursors and bump run retire first, so the occupancy a
    /// full collection records is what it keeps.
    fn start_collection(&mut self, full: bool) -> bool {
        self.retire_record_cursors();
        self.retire_bump();
        full
    }

    /// The emptied buffer a collection's roots go into, kept from the last collection.
    pub(crate) fn take_mark_stack(&mut self) -> Vec<Value> { std::mem::take(&mut self.mark_stack) }

    /// Roots `code`'s folded identities at every collection while it is published.
    pub fn hold_folded_roots(&mut self, code: &std::rc::Rc<dyn crate::folded_roots::RootedCode>) {
        self.folded.register(code);
    }

    /// Appends the identities every published code of this heap embeds to a collection's `roots`.
    pub(crate) fn extend_folded_roots(&mut self, roots: &mut Vec<Value>) { self.folded.extend_roots(roots); }

    pub(crate) fn collect_with_object_roots(
        &mut self,
        roots: impl IntoIterator<Item = Value>,
        mut object_roots: impl FnMut(GcTraceKind, Value, u32, bool, &mut Vec<Value>),
        parallel_object_roots: Option<&ParallelObjectRootTrace<'_>>,
        closure_reads: Option<&dyn ClosureReadSource>,
        full: bool,
    ) -> Vec<(Value, Value)> {
        let full = self.start_collection(full);
        // `QUENCH_TRACE=gc` reports the mark, sweep, and record-sweep times.
        let trace_started = crate::trace::Channel::Gc.on().then(std::time::Instant::now);
        // No reference to a small string's view outlives the shared borrow it came from.
        self.small_views.clear();
        let age = self.promotion_age();
        // A minor collection that leaves survivors young carries the old owners that may
        // point at them to the next one (`carried_owners`): those the barriers logged, then
        // those it promotes.
        let carries_owners = !full && age.ages();
        // The shared unit strings are no roots: one the sweep frees must not be handed out.
        if let Some(strings) = &mut self.ascii_unit_strings {
            strings.fill(Value::UNDEFINED);
        }
        self.charge_materialized_text();
        // The sweep may free an array whose record a later array reuses.
        retire_array_storage_proofs();
        debug_assert_eq!(self.collection_state, CollectionState::Idle);
        self.collection_state = if full { CollectionState::FullMarking } else { CollectionState::MinorMarking };
        // Live slots grow only by allocation and shrink only by sweeping, so the peak is
        // observed here and when it is reported rather than on every allocation.
        self.peak_live = self.peak_live.max(self.live_slots());
        self.collections += 1;
        self.marked_prototypes = [None; MARKED_PROTOTYPE_WAYS];
        #[cfg(feature = "profile-aggregate")]
        let allocations_between_collections = self.allocated_since_collection() as u64;
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        let mark_started = std::time::Instant::now();
        #[cfg(feature = "profile-aggregate")]
        let mark_clear_started = std::time::Instant::now();
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        if full {
            self.gc_profile.full_collections += 1;
        } else {
            self.gc_profile.minor_collections += 1;
            #[cfg(feature = "profile-aggregate")]
            {
                self.gc_profile.remembered_owner_scans += self.remembered.len() as u64;
                self.gc_profile.young_cells_swept += self.young.count() as u64;
            }
        }
        if full {
            Self::clear_marks(&self.marks);
            self.remembered.clear();
            self.remembered_marks.fill(0);
            self.carried_owners.clear();
            // A full collection promotes every survivor, aged or not.
            self.aged.clear();
            for family in self.record_families_mut() {
                family.clear_marks();
                family.forget_ages();
            }
        }
        #[cfg(feature = "profile-aggregate")]
        if full {
            self.gc_profile.mark_clear_nanos += mark_clear_started.elapsed().as_nanos() as u64;
        }
        // The roots become the mark stack: collecting a vector's own iterator keeps its
        // buffer, so a collection holds one root-sized buffer, not a copy beside it.
        let mut work: Vec<Value> = roots.into_iter().collect();
        work.reserve(self.roots.slot_count());
        work.extend(self.roots.values());
        #[cfg(debug_assertions)]
        let verify_roots = work.clone();
        #[cfg(debug_assertions)]
        let verify_old_records = (!full).then(|| {
            let mut old = Vec::new();
            for family in self.record_families() {
                family.for_each_marked(&mut |id| old.push(family.value_of(id)));
            }
            old
        });
        // Under `carries_owners`, the old owners that may point at what stays young.
        let mut remembered_owners: Vec<Value> = Vec::new();
        if !full {
            #[cfg(feature = "profile-aggregate")]
            {
                self.gc_profile.remembered_owner_scans += self
                    .record_families()
                    .iter()
                    .map(|family| family.remembered_count())
                    .sum::<usize>() as u64;
            }
            // What the barriers logged since the last collection is carried to the next one
            // (`carried_owners`); what the last one carried is traced, then dropped.
            if carries_owners {
                self.aged.resize(self.marks.len(), 0);
                remembered_owners.extend(self.remembered.iter().map(|&index| Value::heap(index)));
            }
            // The newborn records are named while the carried owners are still marked old, so
            // only the barriers' log needs excluding from them.
            let written = self.record_families_mut().map(|family| {
                let written = family.take_remembered();
                family.prepare_minor(&written, age);
                written
            });
            for owner in std::mem::take(&mut self.carried_owners) {
                self.write_barrier(owner);
            }
            for (family, mut logged) in self.record_families_mut().into_iter().zip(written) {
                let written = logged.len();
                logged.extend(family.take_remembered());
                let first = work.len();
                work.extend(logged.into_iter().map(|id| family.value_of(id)));
                if carries_owners {
                    remembered_owners.extend_from_slice(&work[first..first + written]);
                }
            }
        }
        #[cfg(debug_assertions)]
        let verify_remembered_records: Vec<Value> = if full {
            Vec::new()
        } else {
            work[verify_roots.len()..].to_vec()
        };
        #[cfg(feature = "profile-aggregate")]
        {
            self.gc_profile.roots += work.len() as u64;
            self.gc_profile.max_worklist = self.gc_profile.max_worklist.max(work.len() as u64);
        }
        // A full collection traces closure environments through what closures can read, once
        // the live set has grown since the last that did: environments over-captured since
        // then are what grew it, and slots cleared before stay cleared. Between them the
        // trace's per-environment state would only add to each full collection's peak
        // (V8-v7 max RSS +50 to +156 KiB on most fixtures when every full collection traced).
        let trims_environments = full && self.environment_trim_due();
        let environment_reads = if trims_environments {
            EnvironmentReads::Closures
        } else {
            EnvironmentReads::Projections
        };
        let mut environments = EnvironmentTrace::new(closure_reads.filter(|_| full), environment_reads);
        {
            let mut ephemerons = weak::EphemeronWork::default();
            if !full {
                let properties = &self.properties;
                let remembered = &self.remembered;
                let mut last_proto = PushedProtos::default();
                for index in remembered.iter().copied() {
                    let value = Value::heap(index);
                    if let Some(cell) = self.get(value) {
                        Self::children(
                            value,
                            cell,
                            properties,
                            &mut work,
                            &mut object_roots,
                            GcTraceKind::Collection,
                            &mut last_proto,
                        );
                        let mark_bits = mark_bits!(self);
                        ephemerons.remembered(
                            value,
                            cell,
                            |target| mark_bits.is_marked(target),
                            &mut work,
                        );
                    }
                }
            }
            let parallel = self.parallel_mark(parallel_object_roots);
            self.mark_work(
                &mut work,
                &mut object_roots,
                parallel,
                &mut ephemerons,
                GcTraceKind::Collection,
                &mut environments,
            );
        }
        #[cfg(debug_assertions)]
        {
            if full {
                self.debug_verify_full(&verify_roots, &mut object_roots, closure_reads, environment_reads);
            } else {
                let mut pinned = verify_old_records.unwrap_or_default();
                pinned.extend(verify_remembered_records);
                self.debug_verify_minor(&verify_roots, &pinned, &mut object_roots);
            }
        }
        self.clear_unread_environment_slots(environments);
        self.mark_stack = { work.clear(); work };
        let finalization_jobs = self.prune_weak_entries(full, age);
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        {
            let mark_nanos = mark_started.elapsed().as_nanos() as u64;
            self.gc_profile.mark_nanos += mark_nanos;
            *if full { &mut self.gc_profile.full_mark_nanos } else { &mut self.gc_profile.minor_mark_nanos } += mark_nanos;
        }
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        let sweep_started = std::time::Instant::now();
        let payload_before_sweep = self.payload_bytes;
        let trace_sweep_started = trace_started.map(|_| std::time::Instant::now());
        // Property-arena owners are listed during the sweep only for a full collection that
        // already knows it compacts: the lists hold an entry per live owner at the heap's peak.
        // A compaction that only this sweep's releases make due is decided after the sweep and
        // lists the survivors then (`list_property_owners`), so it runs in the same collection.
        let compacts = full && self.properties.compaction_due();
        let sweep_pool = if full
            && !self.has_direct_records()
            && self.slots.len() >= PARALLEL_MARK_MINIMUM_CELLS
            && !cfg!(feature = "profile-memory")
        {
            parallel_mark::parallel_mark_pool()
        } else {
            None
        };
        let SweepOutput {
            mut live,
            mut live_payload_bytes,
            property_owners,
            freed_cells,
            swept_slots,
        } = if let Some(pool) = sweep_pool {
            self.sweep_full_parallel(pool, compacts)
        } else {
            self.sweep_serial(full, compacts, age, &mut remembered_owners)
        };
        if freed_cells > 0 {
            // A freed slot may next hold another cell (`view_epoch`): one bump for the sweep.
            buffer_bytes::bump_view_epoch();
        }
        // The direct objects whose young runs survive, by start (`ValueArena::evacuate_young`),
        // where no compaction moves every run.
        let mut young_owners: Vec<(usize, YoungOwner)> = Vec::new();
        // The arrays whose young runs of elements survive, copied out before the young
        // objects' runs move or compaction moves every run.
        let mut young_arrays: Vec<RecordId> = Vec::new();
        let mut narrowable_arrays: Vec<RecordId> = Vec::new();
        let narrow_arrays = self.narrow_runs;
        let mut direct_property_owners = Vec::with_capacity(if compacts {
            self.property_owner_counts.1
        } else {
            0
        });
        let legacy_live = live;
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        let records_started = std::time::Instant::now();
        let trace_records_started = trace_started.map(|_| std::time::Instant::now());
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        {
            self.gc_profile.sweep_phase_nanos[0] +=
                (records_started - sweep_started).as_nanos() as u64;
        }
        {
            #[cfg(feature = "profile-aggregate")]
            {
                self.gc_profile.sweep_slots +=
                    self.record_families()
                        .iter()
                        .map(|family| family.high_water())
                        .sum::<usize>() as u64;
            }
            // A full collection sweeps the strings on a mark-pool worker meanwhile, once
            // there are enough to pay for the hand-off.
            let string_worker = (full
                && !cfg!(feature = "profile-memory")
                && (self.parallel_mark_forced()
                    || self.string_records.live_count() + self.small_strings.live_count()
                        >= PARALLEL_MARK_MINIMUM_CELLS / 4))
                .then(parallel_mark::parallel_mark_pool)
                .flatten();
            let strings = StoppedStringSweep {
                records: &mut self.string_records,
                small: &mut self.small_strings,
                atoms: &mut self.string_atoms,
                full,
                age,
            };
            let mut string_out = StringSweepOutput::default();
            #[cfg(feature = "profile-memory")]
            let memory_profile = &mut self.memory_profile;
            let properties = &mut self.properties;
            let class_space = &mut self.class_space;
            let payload_bytes = &mut self.payload_bytes;
            let live_payload_bytes = &mut live_payload_bytes;
            let mut collection_counts = (0, 0);
            let mut sweep_rest = || {
                // Every young run's owner is a direct object or function (`YoungOwner`), so
                // their sweep shows its survivors, minor or full, and lists those it keeps.
                // A dead object's storage is released in place, and its stub with it; one without
                // extras, which its drop would free, is then forgotten.
                let young_runs = properties.young_runs();
                let dispose = |_record_id: RecordId, record: &OrdinaryRecord| {
                    #[cfg(feature = "profile-memory")]
                    memory_profile.freed_record_cell(Value::direct_object(record.stub), properties.len(record.object.properties()));
                    class_space.release_stub(record.stub);
                    record.object.release_storage(properties)
                };
                let (object_live, object_freed) = self.object_records.sweep_disposing(full, age, records::sweep_support::objects_visited(young_runs, compacts), dispose, |record| match record {
                    RecordSweep::Keep(record_id, record) => {
                        let object = &record.object;
                        if compacts && object.properties().has_compact_range() {
                            direct_property_owners.push((
                                object.properties().start_offset(),
                                Value::direct_object(record.stub),
                            ));
                        } else if young_runs.hold(object.properties()) {
                            note_young_owner(&mut young_owners, object.properties().start_offset(), YoungOwner::Object(record_id));
                        }
                    }
                    // Released in place: dropping it frees its extras.
                    RecordSweep::Dead(..) => {}
                });
                // After the records: a departed cell dies with its record (its stub released in `dispose`), so
                // the cell regions sweep only the cells whose Value is dead and not released yet.
                class_space.sweep_cells(full, age);
                // A function's own properties may take a young run too (`Heap::finished_run_object`):
                // after one did, its sweep shows its survivors and lists those it keeps.
                let young_function_runs = std::mem::take(&mut self.young_function_runs);
                let (function_live, function_freed) =
                    self.function_records.sweep(full, age, full || compacts || young_function_runs, |record| match record {
                        RecordSweep::Keep(record_id, function) => {
                            if compacts && function.object.properties().has_compact_range() {
                                direct_property_owners.push((
                                    function.object.properties().start_offset(),
                                    Value::direct_function(record_id),
                                ));
                            } else if young_runs.hold(function.object.properties()) {
                                note_young_owner(&mut young_owners, function.object.properties().start_offset(), YoungOwner::Function(record_id));
                            }
                        }
                        RecordSweep::Dead(_record_id, function) => {
                            #[cfg(feature = "profile-memory")]
                            let slots = properties.len(function.object.properties());
                            properties.release(function.object.properties());
                            #[cfg(feature = "profile-memory")]
                            memory_profile.freed_record_cell(Value::direct_function(_record_id), slots);
                        }
                    });
                let abandoned_contexts = &mut self.abandoned_contexts;
                let (promise_live, promise_freed) =
                    self.promise_records.sweep(full, age, full, |record| match record {
                        RecordSweep::Keep(record_id, promise) => {
                            if compacts && promise.object.properties().has_compact_range() {
                                direct_property_owners.push((
                                    promise.object.properties().start_offset(),
                                    Value::direct_promise(record_id),
                                ));
                            }
                        }
                        RecordSweep::Dead(_record_id, promise) => {
                            abandoned_contexts.abandon(&promise.record);
                            #[cfg(feature = "profile-memory")]
                            let slots = properties.len(promise.object.properties());
                            properties.release_ordinary(promise.object.properties());
                            #[cfg(feature = "profile-memory")]
                            memory_profile.freed_record_cell(Value::direct_promise(_record_id), slots);
                        }
                    });
                // An await state owns no arena storage; dropping a dead one frees its unresumed
                // continuation.
                let (await_live, await_freed) = self.await_records.sweep(full, age, false, |_| {});
                // A vector owns nothing beyond its record.
                let (v128_live, v128_freed) = self.v128_records.sweep(full, age, false, |_| {});
                let (iterator_live, iterator_freed) =
                    self.iterator_records.sweep(full, age, full, |record| match record {
                        RecordSweep::Keep(record_id, iterator) => {
                            if compacts && iterator.object.properties().has_compact_range() {
                                direct_property_owners.push((
                                    iterator.object.properties().start_offset(),
                                    Value::direct_iterator(record_id),
                                ));
                            }
                        }
                        RecordSweep::Dead(_record_id, iterator) => {
                            #[cfg(feature = "profile-memory")]
                            let slots = properties.len(iterator.object.properties());
                            properties.release_ordinary(iterator.object.properties());
                            #[cfg(feature = "profile-memory")]
                            memory_profile.freed_record_cell(Value::direct_iterator(_record_id), slots);
                        }
                    });
                let collection_records = &mut self.collection_records;
                collection_counts = collection_records.sweep(full, age, full, |record| match record {
                    RecordSweep::Keep(record_id, collection) => {
                        if full {
                            *live_payload_bytes += collection.entries.payload_bytes();
                        }
                        if compacts && collection.object.properties().has_compact_range() {
                            direct_property_owners.push((
                                collection.object.properties().start_offset(),
                                Value::direct_collection(record_id),
                            ));
                        }
                    }
                    RecordSweep::Dead(_record_id, collection) => {
                        #[cfg(feature = "profile-memory")]
                        let slots = properties.len(collection.object.properties());
                        properties.release_ordinary(collection.object.properties());
                        #[cfg(feature = "profile-memory")]
                        memory_profile.freed_record_cell(Value::direct_collection(_record_id), slots);
                    }
                });
                let mut dead_arrays = Vec::new();
                // Every young run of elements is a young array's (`ArrayElements::young`), so
                // their sweep, which shows its survivors minor or full, lists those it keeps.
                let young_arrays = &mut young_arrays;
                let narrowable_arrays = &mut narrowable_arrays;
                // A dead array is released in place; one owning nothing its drop frees is then
                // forgotten, as a young array whose elements its young cycle takes back is.
                let sparse = self.sparse_arrays.is_some();
                let dispose = |record_id: RecordId, array: &ArrayObject| {
                    // An empty array without extras releases nothing (`ArrayObject::owns_nothing`).
                    if !sparse && array.owns_nothing() && !cfg!(feature = "profile-memory") {
                        return false;
                    }
                    *payload_bytes = payload_bytes.saturating_sub(array_payload_bytes(array));
                    #[cfg(feature = "profile-memory")]
                    memory_profile.freed_record_cell(Value::direct_array(record_id), properties.len(array.properties()));
                    properties.release_ordinary(array.properties());
                    if sparse {
                        dead_arrays.push(Value::direct_array(record_id));
                    }
                    array.needs_drop()
                };
                self.array_records.warm_before_drop(records::sweep_support::warm_array_elements);
                let (array_live, array_freed) = self.array_records.sweep_disposing(full, age, true, dispose, |record| match record {
                    RecordSweep::Keep(record_id, array) => {
                        if full {
                            *live_payload_bytes += array_payload_bytes(array);
                        }
                        if array.elements().is_young() {
                            young_arrays.push(record_id);
                        } else if full && narrow_arrays && array.elements().may_narrow() {
                            narrowable_arrays.push(record_id);
                        }
                        if compacts && array.properties().has_compact_range() {
                            direct_property_owners.push((
                                array.properties().start_offset(),
                                Value::direct_array(record_id),
                            ));
                        }
                    }
                    // Released in place: dropping it frees what it owns.
                    RecordSweep::Dead(..) => {}
                });
                if let Some(arrays) = &mut self.sparse_arrays {
                    for array in dead_arrays {
                        arrays.remove(&array);
                    }
                    if arrays.is_empty() {
                        self.sparse_arrays = None;
                    }
                }
                (
                    (object_live, object_freed),
                    (function_live, function_freed),
                    (promise_live, promise_freed),
                    (await_live, await_freed),
                    (v128_live, v128_freed),
                    (iterator_live, iterator_freed),
                    (array_live, array_freed),
                )
            };
            let (
                (object_live, object_freed),
                (function_live, function_freed),
                (promise_live, promise_freed),
                (await_live, await_freed),
                (v128_live, v128_freed),
                (iterator_live, iterator_freed),
                (array_live, array_freed),
            ) = match string_worker {
                Some(pool) => pool.in_place_scope(|scope| {
                    let out = &mut string_out;
                    scope.spawn(move |_| *out = strings.run(true));
                    sweep_rest()
                }),
                None => {
                    string_out = strings.run(false);
                    sweep_rest()
                }
            };
            // The strings that share an `Rc` die on the collector thread.
            drop(std::mem::take(&mut string_out.rc_strings));
            drop(std::mem::take(&mut string_out.rc_small));
            self.payload_bytes = self.payload_bytes.saturating_sub(string_out.dead_bytes);
            *live_payload_bytes += string_out.live_bytes;
            let (string_live, string_freed) = (string_out.live, string_out.freed);
            let (collection_live, collection_freed) = collection_counts;
            // An environment owns no arena storage; dropping a dead one frees its slots.
            let (environment_live, environment_freed) = self.environment_records.sweep(full, age, false, |_| {});
            // A symbol owns no arena storage; a dead one releases its description, which
            // `alloc_symbol` charged as external bytes.
            let mut dead_description_bytes = 0;
            let (symbol_live, symbol_freed) = self.symbol_records.sweep(full, age, false, |record| {
                if let RecordSweep::Dead(_record_id, symbol) = record {
                    dead_description_bytes += symbol.description.as_ref().map_or(0, |description| description.len());
                    #[cfg(feature = "profile-memory")]
                    self.memory_profile.freed_symbol(Value::direct_symbol(_record_id));
                }
            });
            self.external_bytes = self.external_bytes.saturating_sub(dead_description_bytes);
            // A Wasm GC object's payload is external bytes: charged as its record is allocated, and
            // uncharged when the record is swept.
            let mut dead_wasm_gc_bytes = 0;
            let (wasm_gc_live, wasm_gc_freed) = self.wasm_gc_records.sweep(full, age, false, |record| {
                if let RecordSweep::Dead(_record_id, record) = record {
                    dead_wasm_gc_bytes += record.allocated_bytes();
                }
            });
            self.external_bytes = self.external_bytes.saturating_sub(dead_wasm_gc_bytes);
            live += symbol_live + environment_live + wasm_gc_live
                + object_live
                + array_live
                + function_live
                + promise_live
                + await_live
                + v128_live
                + iterator_live
                + collection_live
                + string_live;
            #[cfg(feature = "profile-aggregate")]
            {
                self.gc_profile.freed += (symbol_freed + environment_freed + wasm_gc_freed
                    + object_freed
                    + array_freed
                    + function_freed
                    + promise_freed
                    + await_freed
                    + v128_freed
                    + iterator_freed
                    + collection_freed
                    + string_freed) as u64;
            }
            #[cfg(not(feature = "profile-aggregate"))]
            let _ = (
                symbol_freed,
                environment_freed,
                wasm_gc_freed,
                object_freed,
                array_freed,
                function_freed,
                promise_freed,
                await_freed,
                v128_freed,
                iterator_freed,
                collection_freed,
                string_freed,
            );
        }
        #[cfg(feature = "profile-aggregate")]
        {
            self.gc_profile.sweep_slots += swept_slots as u64;
            self.gc_profile.freed += freed_cells as u64;
        }
        let _ = (freed_cells, swept_slots);
        if carries_owners {
            for family in self.record_families_mut() {
                remembered_owners.extend(family.take_promoted().into_iter().map(|id| family.value_of(id)));
            }
            self.carried_owners = remembered_owners;
        }
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        {
            self.gc_profile.sweep_phase_nanos[1] += records_started.elapsed().as_nanos() as u64;
        }
        let trace_records_us = trace_records_started.map_or(0, |started| started.elapsed().as_micros());
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        let compaction_started = std::time::Instant::now();
        if full {
            // A full sweep recounts the ledger from the survivors: shrinking a live array
            // (truncation, `pop`, `splice`) is not charged back between full collections.
            self.payload_bytes = live_payload_bytes;
        }
        for record_id in young_arrays {
            // SAFETY: the sweep listed these from the records it kept.
            let array = unsafe { self.array_records.get_live_mut_unbarriered(record_id) };
            // Native tiers read backings in place as wide values: only an interpreted run
            // keeps narrow ones (`narrow_runs`).
            if self.narrow_runs {
                array.survive_nursery();
            } else {
                array.leave_nursery();
            }
        }
        let (candidates, mut narrowed, mut saved_total) = (narrowable_arrays.len(), 0usize, 0usize);
        for record_id in narrowable_arrays {
            // SAFETY: the sweep listed these from the records it kept.
            let saved = unsafe { self.array_records.get_live_mut_unbarriered(record_id) }.narrow_elements();
            narrowed += usize::from(saved != 0);
            saved_total += saved;
            self.payload_bytes = self.payload_bytes.saturating_sub(saved);
        }
        if full && crate::trace::Channel::Gc.on() {
            eprintln!("[narrow-elements] candidates={candidates} narrowed={narrowed} saved={}KB", saved_total / 1024);
        }
        // A full collection whose sweep made a compaction due compacts now: the sweep listed no
        // owners, so they are listed from the survivors. Compaction moves the young survivors
        // with every other run and forgets the nursery, so the young evacuation is not needed.
        let late_compaction = !compacts && full && self.properties.compaction_due();
        let (property_owners, direct_property_owners) = if late_compaction {
            self.list_property_owners()
        } else {
            (property_owners, direct_property_owners)
        };
        if compacts || late_compaction {
            // Compaction moves the young survivors with every other run and forgets the
            // nursery (`ValueArena::finish_compaction`): due after the sweep, it stays due,
            // as nothing after the sweep takes arena values back.
            debug_assert!(self.properties.compaction_due());
            self.property_owner_counts = (property_owners.len(), direct_property_owners.len());
            self.compact_property_arena(property_owners, direct_property_owners);
        } else {
            self.evacuate_young_runs(young_owners);
            // The pass visits every object record: it pays where property storage is large
            // (8 MB and up), and is a cost of its own for a small arena of many objects.
            if full && self.narrow_runs && self.properties.resident_words() >= Self::NARROW_PASS_MIN_ARENA_WORDS {
                let resident = self.properties.resident_words();
                let (last, fulls) = self.narrow_pass;
                if resident > last + last / 8 || fulls >= 8 {
                    self.narrow_live_runs();
                    self.narrow_pass = (resident, 0);
                } else {
                    self.narrow_pass = (last.min(resident), fulls + 1);
                }
            }
        }
        if full {
            self.properties.settle_after_full();
        }
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        {
            self.gc_profile.sweep_phase_nanos[2] += compaction_started.elapsed().as_nanos() as u64;
            self.gc_profile.sweep_nanos += sweep_started.elapsed().as_nanos() as u64;
        }
        let trace_times = trace_started.zip(trace_sweep_started).map_or((0, 0), |(started, sweep)| {
            ((sweep - started).as_micros(), sweep.elapsed().as_micros())
        });
        // Live weight in the units allocation is charged in: one per cell or record plus
        // its payload.
        let live_weight = live + self.payload_bytes / payload_unit();
        let allocated = self.allocated_since_collection();
        self.allocated_at_collection = self.allocated;
        self.byte_due = false;
        self.external_since_collection = 0;
        self.external_collection_budget = self.external_bytes.max(self.pacing().minimum_external_budget);
        if full {
            self.allocated_at_full = self.allocated;
            self.reclaimable_program_bytes_since_full = 0;
            self.old_live = legacy_live;
            self.young.clear();
            self.remembered.clear();
            self.remembered_marks.fill(0);
        }
        if full {
            self.live_after_full = live;
            self.live_weight_after_full = live_weight;
        }
        if trims_environments && closure_reads.is_some() {
            self.live_at_environment_trim = Some(live);
        }
        let since_full = self.allocated_since_full();
        let mut aged = std::mem::take(&mut self.aged_fates);
        for family in self.record_families_mut() {
            aged.add(family.take_aged_fates());
        }
        let next = self.pacing().plan(
            &mut self.pacing_state,
            pacing::CollectionOutcome {
                full,
                live,
                live_weight,
                previous_live_weight: self.live_weight_after_previous,
                live_after_full: self.live_after_full,
                live_weight_after_full: self.live_weight_after_full,
                allocated,
                since_full,
                aged,
            },
        );
        if crate::trace::Channel::Gc.on() {
            let (mark_us, sweep_us) = trace_times;
            // Resident set size at this collection's end, so a pacing change can be judged
            // by its peak from one traced run.
            let rss_kb = std::fs::read_to_string("/proc/self/statm")
                .ok()
                .and_then(|statm| statm.split_whitespace().nth(1)?.parse::<usize>().ok())
                .map_or(0, |pages| pages * 4);
            eprintln!("gc rss_kb={rss_kb} live_weight={live_weight} full={full} mark_us={mark_us} sweep_us={sweep_us} records_us={trace_records_us} live={live} prev={} after_full={} alloc={} since_full={} next={next:?} state={:?} {}", self.live_weight_after_previous, self.live_after_full, allocated, self.allocated_since_full(), self.pacing_state, self.properties.trace_summary());
        }
        if full && crate::trace::Channel::StorageCensus.on() {
            self.trace_storage_census();
        }
        self.live_weight_after_previous = live_weight;
        self.set_young_budget(next.threshold);
        self.full_due = next.full;
        #[cfg(feature = "profile-aggregate")]
        {
            self.gc_profile.allocations_between_collections_total +=
                allocations_between_collections;
            self.gc_profile.survivors_total += live as u64;
            if self.collections == 1 {
                self.gc_profile.allocations_between_collections_min =
                    allocations_between_collections;
                self.gc_profile.allocations_between_collections_max =
                    allocations_between_collections;
                self.gc_profile.survivors_min = live as u64;
                self.gc_profile.survivors_max = live as u64;
            } else {
                self.gc_profile.allocations_between_collections_min =
                    self.gc_profile.allocations_between_collections_min.min(allocations_between_collections);
                self.gc_profile.allocations_between_collections_max =
                    self.gc_profile.allocations_between_collections_max.max(allocations_between_collections);
                self.gc_profile.survivors_min = self.gc_profile.survivors_min.min(live as u64);
                self.gc_profile.survivors_max = self.gc_profile.survivors_max.max(live as u64);
            }
        }
        self.peak_survivors = self.peak_survivors.max(live);
        self.retire_dead_prototypes();
        if full {
            self.return_freed_memory(payload_before_sweep);
        }
        self.collection_state = CollectionState::Idle;
        #[cfg(feature = "profile-memory")]
        self.memory_profile.collection_finished();
        finalization_jobs
    }
    fn sweep_full_parallel(&mut self, pool: &ThreadPool, compacts: bool) -> SweepOutput {
        self.collection_state = CollectionState::Sweeping;
        let slot_len = self.slots.len();
        let mut slabs = self
            .slots
            .slabs_mut()
            .map(|(base, slots)| ParallelSweepSlab::new(base, slots))
            .collect::<Vec<_>>();
        let mut output = SweepOutput::default();

        for batch in slabs.chunks_mut(PARALLEL_SWEEP_SLAB_BATCH_SIZE) {
            let results = {
                let marks = &self.marks;
                pool.install(|| {
                    batch
                        .par_iter_mut()
                        .map(|slab| slab.sweep(marks, compacts))
                        .collect::<Vec<_>>()
                })
            };

            for result in results {
                let ParallelSweepResult {
                    slab,
                    live,
                    property_owners,
                    free_slots,
                    released_properties,
                    rc_cells,
                    external_bytes,
                    live_payload_bytes,
                    abandoned_contexts,
                    swept_slots,
                } = result;
                self.abandoned_contexts.append(abandoned_contexts);
                let freed_cells = free_slots.len();
                if freed_cells > 0 {
                    let slab_free = &mut self.free_by_slab[slab as usize];
                    if slab_free.is_empty() {
                        self.free_slabs.push(slab);
                    }
                    slab_free.merge(&free_slots);
                    self.free_slots += freed_cells;
                    for properties in released_properties {
                        self.properties.release(properties);
                    }
                    self.external_bytes = self.external_bytes.saturating_sub(external_bytes);
                }
                for cell in rc_cells {
                    cell.drop_on_owner();
                }
                output.live += live;
                output.live_payload_bytes += live_payload_bytes;
                output.property_owners.extend(property_owners);
                output.freed_cells += freed_cells;
                output.swept_slots += swept_slots;
            }
        }
        debug_assert_eq!(output.swept_slots, slot_len);
        output
    }

    /// Sweeps the slots: every slot for a `full` collection, else the young ones, aging and
    /// promoting their survivors under `age` and listing in `promoted_cells` the aged cells it
    /// promotes.
    fn sweep_serial(
        &mut self,
        full: bool,
        compacts: bool,
        age: PromotionAge,
        promoted_cells: &mut Vec<Value>,
    ) -> SweepOutput {
        let mut live = 0;
        let mut live_payload_bytes = 0;
        let mut freed_cells = 0;
        let mut swept_slots = 0;
        let properties = &mut self.properties;
        let mut property_owners_for_sweep = Vec::with_capacity(if compacts {
            self.property_owner_counts.0
        } else {
            0
        });
        {
            let external_bytes = &mut self.external_bytes;
            let payload_bytes = &mut self.payload_bytes;
            let abandoned_contexts = &mut self.abandoned_contexts;
            let free_by_slab = &mut self.free_by_slab;
            let free_slabs = &mut self.free_slabs;
            let free_slot_count = &mut self.free_slots;
            #[cfg(feature = "profile-memory")]
            let memory_profile = &mut self.memory_profile;
            // Frees `slot`'s cell, if it has one. A BigInt, the cell most often freed where
            // BigInt arithmetic runs, owns nothing beyond its slot but spilled digits, which its
            // own drop releases: no payload or external bytes, no properties, no cell move.
            let mut release = |properties: &mut ValueArena, index: usize, slot: &mut Slot| -> bool {
                let _ = index; // the memory profile's alone (`profile-memory`)
                match &slot.cell {
                    None => return false,
                    Some(Cell::BigInt(_)) => {
                        #[cfg(feature = "profile-memory")]
                        memory_profile.freed(Value::heap(index as u32), slot.cell.as_ref().unwrap(), None);
                        if let Some(Cell::BigInt(value)) = &mut slot.cell {
                            // SAFETY: dropped once; the slot then holds no cell.
                            unsafe { std::ptr::drop_in_place(value) };
                        }
                        // SAFETY: the cell's only owned memory was released above.
                        unsafe { std::ptr::write(&mut slot.cell, None) };
                    }
                    Some(_) => {
                        let cell = slot.cell.take().unwrap();
                        #[cfg(feature = "profile-memory")]
                        {
                            let object_slots = cell
                                .object()
                                .map(|object| properties.len(object.properties()));
                            memory_profile.freed(Value::heap(index as u32), &cell, object_slots);
                        }
                        *external_bytes = external_bytes.saturating_sub(cell.external_bytes());
                        *payload_bytes = payload_bytes.saturating_sub(cell.payload_bytes());
                        if let Cell::Promise(promise) = &cell {
                            abandoned_contexts.abandon(&promise.record);
                        }
                        if let Some(object) = cell.object() {
                            properties.release(object.properties());
                        }
                    }
                }
                true
            };
            // Lists the slots `bits` names in the heap's word `word` of slots as free.
            let mut free_word = |word: usize, bits: u64| {
                let slab = word / FREE_SLOT_WORDS;
                let slab_free = &mut free_by_slab[slab];
                if slab_free.is_empty() {
                    free_slabs.push(slab as u32);
                }
                slab_free.insert_word(word % FREE_SLOT_WORDS, bits);
                *free_slot_count += bits.count_ones() as usize;
                freed_cells += bits.count_ones() as usize;
            };
            self.collection_state = CollectionState::Sweeping;
            if full {
                for (base, slab) in self.slots.slabs_mut() {
                    for (offset, slot) in slab.iter_mut().enumerate() {
                        let index = base + offset;
                        swept_slots += 1;
                        if Self::marked(&self.marks, index) {
                            live += 1;
                            live_payload_bytes += slot.cell.as_ref().map_or(0, Cell::payload_bytes);
                            if let Some(object) = slot.cell.as_ref().and_then(Cell::object)
                                && compacts && properties.has_compact_range(object.properties()) {
                                    property_owners_for_sweep.push(pack_offset_owner(
                                        object.properties().start_offset(),
                                        index,
                                    ));
                                }
                        } else if release(properties, index, slot) {
                            free_word(index / u64::BITS as usize, 1_u64 << (index % u64::BITS as usize));
                        }
                    }
                }
            } else {
                // Each young cell is freed, promoted (left marked), or, when `age` ages it on
                // its first survival, aged: unmarked and young for one more minor collection.
                // An aged cell is freed or promoted whatever `age` is now.
                let slots = &mut self.slots;
                let young = self.young.sweep(
                    &self.marks,
                    &mut self.aged,
                    age,
                    promoted_cells,
                    |index| slots.get_mut(index).is_some_and(|slot| release(properties, index, slot)),
                    &mut free_word,
                );
                swept_slots += young.swept;
                let (promoted, stays_young) = (young.promoted, young.stays_young);
                let aged_fates = young.aged_fates;
                for index in self.remembered.iter().copied() {
                    let index = index as usize;
                    self.remembered_marks[index / 64] &= !(1 << (index % 64));
                }
                self.remembered.clear();
                self.old_live = self.old_live.saturating_add(promoted);
                live = self.old_live + stays_young;
                self.aged_fates.add(aged_fates);
            }
        }
        SweepOutput {
            live,
            live_payload_bytes,
            property_owners: property_owners_for_sweep,
            freed_cells,
            swept_slots,
        }
    }

    #[cfg(debug_assertions)]
    fn snapshot_record_marks(&self) -> Vec<Vec<u64>> {
        self.record_families()
            .iter()
            .map(|family| family.snapshot_marks())
            .collect()
    }

    #[cfg(debug_assertions)]
    fn restore_record_marks(&mut self, marks: &[Vec<u64>]) {
        for (family, marks) in self.record_families_mut().into_iter().zip(marks) {
            family.restore_marks(marks);
        }
    }

    #[cfg(debug_assertions)]
    fn debug_verify_full(
        &mut self,
        roots: &[Value],
        object_roots: &mut impl FnMut(GcTraceKind, Value, u32, bool, &mut Vec<Value>),
        closure_reads: Option<&dyn ClosureReadSource>,
        environment_reads: EnvironmentReads,
    ) {
        let parallel_marks = Self::snapshot_marks(&self.marks);
        let parallel_record_marks = self.snapshot_record_marks();
        #[cfg(feature = "profile-aggregate")]
        let profile = self.gc_profile;
        Self::clear_marks(&self.marks);
        for family in self.record_families_mut() {
            family.clear_marks();
        }
        let mut work = roots.to_vec();
        let mut ephemerons = weak::EphemeronWork::default();
        self.mark_work(
            &mut work,
            object_roots,
            None,
            &mut ephemerons,
            GcTraceKind::VerifyFull,
            &mut EnvironmentTrace::new(closure_reads, environment_reads),
        );
        let sequential_marks = Self::snapshot_marks(&self.marks);
        let sequential_record_marks = self.snapshot_record_marks();
        let agree = |sequential: &[u64], collector: &[u64]| sequential == collector;
        assert!(agree(&sequential_marks, &parallel_marks) && sequential_record_marks.iter().zip(&parallel_record_marks).all(|(s, c)| agree(s, c)), "full mark misses what the single-threaded verifier reaches");
        Self::restore_marks(&self.marks, &parallel_marks);
        self.restore_record_marks(&parallel_record_marks);
        self.collection_state = CollectionState::FullMarking;
        #[cfg(feature = "profile-aggregate")]
        {
            self.gc_profile = profile;
        }
    }

    #[cfg(debug_assertions)]
    fn debug_verify_minor(
        &mut self,
        roots: &[Value],
        old_records: &[Value],
        object_roots: &mut impl FnMut(GcTraceKind, Value, u32, bool, &mut Vec<Value>),
    ) {
        let minor_marks = Self::snapshot_marks(&self.marks);
        let minor_record_marks = self.snapshot_record_marks();
        let young: Vec<usize> = self.young.indices().collect();
        #[cfg(feature = "profile-aggregate")]
        let profile = self.gc_profile;
        Self::clear_marks(&self.marks);
        for family in self.record_families_mut() {
            family.clear_marks();
        }
        let pinned_records: rustc_hash::FxHashSet<Value> = old_records.iter().copied().collect();
        let mut young_slots = vec![false; self.slots.len()];
        for index in young.iter().copied() {
            young_slots[index] = true;
        }
        let mut work = roots.to_vec();
        work.extend(self.slots.iter().enumerate().filter_map(|(index, slot)| {
            (slot.cell.is_some() && !young_slots[index] && Self::marked_words(&minor_marks, index))
                .then_some(Value::heap(index as u32))
        }));
        work.extend(old_records.iter().copied());
        let mut ephemerons = weak::EphemeronWork::default();
        self.collection_state = CollectionState::FullMarking;
        self.mark_work(
            &mut work,
            object_roots,
            None,
            &mut ephemerons,
            GcTraceKind::VerifyFull,
            &mut EnvironmentTrace::new(None, EnvironmentReads::Closures),
        );
        let verified_record_marks = self.snapshot_record_marks();
        let marked = |marks: &[u64], id: RecordId| {
            marks[id as usize / u64::BITS as usize] & (1_u64 << (id as usize % u64::BITS as usize)) != 0
        };
        for ((family, minor), verified) in self
            .record_families()
            .into_iter()
            .zip(&minor_record_marks)
            .zip(&verified_record_marks)
        {
            family.for_each_live_id(&mut |id| {
                let identity = family.value_of(id);
                assert!(
                    pinned_records.contains(&identity) || marked(minor, id) == marked(verified, id),
                    "minor collection's young records differ from a full mark with old records pinned: {identity:?}"
                );
            });
        }
        self.restore_record_marks(&minor_record_marks);
        for index in young {
            let index = index as usize;
            if self.get(Value::heap(index as u32)).is_some() {
                assert_eq!(
                    Self::marked(&self.marks, index),
                    Self::marked_words(&minor_marks, index),
                    "minor collection's young live set differs from a full mark with old cells pinned: {index}"
                );
            }
        }
        Self::restore_marks(&self.marks, &minor_marks);
        self.collection_state = CollectionState::MinorMarking;
        #[cfg(feature = "profile-aggregate")]
        {
            self.gc_profile = profile;
        }
    }

    /// Marks everything `work` reaches, with the closure environments it reaches through
    /// `environments` and the weak map values through `ephemerons`. With `parallel`, work
    /// larger than its `round_minimum` is marked in parallel rounds (`mark_work_in_rounds`);
    /// smaller work is marked on this thread, value by value (`drain_work`), as is all of it
    /// without. Either way marking ends when no work and no environment edge is left.
    // Kept out of `collect_with_object_roots`: inlined there, the visit loop shares registers
    // with the whole collection, and cachegrind counted spills in every visit.
    #[inline(never)]
    fn mark_work(
        &mut self,
        work: &mut Vec<Value>,
        object_roots: &mut impl FnMut(GcTraceKind, Value, u32, bool, &mut Vec<Value>),
        parallel: Option<parallel_mark::ParallelMark<'_>>,
        ephemerons: &mut weak::EphemeronWork,
        trace_kind: GcTraceKind,
        environments: &mut EnvironmentTrace<'_>,
    ) {
        let mut ctx = MarkCtx {
            work,
            object_roots,
            ephemerons,
            trace_kind,
            environments,
            last_proto: PushedProtos::default(),
        };
        let Some(parallel) = parallel else {
            self.drain_work::<false, _>(&mut ctx, 0);
            return;
        };
        self.mark_work_in_rounds(&mut ctx, parallel);
    }

    /// `mark_work` with a parallel mark: drains work until a round is due, runs the round,
    /// and visits the environments its workers left before draining again.
    // Kept out of line: `collect_with_object_roots` then inlines one serial visit
    // (`mark_value`), and with it the children walks, as before parallel rounds existed.
    #[inline(never)]
    fn mark_work_in_rounds<R: FnMut(GcTraceKind, Value, u32, bool, &mut Vec<Value>)>(
        &mut self,
        ctx: &mut MarkCtx<'_, '_, R>,
        parallel: parallel_mark::ParallelMark<'_>,
    ) {
        let mut left_to_collector = Vec::new();
        while self.drain_work::<true, _>(ctx, parallel.round_minimum) {
            self.mark_parallel_round(
                parallel,
                ctx.work,
                &mut left_to_collector,
                ctx.object_roots,
                ctx.ephemerons,
                ctx.trace_kind,
                ctx.environments,
            );
            // A worker counted the visit of an environment it left.
            while let Some(environment) = left_to_collector.pop() {
                self.mark_value(environment, ctx);
            }
        }
    }

    /// Marks `work` value by value until no work and no environment edge is left (`false`)
    /// or, with `ROUNDS`, until more than `round_minimum` values wait (`true`): a parallel
    /// round then takes them. Without `ROUNDS` the loop tests nothing per value but the
    /// stack, as the serial mark did before rounds existed.
    #[inline(always)]
    fn drain_work<const ROUNDS: bool, R>(
        &mut self,
        ctx: &mut MarkCtx<'_, '_, R>,
        round_minimum: usize,
    ) -> bool
    where
        R: FnMut(GcTraceKind, Value, u32, bool, &mut Vec<Value>),
    {
        loop {
            if ROUNDS && ctx.work.len() > round_minimum {
                return true;
            }
            let Some(value) = ctx.work.pop() else {
                if !ctx.environments.has_pending() {
                    return false;
                }
                self.trace_environment_edges(ctx.environments, ctx.work);
                continue;
            };
            #[cfg(feature = "profile-aggregate")]
            {
                self.gc_profile.work_items += 1;
            }
            self.mark_value(value, ctx);
            #[cfg(feature = "profile-aggregate")]
            {
                self.gc_profile.max_worklist = self.gc_profile.max_worklist.max(ctx.work.len() as u64);
            }
        }
    }

    /// The record id of the ordinary object whose stub is `granule` (`class_space::stub_record_at`).
    #[inline(always)]
    pub(super) fn stub_record(&self, granule: u32) -> u32 {
        self.class_space.record_of(granule)
    }

    /// Marks `value` when it is a heap value not marked yet, pushing the values it keeps
    /// alive onto `work`: the serial mark's visit of one value. Objects, arrays, functions and
    /// strings are visited here; the rarer kinds are visited by `mark_rare_value`.
    #[inline]
    fn mark_value<R>(&mut self, value: Value, ctx: &mut MarkCtx<'_, '_, R>)
    where
        R: FnMut(GcTraceKind, Value, u32, bool, &mut Vec<Value>),
    {
        if let Some(object_ref) = value.direct_object_index() {
            // A live class cell is traced by its class and fields, and marked in its own region.
            if let Some(class) = self.class_space.live_class(object_ref.granule()) {
                self.mark_class_cell(value, object_ref.granule(), class, ctx);
                return;
            }
            // Marking a direct object marks its record: the stub has no mark bit of its own. A departed cell
            // keeps its granule's mark while its Value lives (`ClassSpace::mark_cell`, a no-op for a stub).
            self.class_space.mark_cell(object_ref.granule());
            let record_id = self.stub_record(object_ref.granule());
            if !self.object_records.mark(record_id) {
                return;
            }
            #[cfg(feature = "profile-aggregate")]
            {
                self.gc_profile.marked += 1;
                self.gc_profile.marked_kinds[CellKind::Object as usize] += 1;
            }
            // SAFETY: the direct handle is live and its mark bit was newly set.
            let record = unsafe { self.object_records.get_live(record_id) };
            Self::object_children(
                value,
                &record.object,
                &self.properties,
                ctx.work,
                ctx.object_roots,
                ctx.trace_kind,
                &mut ctx.last_proto,
            );
            self.notify_ephemerons_marked(value, None, ctx.ephemerons, ctx.work);
        } else if let Some(record_id) = value.direct_array_index() {
            if !self.array_records.mark(record_id) {
                return;
            }
            #[cfg(feature = "profile-aggregate")]
            {
                self.gc_profile.marked += 1;
                self.gc_profile.marked_kinds[CellKind::Array as usize] += 1;
            }
            // SAFETY: a direct Array Value names an initialized record.
            let array = unsafe { self.array_records.get_live(record_id) };
            let string_records = &mut self.string_records;
            Self::array_children(
                value,
                array,
                &self.properties,
                |string| string_parts::mark_flat(string_records, string),
                ctx.work,
                ctx.object_roots,
                ctx.trace_kind,
                &mut ctx.last_proto,
            );
            if let Some(arrays) = self.sparse_arrays.as_ref()
                && let Some(elements) = arrays.get(&value)
            {
                ctx.work.extend(elements.values.values().copied());
            }
            self.notify_ephemerons_marked(value, None, ctx.ephemerons, ctx.work);
        } else if let Some(record_id) = value.direct_function_index() {
            if !self.function_records.mark(record_id) {
                return;
            }
            #[cfg(feature = "profile-aggregate")]
            {
                self.gc_profile.marked += 1;
                self.gc_profile.marked_kinds[CellKind::Function as usize] += 1;
            }
            // SAFETY: a direct function Value names an initialized record.
            let function = unsafe { self.function_records.get_live(record_id) };
            Self::function_children(
                value,
                function,
                &self.properties,
                ctx.work,
                ctx.object_roots,
                ctx.trace_kind,
                &mut ctx.last_proto,
            );
            ctx.environments.closure(function.env, &function.kind, ctx.work);
            self.notify_ephemerons_marked(value, None, ctx.ephemerons, ctx.work);
        } else if let Some(record_id) = value.direct_string_index() {
            if let Some(local) = small_strings::small_string_local(record_id) {
                self.mark_small_string(local, ctx.work);
                return;
            }
            // A string holds no edges but its parts': marking it and tracing them is the visit.
            if self.string_records.mark(record_id) {
                string_parts::trace_parts(&mut self.string_records, record_id, ctx.work);
                #[cfg(feature = "profile-aggregate")]
                {
                    self.gc_profile.marked += 1;
                    self.gc_profile.marked_kinds[CellKind::String as usize] += 1;
                }
            }
        } else {
            self.mark_rare_value(value, ctx);
        }
    }

    /// Marks live class cell `granule` (of class `class`) once, and traces its edges.
    #[inline(never)]
    fn mark_class_cell<R>(&mut self, value: Value, granule: u32, class: u32, ctx: &mut MarkCtx<'_, '_, R>)
    where
        R: FnMut(GcTraceKind, Value, u32, bool, &mut Vec<Value>),
    {
        if !self.class_space.mark_cell(granule) {
            return;
        }
        #[cfg(feature = "profile-aggregate")]
        {
            self.gc_profile.marked += 1;
            self.gc_profile.marked_kinds[CellKind::Object as usize] += 1;
        }
        let fields = self.class_space.cell_fields(granule);
        Self::cell_children_marking(value, class, fields, |_, _, _| false, ctx.work, ctx.object_roots, ctx.trace_kind, &mut ctx.last_proto);
        self.notify_ephemerons_marked(value, None, ctx.ephemerons, ctx.work);
    }

    /// Marks a value of a kind `mark_value` does not visit inline (iterators, collections,
    /// vectors, environments, Wasm GC objects, symbols, awaits, promises and legacy cells),
    /// with the same visit. Out of line, so the common visit stays small.
    #[inline(never)]
    fn mark_rare_value<R>(&mut self, value: Value, ctx: &mut MarkCtx<'_, '_, R>)
    where
        R: FnMut(GcTraceKind, Value, u32, bool, &mut Vec<Value>),
    {
        if let Some(record_id) = value.direct_iterator_index() {
            if !self.iterator_records.mark(record_id) {
                return;
            }
            #[cfg(feature = "profile-aggregate")]
            {
                self.gc_profile.marked += 1;
                self.gc_profile.marked_kinds[CellKind::Iterator as usize] += 1;
            }
            // SAFETY: a direct iterator Value names an initialized record.
            let iterator = unsafe { self.iterator_records.get_live(record_id) };
            Self::object_children(
                value,
                &iterator.object,
                &self.properties,
                ctx.work,
                ctx.object_roots,
                ctx.trace_kind,
                &mut ctx.last_proto,
            );
            Self::iterator_edges(iterator, ctx.work);
            self.notify_ephemerons_marked(value, None, ctx.ephemerons, ctx.work);
        } else if let Some(record_id) = value.direct_collection_index() {
            if !self.collection_records.mark(record_id) {
                return;
            }
            #[cfg(feature = "profile-aggregate")]
            {
                self.gc_profile.marked += 1;
                let kind = match unsafe { self.collection_records.get_live(record_id) }.entries {
                    CollectionStorage::Map(_) => CellKind::Map,
                    CollectionStorage::Set(_) => CellKind::Set,
                };
                self.gc_profile.marked_kinds[kind as usize] += 1;
            }
            // SAFETY: a direct collection Value names an initialized record.
            let collection = unsafe { self.collection_records.get_live(record_id) };
            Self::object_children(
                value,
                &collection.object,
                &self.properties,
                ctx.work,
                ctx.object_roots,
                ctx.trace_kind,
                &mut ctx.last_proto,
            );
            Self::collection_edges(&collection.entries, ctx.work);
            self.notify_ephemerons_marked(value, None, ctx.ephemerons, ctx.work);
        } else if let Some(record_id) = value.direct_v128_index() {
            // A vector holds no heap edges: marking it is the whole visit.
            if self.v128_records.mark(record_id) {
                #[cfg(feature = "profile-aggregate")]
                {
                    self.gc_profile.marked += 1;
                    self.gc_profile.marked_kinds[CellKind::WasmV128 as usize] += 1;
                }
            }
        } else if let Some(record_id) = value.direct_environment_index() {
            // A full collection traces environments by what their readers read.
            // SAFETY: a direct environment Value names an initialized record while it is live.
            let function = unsafe { self.environment_records.get_live(record_id) }.function;
            if ctx.environments.whole(value, record_id, function) {
                return;
            }
            if !self.environment_records.mark(record_id) {
                return;
            }
            #[cfg(feature = "profile-aggregate")]
            {
                self.gc_profile.marked += 1;
                self.gc_profile.marked_kinds[CellKind::Environment as usize] += 1;
            }
            // SAFETY: as above.
            Self::environment_children(unsafe { self.environment_records.get_live(record_id) }, ctx.work, &mut ctx.last_proto);
        } else if let Some(record_id) = value.direct_wasm_gc_index() {
            // The payload holds the references the object keeps: marking it pushes them.
            if self.wasm_gc_records.mark(record_id) {
                #[cfg(feature = "profile-aggregate")]
                {
                    self.gc_profile.marked += 1;
                    self.gc_profile.marked_kinds[CellKind::WasmGc as usize] += 1;
                }
                // SAFETY: just marked, so occupied.
                unsafe { self.wasm_gc_records.get_live(record_id) }.trace_children(ctx.work);
                // A Wasm GC object may key a weak map (a JS cache of its wrapper): the value that
                // waits on it is told that it is marked.
                self.notify_ephemerons_marked(value, None, ctx.ephemerons, ctx.work);
            }
        } else if let Some(record_id) = value.direct_symbol_index() {
            // A symbol holds no edges: marking it is the whole visit, and a weak key's ephemeron
            // is told that it is marked.
            if self.symbol_records.mark(record_id) {
                #[cfg(feature = "profile-aggregate")]
                {
                    self.gc_profile.marked += 1;
                    self.gc_profile.marked_kinds[CellKind::Symbol as usize] += 1;
                }
                self.notify_ephemerons_marked(value, None, ctx.ephemerons, ctx.work);
            }
        } else if let Some(record_id) = value.direct_await_index() {
            if !self.await_records.mark(record_id) {
                return;
            }
            #[cfg(feature = "profile-aggregate")]
            {
                self.gc_profile.marked += 1;
                self.gc_profile.marked_kinds[CellKind::AwaitState as usize] += 1;
            }
            // SAFETY: a direct await Value names an initialized record.
            unsafe { self.await_records.get_live(record_id) }.extend_roots(ctx.work);
        } else if let Some(record_id) = value.direct_promise_index() {
            if !self.promise_records.mark(record_id) {
                return;
            }
            #[cfg(feature = "profile-aggregate")]
            {
                self.gc_profile.marked += 1;
                self.gc_profile.marked_kinds[CellKind::Object as usize] += 1;
            }
            // SAFETY: a direct promise Value names an initialized record.
            let promise = unsafe { self.promise_records.get_live(record_id) };
            Self::object_children(
                value,
                &promise.object,
                &self.properties,
                ctx.work,
                ctx.object_roots,
                ctx.trace_kind,
                &mut ctx.last_proto,
            );
            promise.record.append_roots(ctx.work);
            self.notify_ephemerons_marked(value, None, ctx.ephemerons, ctx.work);
        } else {
            let Some(index) = value.legacy_heap_index().map(|value| value as usize) else {
                return;
            };
            let Some(slot) = self.slots.get_mut(index) else {
                return;
            };
            if slot.cell.is_none() {
                return;
            }
            if !Self::mark_serial(&self.marks, index) {
                return;
            }
            let cell = slot.cell.as_ref().unwrap();
            #[cfg(feature = "profile-aggregate")]
            {
                self.gc_profile.marked += 1;
                self.gc_profile.marked_kinds[Self::cell_kind(cell) as usize] += 1;
            }
            Self::children(
                value,
                cell,
                &self.properties,
                ctx.work,
                ctx.object_roots,
                ctx.trace_kind,
                &mut ctx.last_proto,
            );
            ephemeron_tail!(self, ctx, value, Some(cell));
        }
    }

    /// Traces the parts of a small string and counts them when the string survives.
    #[inline(never)]
    fn mark_small_string(&mut self, local: RecordId, work: &mut Vec<Value>) {
        if self.small_strings.mark(local) {
            small_strings::trace_small(&self.small_strings, &mut self.string_records, local, work);
            #[cfg(feature = "profile-aggregate")]
            {
                self.gc_profile.marked += 1;
                self.gc_profile.marked_kinds[CellKind::String as usize] += 1;
            }
        }
    }

    /// Tells the weak map values waiting on a newly marked `value` (`EphemeronWork::newly_marked`).
    #[inline(never)]
    fn notify_ephemerons_marked(&self, value: Value, cell: Option<&Cell>, ephemerons: &mut weak::EphemeronWork, work: &mut Vec<Value>) {
        let mark_bits = mark_bits!(self);
        ephemerons.newly_marked(value, cell, |target| mark_bits.is_marked(target), work);
    }

    #[inline(always)]
    fn marked(marks: &[AtomicU64], index: usize) -> bool {
        marks[index / 64].load(Ordering::Relaxed) & (1 << (index % 64)) != 0
    }
    #[inline(always)]
    fn mark_serial(marks: &[AtomicU64], index: usize) -> bool {
        let mask = 1_u64 << (index % u64::BITS as usize);
        mark_word_if_new(&marks[index / u64::BITS as usize], mask)
    }
    #[cfg(debug_assertions)]
    fn marked_words(marks: &[u64], index: usize) -> bool {
        marks[index / 64] & (1 << (index % 64)) != 0
    }
    #[cfg(debug_assertions)]
    fn snapshot_marks(marks: &[AtomicU64]) -> Vec<u64> {
        marks
            .iter()
            .map(|word| word.load(Ordering::Relaxed))
            .collect()
    }
    fn clear_marks(marks: &[AtomicU64]) {
        for word in marks {
            word.store(0, Ordering::Relaxed);
        }
    }
    #[cfg(debug_assertions)]
    fn restore_marks(marks: &[AtomicU64], words: &[u64]) {
        debug_assert_eq!(marks.len(), words.len());
        for (mark, value) in marks.iter().zip(words) {
            mark.store(*value, Ordering::Relaxed);
        }
    }
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
    /// owner-attached VM tables): `owner` is logged as if it had been written.
    pub(crate) fn write_barrier(&mut self, owner: Value) {
        match owner.heap_reference() {
            Some(HeapReference::ObjectRecord(granule)) => match self.class_space.live_class(granule) {
                Some(_) => self.class_space.barrier(granule),
                None => {
                    let record_id = self.stub_record(granule);
                    self.object_records.barrier(record_id)
                }
            },
            Some(HeapReference::ArrayRecord(record_id)) => self.array_records.barrier(record_id),
            Some(HeapReference::FunctionRecord(record_id)) => {
                self.function_records.barrier(record_id)
            }
            Some(HeapReference::PromiseRecord(record_id)) => self.promise_records.barrier(record_id),
            Some(HeapReference::AwaitRecord(record_id)) => self.await_records.barrier(record_id),
            Some(HeapReference::EnvironmentRecord(record_id)) => self.environment_records.barrier(record_id),
            Some(HeapReference::IteratorRecord(record_id)) => {
                self.iterator_records.barrier(record_id)
            }
            Some(HeapReference::CollectionRecord(record_id)) => {
                self.collection_records.barrier(record_id)
            }
            Some(HeapReference::WasmGcRecord(record_id)) => self.wasm_gc_records.barrier(record_id),
            Some(HeapReference::LegacySlot(index)) => self.remember(index as usize),
            // Strings and vectors are immutable and hold no edges, so nothing is ever stored
            // into them.
            Some(HeapReference::StringRecord(_) | HeapReference::V128Record(_) | HeapReference::SymbolRecord(_)) | None => {}
        }
    }
    pub(crate) fn sparse_set(&mut self, array: Value, index: usize, value: Value) {
        self.note_prototype_elements(array);
        self.write_barrier(array);
        if let Some(record) = self.object_record_mut(array) {
            record.mark_sparse_elements();
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
        if let Some(record) = self.object_record_mut(array) {
            record.mark_sparse_elements();
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
    #[inline(always)] pub fn property_get(&self, object: &Object, slot: usize) -> Option<Value> {
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
    pub unsafe fn property_get_shaped(&self, object: &Object, slot: usize) -> Option<Value> {
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
        // SAFETY: the caller's; the staged hit takes the arena form first, on one compare.
        unsafe { crate::staged::concrete_own_field_hit(object, self, shape, slot) }
    }
    /// `own_field_hit` of the global object, whose values may live behind extras
    /// (`staged::concrete_global_field_hit`).
    /// # Safety
    /// As for `own_field_hit`.
    #[inline(always)]
    pub(crate) unsafe fn global_field_hit(&self, object: &Object, shape: u32, slot: u16) -> Option<Value> {
        unsafe { crate::staged::concrete_global_field_hit(object, self, shape, slot) }
    }
    /// `own_field_hit` through a lane record's own-field hint (`Object::hint_shape`): `None`
    /// on another exact shape or storage form.
    /// # Safety
    /// As for `own_field_hit`.
    #[inline(always)]
    pub(crate) unsafe fn own_field_hint_hit<const FORM: u32>(&self, object: &Object, hint: u32, slot: u16) -> Option<Value> {
        unsafe { crate::staged::concrete_hinted_own_field_hit::<FORM>(object, self, hint, slot) }
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
        let data = self.object(object)?;
        // A class cell has no arena run to point into: it declines, as an inline value does.
        let vector = data.record_only()?.properties();
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

    /// The store of `value` to field `slot` of the live class cell `object` (a fixed class's instance, R4-F), in place:
    /// the word `set_leading_properties` stores for a slot, under the cell's shape, which already names the slot.
    pub(crate) fn set_fixed_cell_slot(&mut self, object: Value, slot: usize, value: Value) {
        let granule = self.live_class_cell(object).expect("a fixed class's instance is a live class cell");
        self.note_value_slot(object, slot, value);
        let word = self.stored_slot_word(object, slot, value);
        self.class_cell_store(granule, slot, word);
    }

    /// `property_set` of each of `values` at slot `0..INLINE_PROPERTY_COUNT` of `object`,
    /// whose shape has those slots, with one lookup and one write barrier: one payload store
    /// when they pack there (`Object::store_packed_inline_values`), the arena's slots when
    /// they live there, else `property_set` of each.
    pub(crate) fn set_leading_properties(&mut self, object: Value, values: [Value; INLINE_PROPERTY_COUNT]) {
        for (slot, value) in values.into_iter().enumerate() {
            self.note_value_slot(object, slot, value);
        }
        let values: [Value; INLINE_PROPERTY_COUNT] = std::array::from_fn(|slot| self.stored_slot_word(object, slot, values[slot]));
        if let Some(granule) = self.live_class_cell(object) {
            for (slot, value) in values.into_iter().enumerate() {
                self.class_cell_store(granule, slot, value);
            }
            return;
        }
        // The barrier remembers the owner of any reference stored; one test covers each.
        let stored = values.into_iter().find(|value| value.heap_index().is_some()).unwrap_or(Value::UNDEFINED);
        let record = match self.object_store_mut(object, stored).expect("property owner is an object") {
            // The class cell stored in place above, or it departed when a store broke its representation.
            ObjectMut::Cell(_) => unreachable!("a class cell that declined a store has departed"),
            ObjectMut::Record(record) => record,
        };
        record.note_constant_write();
        if record.store_packed_inline_values(values) {
            return;
        }
        let vector = record.properties();
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
    pub fn set_narrow_runs(&mut self, narrow: bool) {
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
        self.rewrite_widened_numbers();
    }

    /// The word a store of `value` writes at `slot` of `object` (`ValueArena::stored_word` of the
    /// object's exact shape), read after `note_value_slot` observed `value`.
    #[inline(always)]
    fn stored_slot_word(&self, object: Value, slot: usize, value: Value) -> Value {
        if !self.properties.tracks() { return value; }
        match self.object(object) {
            Some(data) => {
                let shape = data.exact_shape();
                if slot < self.properties.shape_length(shape) { self.properties.stored_word(shape, slot, value) } else { value }
            }
            None => value,
        }
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
            let word = properties.stored_word(data.exact_shape(), slot, value);
            properties.set(vector, slot, word);
        } else if !data.store_inline_narrow(slot, value) {
            self.property_set(object, slot, value);
        }
        self.rewrite_widened_numbers();
    }

    pub fn property_set(&mut self, object: Value, slot: usize, value: Value) {
        self.note_value_slot(object, slot, value);
        let value = self.stored_slot_word(object, slot, value);
        if let Some(granule) = self.live_class_cell(object) {
            self.class_cell_store(granule, slot, value);
            return;
        }
        let vector = {
            let record = match self.object_store_mut(object, value).expect("property owner is an object") {
                // The class cell stored in place above, or it departed when a store broke its representation.
                ObjectMut::Cell(_) => unreachable!("a class cell that declined a store has departed"),
                ObjectMut::Record(record) => record,
            };
            record.note_value_write(slot);
            let vector = record.properties();
            if !vector.has_inline_property_storage() {
                Some(vector)
            } else if record.store_inline_narrow(slot, value) {
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
        self.properties.keep_shape_wide(vector);
        let vector = self.object_record_mut(object).expect("a narrow run is a record's").properties();
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
        let value = self.stored_slot_word(object, slot, value);
        if let Some(granule) = self.live_class_cell(object) {
            self.class_cell_store(granule, slot, value);
            return;
        }
        let vector = {
            let record = match self.object_store_mut(object, value).expect("property owner is an object") {
                // The class cell stored in place above, or it departed when a store broke its representation.
                ObjectMut::Cell(_) => unreachable!("a class cell that declined a store has departed"),
                ObjectMut::Record(record) => record,
            };
            record.note_value_write(slot);
            let vector = record.properties();
            if !vector.has_inline_property_storage() {
                Some(vector)
            } else if record.store_inline_narrow(slot, value) {
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
        if !LOGS {
            if self.object_records.store_logs(record_id, value) {
                return false;
            }
            // SAFETY: the caller proves the record live; the barrier admitted the store.
            return unsafe { self.record_property_set_unlogged(record_id, slot, value, layout) };
        }
        // SAFETY: the caller proves the record live; `&mut self` is exclusive.
        let data = unsafe { self.object_records.get_live_mut_storing(record_id, value) };
        let stored = store_exact_slot(data, &mut self.properties, slot, value, layout);
        self.rewrite_widened_numbers();
        stored
    }

    /// Whether the write barrier logs a store of `value` into the object record `record_id`
    /// (`RecordSpace::store_logs`). A caller runs this once per store and passes the verdict
    /// on: `record_property_set_unlogged` for a store it did not log.
    #[inline(always)]
    pub(crate) fn object_store_logs(&self, record_id: u32, value: Value) -> bool {
        self.object_records.store_logs(record_id, value)
    }

    /// The store of `value` at `slot` of the object record `record_id` with the barrier already
    /// run: `record_property_set_flat::<false>` without a second barrier test, for a store
    /// `object_store_logs` did not log. Changes nothing a barrier would log.
    ///
    /// # Safety
    /// `record_id` names a live object record; `object_store_logs(record_id, value)` is false.
    #[inline(always)]
    pub(crate) unsafe fn record_property_set_unlogged(&mut self, record_id: u32, slot: usize, value: Value, layout: PropertyStoreLayout) -> bool {
        // SAFETY: the caller's contract; `&mut self` is exclusive.
        let data = unsafe { self.object_records.get_live_mut_unbarriered(record_id) };
        let stored = store_exact_slot(data, &mut self.properties, slot, value, layout);
        self.rewrite_widened_numbers();
        stored
    }

    /// A tracked arena's store through a lane record's hint, the layout id `hint`
    /// (`Object::hint_shape`) at `slot`: where the record's layout is the hint, its form the
    /// arena's (the payload is the run's start), the barrier logs nothing and the slot's owner
    /// already holds `value`'s class (`ValueArena::held_slot_admits`), one owner lookup and one
    /// class test, no join and no settling. Otherwise the unbarriered store, as before; false
    /// leaves the logged or widening store to the caller. The barrier is tested once, here.
    ///
    /// # Safety
    /// `record_id` names a live object record; the hint's slot is one its shape holds.
    #[inline(always)]
    pub(crate) unsafe fn hinted_tracked_store(&mut self, record_id: u32, hint: u32, slot: u16, value: Value) -> bool {
        // SAFETY: the caller's contract.
        let record = unsafe { &*self.unlogged_object_record(record_id) };
        if record.layout_id() != hint {
            return false;
        }
        // The barrier's one test for this store: a logged store is the caller's.
        if self.object_store_logs(record_id, value) {
            return false;
        }
        let form = crate::staged::ic::layout_form(hint);
        if form == 0
            // SAFETY: the matched layout's shape holds `slot`, in the arena form; the barrier admitted the store.
            && unsafe { self.held_arena_store(record_id, crate::staged::ic::layout_shape(hint), slot, value) } {
            return true;
        }
        // Packed inline values: where the slot's owner already holds the value's class, the store
        // widens nothing, so it is the value's narrow form into its half of the payload word
        // (`store_exact_slot`'s packed arm, without the join). A widening or
        // unnarrowable store, or a marked object, takes the joining store below.
        if form == NATIVE_OBJECT_LAYOUT.inline_form && !record.holds_constant_mark()
            // SAFETY: the matched layout's shape holds `slot`.
            && unsafe { self.properties.held_slot_admits(crate::staged::ic::layout_shape(hint), usize::from(slot), value) } {
            if let Some(narrow) = value.to_narrow() {
                // SAFETY: the caller's contract; the layout's form is packed inline storage; the barrier admitted the store.
                let data = unsafe { self.object_records.get_live_mut_unbarriered(record_id) };
                return data.store_packed_narrow(usize::from(slot), Some(narrow));
            }
        }
        // SAFETY: as above; the barrier admitted the store.
        unsafe { self.record_property_set_unlogged(record_id, usize::from(slot), value, PropertyStoreLayout::Preserve) }
    }

    /// The store of `value` at `slot` of the arena-form record `record_id` of exact `shape`
    /// where the slot's owner holds `value`'s class: one owner lookup, one class test, the
    /// write. False changes nothing, for the joining store. The barrier is not tested
    /// here: the caller runs `object_store_logs` and calls this only for a store it did not log.
    ///
    /// # Safety
    /// `record_id` names a live object record of exact `shape` whose values are a plain arena
    /// run (`Object::plain_arena_start`), `shape` holds `slot`, and `object_store_logs(record_id,
    /// value)` is false.
    #[inline(always)]
    pub(crate) unsafe fn held_arena_store(&mut self, record_id: u32, shape: u32, slot: u16, value: Value) -> bool {
        // SAFETY: the caller's contract.
        if !unsafe { self.properties.held_slot_admits(shape, usize::from(slot), value) } {
            return false;
        }
        // A Number-typed slot admits an int32 and holds its raw double (`ValueArena::stored_word`).
        let value = self.properties.stored_word(shape, usize::from(slot), value);
        // SAFETY: as above; the barrier admitted the write.
        let record = unsafe { &*self.unlogged_object_record(record_id) };
        // A marked object's store retires its readers first (`store_exact_slot`).
        if record.holds_constant_mark() {
            return false;
        }
        let start = record.proved_arena_start();
        // SAFETY: the run holds the shape's slots.
        unsafe { self.properties.set_flat_at(start, usize::from(slot), value) };
        true
    }

    /// Whether the arena keeps slot representations, which observe every store (`ValueArena::tracks`).
    pub fn tracks_slot_representations(&self) -> bool {
        self.properties.tracks()
    }

    /// The held type a tracked store row through the hint `(layout, slot)` is installed for
    /// (`slot_store::HELD_*`): the type slot `slot` of the layout's shape holds now, where a row
    /// keeps a store of it in place (an arena-form hint, `hinted_arena`, while tracking); else
    /// `HELD_NONE`, the tracked row's store.
    pub fn tracked_store_held(&self, layout: u32, slot: u16) -> u8 {
        use crate::staged::slot_store::{hinted_arena, HELD_INT32, HELD_NONE, HELD_OBJECT, HELD_OBJECT_OR_NULL, HELD_TAGGED};
        use crate::typed_ir::SlotType;
        if !self.properties.tracks() || !hinted_arena(layout, false) { return HELD_NONE; }
        let shape = crate::staged::ic::layout_shape(layout);
        if usize::from(slot) >= self.properties.shape_length(shape) { return HELD_NONE; }
        match self.properties.slot_observation(shape, usize::from(slot)) {
            SlotType::Int32 => HELD_INT32,
            SlotType::DirectObject => HELD_OBJECT,
            SlotType::DirectObjectOrNull => HELD_OBJECT_OR_NULL,
            SlotType::Tagged => HELD_TAGGED,
            _ => HELD_NONE,
        }
    }

    /// The base addresses of the arena's slot owner tables: `shape_chains`, `chain_owners`,
    /// `owner_types` (`ValueArena::owner_table_bases`), valid until the arena next grows them.
    #[inline(always)]
    pub(crate) fn owner_table_bases(&self) -> [u64; 3] {
        self.properties.owner_table_bases()
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
        self.rewrite_widened_numbers();
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
    pub fn property_add(&mut self, object: Value, value: Value, shape: u32) {
        let _slot = if let Some(object_ref) = object.direct_object_index() {
            // An add appends to the record's arena run: a live class cell departs first.
            self.depart_for_mutation(object);
            let record_id = self.stub_record(object_ref.granule());
            let Self {
                object_records,
                properties,
                ..
            } = &mut *self;
            // SAFETY: direct-object Values name live records; `&mut self` is exclusive.
            let data = unsafe { object_records.get_live_mut_storing(record_id, value) };
            let slot = properties.len(data.properties());
            append_property(data, properties, slot, value, shape, 0);
            slot
        } else {
            let (data, properties) = self
                .object_and_arena_mut(object)
                .expect("property owner is an object");
            let slot = properties.len(data.properties());
            append_property(data, properties, slot, value, shape, 0);
            slot
        };
        self.rewrite_widened_numbers();
        #[cfg(feature = "profile-memory")]
        self.note_object_slots(object, _slot + 1);
    }

    /// `property_add` to a direct object's record at `slot`, which the caller proved its
    /// storage length (a cached add's recorded slot), without deriving it again.
    #[inline(always)]
    pub(crate) fn record_property_add_at(&mut self, object: Value, slot: usize, value: Value, shape: u32) {
        self.record_property_add_wanting(object, slot, value, shape, 0);
    }

    /// `record_property_add_at` whose move, if the add makes one, takes the run capacity `want`
    /// (`ValueArena::grow`); zero takes the next class.
    #[inline(always)]
    fn record_property_add_wanting(&mut self, object: Value, slot: usize, value: Value, shape: u32, want: usize) {
        let Some(object_ref) = object.direct_object_index() else {
            return self.property_add(object, value, shape);
        };
        // An add appends to the record's arena run: a live class cell departs first.
        self.depart_for_mutation(object);
        let record_id = self.stub_record(object_ref.granule());
        let Self { object_records, properties, .. } = &mut *self;
        // SAFETY: direct-object Values name live records; `&mut self` is exclusive.
        let data = unsafe { object_records.get_live_mut_storing(record_id, value) };
        debug_assert_eq!(slot, properties.len(data.properties()));
        append_property(data, properties, slot, value, shape, want);
        self.rewrite_widened_numbers();
        #[cfg(feature = "profile-memory")]
        self.note_object_slots(object, slot + 1);
    }

    /// `record_property_add_at` out of line, for an add whose storage moves or whose store
    /// the write barrier logs, so a caller's plain appends keep their code call-free. `want` is
    /// the arena capacity the moved run reserves (`Vm::predicted_run_capacity`): the run takes
    /// the size the shape tree predicts for the object instead of only the next class, so an
    /// object that grows by several properties moves once rather than at each class.
    #[inline(never)]
    pub(crate) fn record_property_add_moving(&mut self, object: Value, slot: usize, value: Value, shape: u32, want: usize) {
        self.record_property_add_wanting(object, slot, value, shape, want);
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
        self.rewrite_widened_numbers();
        #[cfg(feature = "profile-memory")]
        let value = self.object_value(record_id);
        self.memory_profile.object_slots_changed(value, slot + 1);
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
        let object_id = identity.direct_object_index().and_then(|object| self.record_id(object));
        let Self {
            object_records,
            array_records,
            function_records,
            promise_records,
            iterator_records,
            symbol_records,
            collection_records,
            wasm_gc_records,
            slots,
            properties,
            memory_profile,
            ..
        } = self;
        if let Some(record_id) = object_id {
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
        if let Some(record_id) = identity.direct_symbol_index() {
            // SAFETY: profile runs immediately after inserting this symbol.
            let symbol = unsafe { symbol_records.get_live(record_id) };
            memory_profile.allocated_symbol(identity, symbol);
            return;
        }
        if let Some(record_id) = identity.direct_wasm_gc_index() {
            // SAFETY: profile runs immediately after inserting this Wasm GC object.
            let record = unsafe { wasm_gc_records.get_live(record_id) };
            memory_profile.allocated_wasm_gc(identity, record);
            return;
        }
        if let Some(record_id) = identity.direct_collection_index() {
            // SAFETY: profile runs immediately after inserting this collection.
            let collection = unsafe { collection_records.get_live(record_id) };
            let slots = properties.len(collection.object.properties());
            memory_profile.allocated_collection(identity, collection, slots);
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
    /// Push the values `object` keeps alive. Objects that share a prototype push it once:
    /// `last_proto`, the prototypes pushed last in this walk, are already on the work list or
    /// marked, so a second push would only be popped again.
    #[inline(always)]
    fn object_children(
        owner: Value,
        object: &Object,
        properties: &ValueArena,
        work: &mut Vec<Value>,
        object_roots: &mut impl FnMut(GcTraceKind, Value, u32, bool, &mut Vec<Value>),
        trace_kind: GcTraceKind,
        last_proto: &mut PushedProtos,
    ) {
        Self::object_children_marking(owner, object, properties, |_, _, _| false, work, object_roots, trace_kind, last_proto);
    }

    /// `object_children`, marking each inline string value through `mark_string` where the
    /// trace finds it (`Heap::mark_leaf`), as an array's string elements are: a string's only
    /// edges are its parts, which `mark_string` marks too, so it never enters the work list
    /// (V8-v7 Splay: a string per leaf object, a sixth of a full collection's pops).
    #[allow(clippy::too_many_arguments)]
    #[inline(always)]
    fn object_children_marking(
        owner: Value,
        object: &Object,
        properties: &ValueArena,
        mut mark_leaf: impl FnMut(Value, &mut Vec<Value>, &mut PushedProtos) -> bool,
        work: &mut Vec<Value>,
        object_roots: &mut impl FnMut(GcTraceKind, Value, u32, bool, &mut Vec<Value>),
        trace_kind: GcTraceKind,
        last_proto: &mut PushedProtos,
    ) {
        // The common object: inline values, no extras. Its prototype is tested by link first,
        // and its values by narrow code, before anything is decoded. One loop takes either
        // kind of inline values, so `mark_leaf` stays inlined at a single site.
        let inline = if let Some((link, narrows)) = object.plain_inline_trace() {
            if last_proto.first_link(link) {
                let proto = link.prototype();
                if proto.is_heap() && last_proto.first_push(proto) {
                    work.push(proto);
                }
            }
            object_roots(trace_kind, owner, object.shape(), false, work);
            Some(narrows.map(|narrow| if Value::narrow_is_heap(narrow) { Value::from_narrow(narrow) } else { Value::UNDEFINED }))
        } else {
            let proto = object.proto();
            if proto.is_heap() && last_proto.first_push(proto) {
                work.push(proto);
            }
            object_roots(trace_kind, owner, object.shape(), object.was_used_as_prototype(), work);
            object.visit_extras_roots(|value| work.push(value));
            object.inline_properties()
        };
        if let Some(values) = inline {
            for value in values {
                if value.is_heap() && !value.is_deleted() && !mark_leaf(value, work, last_proto) {
                    work.push(value);
                }
            }
        } else {
            properties.append_heap_references(object.properties(), work);
        }
    }

    /// The edges of live class cell `owner` (of class `class`, with fields `fields`), pushed as an object's are
    /// (`object_children_marking`): the class's prototype once per run of siblings, the class's shape roots,
    /// then each heap field that `mark_leaf` does not mark. A cell is never a prototype (its first use departs
    /// it), so the shape roots carry no prototype flag. Slack fields hold `Value::DELETED`, which is no edge.
    #[allow(clippy::too_many_arguments)]
    #[inline(always)]
    fn cell_children_marking(
        owner: Value,
        class: u32,
        fields: &[Value],
        mut mark_leaf: impl FnMut(Value, &mut Vec<Value>, &mut PushedProtos) -> bool,
        work: &mut Vec<Value>,
        object_roots: &mut impl FnMut(GcTraceKind, Value, u32, bool, &mut Vec<Value>),
        trace_kind: GcTraceKind,
        last_proto: &mut PushedProtos,
    ) {
        let link = class_link(class);
        if last_proto.first_link(link) {
            let proto = link.prototype();
            if proto.is_heap() && last_proto.first_push(proto) {
                work.push(proto);
            }
        }
        object_roots(trace_kind, owner, class_shape(class), false, work);
        for &value in fields {
            if value.is_heap() && !value.is_deleted() && !mark_leaf(value, work, last_proto) {
                work.push(value);
            }
        }
    }

    /// An environment's edges: its parent (once per run of siblings), the heap values of its
    /// slots, and its scope's dynamic bindings and `with` objects.
    #[inline(always)]
    pub(super) fn environment_children(environment: &EnvironmentRecord, work: &mut Vec<Value>, last_proto: &mut PushedProtos) {
        let EnvironmentRecord { parent, slots, scope, .. } = environment;
        if last_proto.first_shared(*parent) {
            work.push(*parent);
        }
        // Only heap values are edges: a slot's number or `undefined` costs a pop.
        slots.extend_heap_roots(work);
        let (dynamic_bindings, with_objects) = (scope.dynamic_bindings(), scope.with_objects());
        match dynamic_bindings {
            EnvironmentBindings::Owned(bindings) => {
                work.extend(bindings.iter().map(|(_, value)| *value));
            }
            EnvironmentBindings::Shared(owner) => work.push(*owner),
        }
        work.extend(with_objects.iter().copied());
    }

    fn children(
        owner: Value,
        cell: &Cell,
        properties: &ValueArena,
        work: &mut Vec<Value>,
        object_roots: &mut impl FnMut(GcTraceKind, Value, u32, bool, &mut Vec<Value>),
        trace_kind: GcTraceKind,
        last_proto: &mut PushedProtos,
    ) {
        let mut object = |object: &Object| {
            Self::object_children(
                owner,
                object,
                properties,
                work,
                object_roots,
                trace_kind,
                last_proto,
            );
        };
        if let Some((value, buffer)) = cell.typed_array_backing() {
            object(value);
            work.push(buffer);
            return;
        }
        match cell {
            Cell::ArrayBuffer { object: value, .. } => object(value),
            Cell::RegExp {
                object: value,
                meta,
                ..
            } => {
                object(value);
                work.push(meta.legacy_constructor.constructor());
            }
            Cell::DataView {
                object: value,
                buffer,
                ..
            } => {
                object(value);
                work.push(*buffer);
            }
            Cell::ShadowRealm {
                object: value,
                caller_global,
                realm_global,
            } => {
                object(value);
                work.extend([*caller_global, *realm_global]);
            }
            Cell::WeakMap { object: value, .. } | Cell::WeakSet { object: value, .. } => {
                object(value);
            }
            Cell::WeakRef { object: value, .. } => object(value),
            Cell::FinalizationRegistry {
                object: value,
                callback,
                entries,
            } => {
                object(value);
                work.push(*callback);
                work.extend(entries.iter().map(|entry| entry.held));
            }
            Cell::AwaitState(state) => state.extend_roots(work),
            Cell::ArrayFromAsyncState(state) => {
                work.extend([state.output, state.result, state.this_arg]);
                work.extend(state.iterator);
                work.extend(state.mapper);
                work.extend(state.array_like.map(|(source, _)| source));
            }
            Cell::Proxy {
                object: value,
                target,
                handler,
                ..
            } => {
                object(value);
                work.extend([*target, *handler]);
            }
            Cell::WasmGlobal { header, bits, ty, .. } => {
                object(&header.object);
                work.extend((WasmStorage::of(*ty) == WasmStorage::Handle).then(|| Value::from_bits(*bits)));
            }
            Cell::BindingReference { environment, .. } => work.push(*environment),
            Cell::WasmElements(elements) => work.extend(elements.iter().copied()),
            Cell::WasmTable { object: value, elements, .. } => {
                object(value);
                work.extend(elements.iter().copied());
            }
            Cell::WasmExtern(value) => work.push(*value),
            Cell::AccessorPair { getter, setter } => {
                work.push(*getter);
                work.push(*setter);
            }
            Cell::WasmException { tag, payload } => {
                work.push(*tag);
                work.extend(payload.iter().copied());
            }
            Cell::PromiseResolvingState { promise, .. } => work.push(*promise),
            Cell::Promise(promise) => {
                object(&promise.object);
                promise.record.append_roots(work);
            }
            Cell::Date { object: value, .. }
            | Cell::TemporalDuration { object: value, .. }
            | Cell::TemporalPlainDate { object: value, .. }
            | Cell::TemporalPlainDateTime { object: value, .. }
            | Cell::TemporalPlainMonthDay { object: value, .. }
            | Cell::TemporalPlainYearMonth { object: value, .. }
            | Cell::TemporalZonedDateTime { object: value, .. }
            | Cell::TemporalInstant { object: value, .. }
            | Cell::WasmTag { object: value, .. } => object(value),
            Cell::BigInt(_)
            | Cell::Error(_)
            | Cell::WasmHostFunction {
                target: crate::heap::WasmHostTarget::Host(_),
                ..
            }
            | Cell::WasmBits64(_) | Cell::WasmInstance(_) | Cell::WasmV128(_) => {}
            Cell::WasmHostFunction {
                target: crate::heap::WasmHostTarget::Js(target),
                ..
            } => work.push(*target),
            Cell::WasmHostFunction {
                target: crate::heap::WasmHostTarget::JsEmscriptenMemcpy(target),
                ..
            } => {
                work.push(target.function);
                work.push(target.view_environment);
            }
            Cell::WasmObject { object: value, slot } => {
                object(value);
                if let crate::heap::WasmObjectSlot::Instance { exports } = slot {
                    work.push(*exports);
                }
            }
            Cell::WasmMemory { header, buffer, .. } => {
                object(&header.object);
                work.push(*buffer);
            }
            _ => unreachable!("typed array backing handled above"),
        }
    }

    /// A direct Array record traces its object part, then its heap-valued elements.
    // Inlined into each mark visit, as the serial mark had it: left to its heuristics, LLVM
    // calls it from `collect_with_object_roots`, which cachegrind measured at more instructions.
    #[inline(always)]
    fn array_children(
        owner: Value,
        array: &ArrayObject,
        properties: &ValueArena,
        mut mark_string: impl FnMut(RecordId) -> bool,
        work: &mut Vec<Value>,
        object_roots: &mut impl FnMut(GcTraceKind, Value, u32, bool, &mut Vec<Value>),
        trace_kind: GcTraceKind,
        last_proto: &mut PushedProtos,
    ) {
        Self::object_children(
            owner,
            array,
            properties,
            work,
            object_roots,
            trace_kind,
            last_proto,
        );
        // The count of non-int32 values is kept per backing: none means no element to scan.
        if array.elements().holds_only_int32() || array.elements().double_values().is_some() { return; }
        // A run of one value (an array filled with one object or function) pushes it once:
        // a pushed value is marked before the collection ends, and each repeat cost a pop.
        let mut last = Value::UNDEFINED;
        let mut push = |value: Value, work: &mut Vec<Value>| {
            if value != last && !Self::mark_leaf(&mut mark_string, value) {
                work.push(value);
            }
            last = value;
        };
        let elements = array.elements();
        if let Some(values) = elements.wide_values() {
            for &value in values {
                if value.is_heap() {
                    push(value, work);
                }
            }
        } else if let Some(words) = elements.narrow_words() {
            for &word in words {
                if Value::narrow_is_heap(word) {
                    push(Value::from_narrow(word), work);
                }
            }
        } else {
            for value in elements.iter() {
                if value.is_heap() {
                    push(value, work);
                }
            }
        }
    }

    /// Marks `value` where the trace finds it, through `mark_string`, when it is a string, and
    /// says whether it was one. A string holds no heap edges, so marking it is the whole visit
    /// (`mark_value`): an array of strings adds nothing to the mark stack, which had held a
    /// value per element at the heap's peak (V8-v7 RegExp: 141 KiB). Profiling builds leave
    /// every value to `mark_value`, which counts each visit.
    fn mark_leaf(mark_string: &mut impl FnMut(RecordId) -> bool, value: Value) -> bool {
        if cfg!(feature = "profile-aggregate") {
            return false;
        }
        let Some(record_id) = value.direct_string_index() else {
            return false;
        };
        mark_string(record_id)
    }
    /// A Map's or Set's edges besides its object: its keys and values, or its values.
    fn collection_edges(entries: &CollectionStorage, work: &mut Vec<Value>) {
        match entries {
            CollectionStorage::Map(entries) => work.extend(entries.iter().flat_map(|(key, value)| [key, value])),
            CollectionStorage::Set(entries) => work.extend(entries.iter()),
        }
    }
    /// An iterator's edges besides its object: its source, cached next method, helper
    /// state and suspended generator.
    fn iterator_edges(iterator: &IteratorRecord, work: &mut Vec<Value>) {
        let IteratorExt {
            next_method,
            helper,
            generator,
            ..
        } = &iterator.ext;
        work.push(iterator.source);
        work.extend(*next_method);
        if let Some(helper) = helper {
            match helper.as_ref() {
                IteratorHelper::Map { callback, .. }
                | IteratorHelper::Filter { callback, .. }
                | IteratorHelper::FlatMap { callback, .. } => work.push(*callback),
                IteratorHelper::RegExpStringMatchAll { input, .. } => work.push(*input),
                IteratorHelper::Take { .. } | IteratorHelper::Drop { .. } => {}
                IteratorHelper::Concat {
                    items,
                    methods,
                    opened,
                    active,
                    ..
                } => {
                    work.extend(items.iter().copied());
                    work.extend(methods.iter().copied());
                    work.extend(opened.iter().flatten().copied());
                    work.extend(*active);
                }
                IteratorHelper::Zip {
                    iterators,
                    padding,
                    keys,
                    ..
                } => {
                    work.extend(iterators.iter().copied());
                    work.extend(padding.iter().copied());
                    work.extend(keys.iter().flatten().copied());
                }
            }
            match helper.as_ref() {
                IteratorHelper::FlatMap {
                    inner: Some(inner), ..
                } => work.push(*inner),
                IteratorHelper::FlatMap { inner: None, .. } => {}
                IteratorHelper::Zip { .. }
                | IteratorHelper::Map { .. }
                | IteratorHelper::Filter { .. }
                | IteratorHelper::Take { .. }
                | IteratorHelper::Drop { .. }
                | IteratorHelper::Concat { .. }
                | IteratorHelper::RegExpStringMatchAll { .. } => {}
            }
        }
        if let Some(record) = generator {
            record.extend_roots(work);
        }
    }
    /// A function's edges: its object (with any realm override), and its closure environment.
    // Inlined into each mark visit, as `array_children`.
    #[inline(always)]
    fn function_children(
        owner: Value,
        function: &FunctionRecord,
        properties: &ValueArena,
        work: &mut Vec<Value>,
        object_roots: &mut impl FnMut(GcTraceKind, Value, u32, bool, &mut Vec<Value>),
        trace_kind: GcTraceKind,
        last_proto: &mut PushedProtos,
    ) {
        Self::object_children(
            owner,
            &function.object,
            properties,
            work,
            object_roots,
            trace_kind,
            last_proto,
        );
        // The environment edge is the caller's (`EnvironmentTrace::closure`). The realm edge is
        // the object's extras (`visit_extras_roots`) when overridden, and the main realm is a root.
    }
    pub(super) fn cell_kind(cell: &Cell) -> CellKind {
        match cell {
            Cell::WasmObject { .. }
            | Cell::ArrayBuffer { .. }
            | Cell::TypedArray { .. }
            | Cell::DataView { .. }
            | Cell::ShadowRealm { .. }
            | Cell::Proxy { .. }
            | Cell::PromiseResolvingState { .. }
            | Cell::Promise(_) => CellKind::Object,
            Cell::WeakMap { .. } => CellKind::WeakMap,
            Cell::WeakSet { .. } => CellKind::WeakSet,
            Cell::WeakRef { .. } | Cell::FinalizationRegistry { .. } => CellKind::WeakRef,
            Cell::BindingReference { .. } => CellKind::Environment,
            Cell::BigInt(_) => CellKind::BigInt,
            Cell::WasmElements(_) | Cell::WasmInstance(_) => CellKind::WasmElements,
            Cell::WasmGlobal { .. } => CellKind::WasmGlobal,
            Cell::WasmMemory { .. } => CellKind::WasmMemory,
            Cell::WasmTable { .. } => CellKind::WasmTable,
            Cell::WasmBits64(_) => CellKind::WasmBits64,
            Cell::WasmV128(_) => CellKind::WasmV128,
            Cell::WasmExtern(_) => CellKind::WasmExtern,
            Cell::AccessorPair { .. } => CellKind::Function,
            Cell::WasmTag { .. } => CellKind::WasmTag,
            Cell::WasmException { .. } => CellKind::WasmException,
            Cell::WasmHostFunction { .. } => CellKind::WasmHostFunction,
            Cell::Date { .. } => CellKind::Date,
            Cell::Error(_) => CellKind::Error,
            Cell::RegExp { .. } => CellKind::RegExp,
            Cell::ArrayFromAsyncState(_) => CellKind::ArrayFromAsync,
            Cell::AwaitState(_) => CellKind::AwaitState,
            Cell::TemporalDuration { .. } => CellKind::TemporalDuration,
            Cell::TemporalPlainDate { .. } => CellKind::TemporalPlainDate,
            Cell::TemporalPlainDateTime { .. } => CellKind::TemporalPlainDateTime,
            Cell::TemporalPlainMonthDay { .. } => CellKind::TemporalPlainMonthDay,
            Cell::TemporalPlainYearMonth { .. } => CellKind::TemporalPlainYearMonth,
            Cell::TemporalZonedDateTime { .. } => CellKind::TemporalZonedDateTime,
            Cell::TemporalInstant { .. } => CellKind::TemporalInstant,
        }
    }
    pub(super) fn cell_payload_bytes(cell: &Cell) -> usize {
        let object_extra_bytes = cell
            .object()
            .map_or(0, |object| object.allocated_extra_bytes());
        object_extra_bytes
            + match cell {
                Cell::WasmObject { .. }
                | Cell::ShadowRealm { .. }
                | Cell::ArrayFromAsyncState(_)
                | Cell::AwaitState(_)
                | Cell::Proxy { .. }
                | Cell::Date { .. }
                | Cell::PromiseResolvingState { .. }
                | Cell::BindingReference { .. }
                | Cell::TemporalDuration { .. }
                | Cell::TemporalInstant { .. }
                | Cell::TypedArray { .. }
                | Cell::DataView { .. }
                | Cell::WeakRef { .. }
                | Cell::FinalizationRegistry { .. }
                | Cell::WasmGlobal { .. }
                | Cell::WasmInstance(_)
                | Cell::WasmBits64(_)
                | Cell::WasmV128(_)
                | Cell::WasmExtern(_)
                | Cell::AccessorPair { .. }
                | Cell::WasmTag { .. } => 0,
                Cell::Promise(promise) => size_of::<PromiseCell>() + promise.record.reaction_heap_bytes(),
                Cell::WasmException { payload, .. } => payload.capacity() * size_of::<Value>(),
                Cell::WasmHostFunction { target, signature } => {
                    size_of::<crate::WasmSignature>()
                        + (signature.params.capacity() + signature.results.capacity())
                            * size_of::<crate::WasmType>()
                        + if matches!(target, crate::heap::WasmHostTarget::JsEmscriptenMemcpy(_)) {
                            size_of::<crate::heap::WasmHostMemcpyTarget>()
                        } else {
                            0
                        }
                }
                Cell::WasmElements(elements) => elements.capacity() * size_of::<Value>(),
                Cell::WasmTable { elements, .. } => elements.external_bytes(),
                Cell::TemporalPlainDate { calendar, .. }
                | Cell::TemporalPlainDateTime { calendar, .. }
                | Cell::TemporalPlainMonthDay { calendar, .. }
                | Cell::TemporalPlainYearMonth { calendar, .. } => calendar.capacity(),
                Cell::TemporalZonedDateTime {
                    instant, calendar, ..
                } => instant.time_zone.capacity() + calendar.capacity(),
                Cell::RegExp { meta, .. } => meta.source.capacity(),
                Cell::ArrayBuffer { bytes, .. } => bytes.capacity(),
                Cell::WasmMemory { bytes, .. } => bytes.capacity(),
                Cell::WeakMap { entries, .. } => entries.allocated_bytes(),
                Cell::WeakSet { entries, .. } => entries.allocated_bytes(),
                Cell::BigInt(value) => value.spilled_bytes(),
                Cell::Error(value) => value.capacity(),
            }
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
/// storage length, and moves the owner to `shape`. `want` is the capacity a moved arena run
/// takes (`ValueArena::push`), zero for the next class.
#[inline(always)]
fn append_property(data: &mut Object, properties: &mut ValueArena, slot: usize, value: Value, shape: u32, want: usize) {
    properties.note_shape_value(shape, slot, value);
    let mut vector = data.properties();
    if vector.is_narrow() {
        // An add grows the run, which only a wide run does (`ValueArena::push`).
        vector = properties.widen(vector);
        data.set_properties(vector, properties);
    }
    if !vector.has_inline_property_storage() {
        let before = vector.into_parts();
        let word = properties.stored_word(shape, slot, value);
        properties.push(&mut vector, word, want);
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
    unsafe { crate::staged::concrete_own_read(object, crate::staged::concrete_over_arena(properties), slot) }
}

/// Whether a store that `widened` its slot's owner is left to the caller: a native-preserving
/// store declines it, before it changes anything.
#[inline(always)]
fn declines_widening(widened: bool, layout: PropertyStoreLayout) -> bool {
    widened && layout == PropertyStoreLayout::PreserveNative
}

/// Stores `value` at `slot` of `data`, whose exact shape outside dictionary storage holds it:
/// arena values without extras need no other decoding (as `property_get_flat`), and inline
/// values that cannot take `value` move to the arena only under MayWiden.
/// Preserve declines before changing storage or values.
#[inline(always)]
fn store_exact_slot(data: &mut Object, properties: &mut ValueArena, slot: usize, value: Value, layout: PropertyStoreLayout) -> bool {
    // The caller matched the exact shape, which holds the slot outside dictionary storage.
    // SAFETY: the caller matched the exact shape, which holds the slot.
    let widened = unsafe { properties.note_exact_slot_word(data.shape(), slot, value) };
    if declines_widening(widened, layout) { return false; }
    // The observed slot's word: the raw double of a Number-typed slot (`ValueArena::stored_word`).
    let value = properties.stored_word(data.shape(), slot, value);
    // Code that held the object's values as constants retires before the write
    // (`Object::note_constant_write`); the exact shape a cache records leaves the mark out.
    data.note_constant_write();
    // A plain arena run first, by one payload compare (`Object::plain_arena_start`): the
    // extras-tag and flag tests of `exact_storage` were six instructions per store.
    if let Some(start) = data.plain_arena_start() {
        debug_assert!(matches!(data.exact_storage(), ExactStorage::Plain(plain) if plain == start));
        // SAFETY: as below.
        unsafe { properties.set_flat_at(start, slot, value) };
        return true;
    }
    match data.exact_storage() {
        // SAFETY: as above.
        ExactStorage::Plain(start) => unsafe { properties.set_flat_at(start, slot, value) },
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
            unsafe { store_extended_slot(data, properties, slot, value, inline) };
        }
        // A narrow run takes a narrow value in place; another value widens it first.
        ExactStorage::Narrow => match value.to_narrow() {
            // SAFETY: as above.
            Some(narrow) => unsafe { properties.narrow_set(data.properties(), slot, narrow) },
            None if layout != PropertyStoreLayout::MayWiden => return false,
            None => {
                let wide = properties.widen_for_value(data.properties());
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
unsafe fn store_extended_slot(data: &mut Object, properties: &mut ValueArena, slot: usize, value: Value, inline: bool) {
    if inline {
        data.store_extended_inline(slot, value);
    } else {
        // SAFETY: the caller proves that the property slot exists outside dictionary storage.
        unsafe { properties.set_flat_unchecked(data.properties(), slot, value) };
    }
}

#[cfg(test)]
mod offset_order_tests;

/// The prototypes a mark walk pushed last (`Heap::object_children`): two, as a walk of plain
/// objects alternates between theirs and their arrays' (V8-v7 Splay's leaves: one entry
/// missed about twice per leaf, a third of the pops of a full collection).
#[derive(Clone, Copy)]
pub(crate) struct PushedProtos([Value; 2], ProtoLink, [Value; 2]);

impl Default for PushedProtos {
    fn default() -> Self {
        Self([Value::UNDEFINED; 2], ProtoLink::NULL, [Value::UNDEFINED; 2])
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

    /// Whether the shared edge `edge` (a function's realm, an environment's parent) is a heap
    /// value this walk did not push last as such. Closures of one realm and activations of one
    /// scope name the same few cells: each push of one already pushed costs a work-list pop
    /// that finds it marked (a closure per activation: 2 of its 8 pops, about 70 Ir).
    #[inline(always)]
    pub(crate) fn first_shared(&mut self, edge: Value) -> bool {
        if !edge.is_heap() || edge == self.2[0] || edge == self.2[1] {
            return false;
        }
        self.2 = [edge, self.2[0]];
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

impl Heap {
    /// `QUENCH_TRACE=storage-census`: per container, the live 8-byte values and how many have a
    /// narrow form (`Value::to_narrow`), the bytes that 4-byte storage would save, and the
    /// record counts beside them, after a full collection. One line, in KB where sized.
    #[cold]
    #[inline(never)]
    fn trace_storage_census(&self) {
        #[derive(Default)]
        struct Tally { containers: usize, values: usize, narrow: usize, words: usize }
        impl Tally {
            fn take(&mut self, values: impl Iterator<Item = Value>) {
                self.containers += 1;
                for value in values {
                    self.values += 1;
                    if value.to_narrow().is_some() { self.narrow += 1; }
                }
            }
            fn line(&self, name: &str) -> String {
                format!("{name}={}/{}n/{}w/{}c save={}KB", self.values, self.narrow, self.words, self.containers, self.narrow_savings() / 1024)
            }
            fn narrow_savings(&self) -> usize {
                // A narrow value takes 4 bytes of the 8, where the container is narrow whole.
                self.narrow * 4
            }
        }
        let properties = &self.properties;
        let mut own = Tally::default();
        let mut runs_narrow = 0usize;
        let mut elements = Tally::default();
        let mut element_capacity = 0usize;
        let mut reasons = [(0usize, 0usize); 3];
        let mut take_object = |object: &Object, own: &mut Tally| {
            let vector = object.properties();
            if vector.is_dictionary() { return; }
            let len = properties.len(vector);
            if !vector.has_inline_property_storage() {
                own.words += properties.capacity(vector);
            }
            if vector.is_narrow() || vector.has_inline_property_storage() {
                // Already 4 bytes a value, or in the record's own word: nothing more to save.
                runs_narrow += usize::from(vector.is_narrow());
                own.containers += 1;
                own.values += len;
                return;
            }
            let all_narrow = (0..len).all(|slot| properties.get(vector, slot).is_some_and(|v| v.to_narrow().is_some()));
            let idx = if !all_narrow { 0 } else if properties.narrow_class_smaller(len) { 1 } else { 2 };
            reasons[idx].0 += 1; reasons[idx].1 += len;
            own.take((0..len).filter_map(|slot| properties.get(vector, slot)));
        };
        let (mut objects, mut arrays, mut functions) = (0usize, 0usize, 0usize);
        self.object_records.for_each_live(|_, record| { objects += 1; take_object(&record.object, &mut own); });
        self.array_records.for_each_live(|_, array| {
            arrays += 1;
            take_object(array, &mut own);
            let backing = array.elements();
            element_capacity += backing.capacity();
            elements.words += backing.capacity();
            elements.take(backing.as_slice().iter().copied());
        });
        self.function_records.for_each_live(|_, function| { functions += 1; take_object(&function.object, &mut own); });
        let (mut maps, mut sets, mut environments, mut cells) = (Tally::default(), Tally::default(), Tally::default(), 0usize);
        self.environment_records.for_each_live(|_, environment| {
            environments.containers += 1;
            environments.values += environment.slots.len();
            environments.words += environment.slots.len();
        });
        let mut map_bytes = 0usize;
        cells += self.slots.iter().filter(|slot| slot.cell.is_some()).count();
        self.collection_records.for_each_live(|_, collection| {
            map_bytes += collection.entries.payload_bytes();
            match &collection.entries {
                CollectionStorage::Map(entries) => {
                    maps.words += 2 * entries.capacity();
                    maps.take(entries.iter().flat_map(|(key, value)| [key, value]));
                }
                CollectionStorage::Set(entries) => {
                    sets.words += entries.capacity();
                    sets.take(entries.iter());
                }
            }
        });
        let _ = element_capacity;
        // Strings: records and their unit storage (shared storage counted once).
        let (mut strings, mut string_bytes) = (0usize, 0usize);
        let mut seen = rustc_hash::FxHashSet::default();
        self.string_records.for_each_live(|_, string| {
            strings += 1;
            let ((unit_address, unit_bytes), host) = string.memory_parts();
            if unit_bytes > 0 && seen.insert(unit_address) { string_bytes += unit_bytes; }
            if let Some((address, bytes)) = host { if seen.insert(address) { string_bytes += bytes; } }
        });
        // Cells by kind, with the payload each owns beyond its slot.
        let mut kinds: std::collections::BTreeMap<&'static str, (usize, usize)> = Default::default();
        for slot in self.slots.iter() {
            let Some(cell) = slot.cell.as_ref() else { continue };
            let kind = match cell {
                Cell::Proxy { .. } => "proxy", Cell::WeakMap { .. } => "weakmap",
                Cell::AccessorPair { .. } => "accessor", Cell::BindingReference { .. } => "binding",
                _ => "other",
            };
            let entry = kinds.entry(kind).or_default();
            entry.0 += 1;
            entry.1 += cell.payload_bytes();
        }
        self.collection_records.for_each_live(|_, collection| {
            let kind = match collection.entries {
                CollectionStorage::Map(_) => "map",
                CollectionStorage::Set(_) => "set",
            };
            let entry = kinds.entry(kind).or_default();
            entry.0 += 1;
            entry.1 += collection.entries.payload_bytes();
        });
        let kinds = kinds.iter().map(|(kind, (count, bytes))| format!("{kind}:{count}/{}KB", bytes / 1024)).collect::<Vec<_>>().join(",");
        eprintln!("storage census strings={strings} string_bytes={}KB slot_bytes={} cells[{kinds}]", string_bytes / 1024, std::mem::size_of::<Slot>());
        eprintln!("storage census reasons: wide={:?} eligible={:?} same_class={:?}", reasons[0], reasons[1], reasons[2]);
        let rss_kb = std::fs::read_to_string("/proc/self/statm")
            .ok()
            .and_then(|statm| statm.split_whitespace().nth(1)?.parse::<usize>().ok())
            .map_or(0, |pages| pages * 4);
        let total = own.narrow_savings() + elements.narrow_savings() + maps.narrow_savings() + sets.narrow_savings();
        eprintln!(
            "storage census: rss_kb={rss_kb} records objects={objects} arrays={arrays} functions={functions} cells={cells} arena_words={} {} narrow_runs={runs_narrow} {} {} {} envs={}/{}c map_set_payload={}KB save_total={}KB",
            properties.resident_words(),
            own.line("own"),
            elements.line("elements"),
            maps.line("map"),
            sets.line("set"),
            environments.values,
            environments.containers,
            map_bytes / 1024,
            total / 1024,
        );
    }
}
