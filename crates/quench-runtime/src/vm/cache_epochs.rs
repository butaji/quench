//! The VM's invalidation epochs, one counter per event that retires cached answers.
//!
//! A cache keeps the value of an epoch counter when it stores an answer and is valid only
//! while the counter still reads the same. Each counter here is bumped at exactly one place
//! (or a small, named set of places), so the rule for every cache below is "compare against
//! the event it names". Caches not listed here keep their own rule and are recorded as
//! "not migrated" with the reason.
//!
//! | Cache (file)                                                                 | Event         | Migrated | Invalidated where |
//! |------------------------------------------------------------------------------|---------------|----------|-------------------|
//! | `ConcatMemo`: string-concat atoms, `memoized_concat`/`memoize_concat` (vm/string_cache.rs) | `gc`   | yes | `Vm::collect_inner` (vm/gc.rs), after `Heap::collect_with_object_roots` |
//! | `CaptureWalk` / `CaptureCache`: capture walks and frame capture slots (vm/environment.rs `walked_capture_slots`, `remembered_capture`, `current_capture_slots`, `cached_capture`; vm.rs `CaptureCache`) | `gc` | yes | `Vm::collect_inner` |
//! | Lane generation: `LaneStack::enter_generation` / `retire_after_collection` keys, lane targets and nested-call checks (vm/dispatch_fast.rs, vm/dispatch_fast/lane_roots.rs, `PreparedLaneCallback::epoch`) | `gc` | yes | `Vm::collect_inner` (retire key read there too) |
//! | Generator window plans, `WindowPlan::key` (vm/dispatch_fast/generator_windows.rs) | `gc` (+ `ProgramStore::context_epoch`) | yes (gc part) | `Vm::collect_inner` |
//! | Callee admissions, `CallEntry::epoch` (vm/compiled/frameless/admission.rs, vm/compiled/frameless.rs) | `gc` | yes | `Vm::collect_inner` |
//! | Field-cache table rebind check in `run_nested_lane_method` (vm/compiled.rs) | `field_entries` | yes | `Vm::field_caches_moved` (vm.rs), when the field cache table grows or is replaced |
//! | `ProgramContext::epoch` (vm.rs), `ProgramStore::context_epoch` | programs | no | Not migrated: the counter lives in `ProgramStore` (vm/program_store.rs) and is bumped inside it at its own sites (`insert_reclaimable_shared`, `reclaim_unreferenced`, `reset`, `insert_shared`) as well as by `Vm` callers (vm.rs, vm/snapshot.rs). Moving it means moving that ownership. |
//! | Prototype-layout proofs: `FieldAddCache::proved_epoch` (vm.rs), `AccessorFields` (vm/accessor_cache.rs), `InPresence` (vm/in_presence.rs), `iterator_absence_proof` (vm/iterator_proofs.rs), `concat_spreadable_proof` (vm/array_dense.rs) | prototype layout | no | Not migrated: `heap::prototype_layout_epoch` is a process-wide static (heap/cell.rs) shared by every heap, advanced from heap code (heap.rs, heap/cell.rs, heap/cell/prototype_values.rs) and VM code (vm/gc.rs, `compact_live_shapes`). Mirroring it per VM would miss advances made by other heaps. |
//! | `shape_memo` tables (`DirectMappedTable`, shape slot/attribute/copy/assign memos) | none (explicit clears) | no | Not migrated: no epoch. Entries are dropped when shape ids or atoms are reassigned (shape compaction, VM reset). |
//! | `heap::EnvironmentSlots::link` / `linked_parent` stamps (vm/environment.rs `link_capture_chain`, vm/construction.rs, vm/dispatch_fast.rs) | `Heap::collection_epoch` | no | Not migrated: these stamps are read by the heap's own slot records, which compare against the heap counter. |
//! | `settle_registers` / `suspended_epoch` (vm/generator.rs, vm/activation/frame_record.rs) | `Heap::collection_epoch` | no | Not migrated: stamped into heap-side frame records. |
//! | For-in key lists (vm/object_static/for_in_keys.rs), JSON parse cache (vm/json/*), promise async (vm/promise_async.rs), lane frame heap records (vm/dispatch_fast/lane_frame.rs `heap_records`) | `Heap::collection_epoch` | no | Not migrated in this pass: each stores the heap counter next to heap-side records. |
//! | `rc_pin_epoch` (vm.rs, vm/gc.rs) | `Heap::rc_pin_epoch` | no | Not migrated: a heap-owned pin counter, not a collection event. |
//! | `NativeBases::epoch` (vm/compiled.rs) | `Heap::collection_epoch` | no | Not migrated: written at reread, never compared in this file. |
//!
//! Every `gc` value is `Heap::collection_epoch`'s count at the time of the last `bump_gc`: the
//! heap bumps its own counter at the start of a collection and this one at the end of the VM's
//! collection call (the only production path into `Heap::collect_with_object_roots`), so no
//! VM code runs between the two. The counters are monotonic and never reset, so a stored value
//! equals the current one only when no collection has run since it was stored.

/// The invalidation counters of one VM.
#[derive(Default)]
pub(crate) struct CacheEpochs {
    /// Counts the stop-the-world collections that finished in this VM (`bump_gc`).
    pub gc: u64,
    /// Counts moves of the field cache table: its growth or replacement (`bump_field_entries`).
    pub field_entries: u64,
}

impl CacheEpochs {
    /// A collection finished: every cache keyed on `gc` retires.
    #[inline(always)]
    pub(crate) fn bump_gc(&mut self) {
        self.gc += 1;
    }

    /// The field cache table moved: every pointer into it is stale.
    #[inline(always)]
    pub(crate) fn bump_field_entries(&mut self) {
        self.field_entries += 1;
    }
}
