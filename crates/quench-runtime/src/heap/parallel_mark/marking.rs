//! The per-value step of a parallel mark round: `ParallelMarkView::mark_value` marks one
//! reference if it is new and leaves what it keeps alive on the worker's work list, one
//! function per kind of record.
use super::*;

/// The shape roots a visit defers: traced at once when the round has the VM's trace, else
/// recorded as `(owner, shape, prototype)` for the collector.
#[inline(always)]
fn object_roots_sink<'s>(
    parallel: Option<&'s ParallelObjectRootTrace<'_>>,
    deferred: &'s mut Vec<(Value, u32, bool)>,
) -> impl FnMut(GcTraceKind, Value, u32, bool, &mut Vec<Value>) + 's {
    move |kind, owner, shape, prototype, roots| match parallel {
        Some(trace) => trace.append(kind, owner, shape, prototype, roots),
        None => deferred.push((owner, shape, prototype)),
    }
}

impl<'heap> ParallelMarkView<'heap> {
    /// Visits the array `id` (`value`) where an object's property finds it, when its visit
    /// pushes nothing but its prototype: no named properties or extras, int32 elements only,
    /// a shape whose roots are already traced, no sparse part and no weak map waiting on it.
    /// Answers whether that was its whole visit; any other array goes to the work list.
    #[inline(always)]
    fn mark_leaf_array_if_new(
        &self,
        value: Value,
        id: RecordId,
        roots: Option<&ParallelObjectRootTrace<'_>>,
        trace_kind: GcTraceKind,
        last_proto: &mut PushedProtos,
        work: &mut Vec<Value>,
    ) -> bool {
        if cfg!(feature = "profile-aggregate") || self.sparse_arrays.is_some() {
            return false;
        }
        let Some(roots) = roots else { return false };
        // SAFETY: an object holds live arrays, and no record is written while marking.
        let array = unsafe { self.arrays.get(id) };
        if !self.leaf_array_visit_allowed(value, array, roots, trace_kind) {
            return false;
        }
        if self.arrays.mark_if_new(id) {
            let proto = array.proto();
            if proto.is_heap() && last_proto.first_push(proto) {
                work.push(proto);
            }
        }
        true
    }

    /// Whether the array `value` reaches has no visit beyond its prototype (see
    /// `mark_leaf_array_if_new`) and no weak map waits for it.
    #[inline(always)]
    fn leaf_array_visit_allowed(
        &self,
        value: Value,
        array: &ArrayObject,
        roots: &ParallelObjectRootTrace<'_>,
        trace_kind: GcTraceKind,
    ) -> bool {
        !(array.has_extras()
            || !array.properties().holds_no_values()
            || !array.elements().holds_only_int32()
            || array.was_used_as_prototype()
            || !roots.adds_nothing(trace_kind, array.shape())
            || self.ephemerons.awaits_key(value))
    }

    /// `Heap::mark_value` on a worker: marks `value` if new and pushes what it keeps alive,
    /// leaving to `batch` what the collector must apply (see the module documentation).
    #[inline(always)]
    fn mark_value(
        &self,
        value: Value,
        work: &mut Vec<Value>,
        batch: &mut ParallelMarkBatch,
        parallel_object_roots: Option<&ParallelObjectRootTrace<'_>>,
        trace_kind: GcTraceKind,
        last_proto: &mut PushedProtos,
    ) {
        let Some(reference) = value.heap_reference() else {
            return;
        };
        let marked = self.mark_reference(value, reference, work, batch, parallel_object_roots, trace_kind, last_proto);
        if let Some(kind) = marked {
            batch.count_marked(kind);
        }
    }

    /// Marks the record or slot `reference` names when it is new, answering its kind, or
    /// `None` when it was already marked.
    #[inline(always)]
    fn mark_reference(
        &self,
        value: Value,
        reference: HeapReference,
        work: &mut Vec<Value>,
        batch: &mut ParallelMarkBatch,
        parallel_object_roots: Option<&ParallelObjectRootTrace<'_>>,
        trace_kind: GcTraceKind,
        last_proto: &mut PushedProtos,
    ) -> Option<CellKind> {
        match reference {
            HeapReference::ObjectRecord(id) => self.mark_object(value, id, work, batch, parallel_object_roots, trace_kind, last_proto),
            HeapReference::ArrayRecord(id) => self.mark_array(value, id, work, batch, parallel_object_roots, trace_kind, last_proto),
            HeapReference::FunctionRecord(id) => self.mark_function(value, id, work, batch, parallel_object_roots, trace_kind, last_proto),
            HeapReference::IteratorRecord(id) => self.mark_iterator(value, id, work, batch, parallel_object_roots, trace_kind, last_proto),
            HeapReference::PromiseRecord(id) => self.mark_promise(value, id, work, batch, parallel_object_roots, trace_kind, last_proto),
            HeapReference::AwaitRecord(id) => self.mark_await(id, work),
            // A vector holds no heap edges: marking one is the whole visit; a string's are its parts.
            HeapReference::StringRecord(id) => self.mark_string(id, work),
            HeapReference::V128Record(id) => self.v128s.mark_if_new(id).then_some(CellKind::WasmV128),
            HeapReference::LegacySlot(index) => self.mark_legacy_slot(value, index as usize, work, batch, parallel_object_roots, trace_kind, last_proto),
        }
    }

    #[inline(always)]
    fn mark_object(
        &self,
        value: Value,
        id: RecordId,
        work: &mut Vec<Value>,
        batch: &mut ParallelMarkBatch,
        parallel_object_roots: Option<&ParallelObjectRootTrace<'_>>,
        trace_kind: GcTraceKind,
        last_proto: &mut PushedProtos,
    ) -> Option<CellKind> {
        if !self.objects.mark_if_new(id) {
            return None;
        }
        // SAFETY: this view just marked the occupied record.
        let object = unsafe { self.objects.get(id) };
        let (strings, small_strings) = (self.strings, self.small_strings);
        let mut object_roots = object_roots_sink(parallel_object_roots, &mut batch.object_roots);
        Heap::object_children_marking(
            value,
            object,
            self.properties,
            |child, work, last_proto| match child.heap_reference() {
                Some(HeapReference::StringRecord(string)) => {
                    crate::heap::string_parts::mark_leaf_string_if_new(&strings, &small_strings, string, work)
                }
                Some(HeapReference::ArrayRecord(array)) => {
                    self.mark_leaf_array_if_new(child, array, parallel_object_roots, trace_kind, last_proto, work)
                }
                _ => false,
            },
            work,
            &mut object_roots,
            trace_kind,
            last_proto,
        );
        self.note_marked_key(value, batch);
        Some(CellKind::Object)
    }

    #[inline(always)]
    fn mark_array(
        &self,
        value: Value,
        id: RecordId,
        work: &mut Vec<Value>,
        batch: &mut ParallelMarkBatch,
        parallel_object_roots: Option<&ParallelObjectRootTrace<'_>>,
        trace_kind: GcTraceKind,
        last_proto: &mut PushedProtos,
    ) -> Option<CellKind> {
        if !self.arrays.mark_if_new(id) {
            return None;
        }
        // SAFETY: this view just marked the occupied record.
        let array = unsafe { self.arrays.get(id) };
        let strings = self.strings;
        let mut object_roots = object_roots_sink(parallel_object_roots, &mut batch.object_roots);
        Heap::array_children(
            value,
            array,
            self.properties,
            |string| crate::heap::string_parts::mark_flat_if_new(&strings, string),
            work,
            &mut object_roots,
            trace_kind,
            last_proto,
        );
        self.sparse_elements(value, work);
        self.note_marked_key(value, batch);
        Some(CellKind::Array)
    }

    #[inline(always)]
    fn mark_function(
        &self,
        value: Value,
        id: RecordId,
        work: &mut Vec<Value>,
        batch: &mut ParallelMarkBatch,
        parallel_object_roots: Option<&ParallelObjectRootTrace<'_>>,
        trace_kind: GcTraceKind,
        last_proto: &mut PushedProtos,
    ) -> Option<CellKind> {
        if !self.functions.mark_if_new(id) {
            return None;
        }
        // SAFETY: this view just marked the occupied record.
        let function = unsafe { self.functions.get(id) };
        let mut object_roots = object_roots_sink(parallel_object_roots, &mut batch.object_roots);
        Heap::function_children(value, function, self.properties, work, &mut object_roots, trace_kind, last_proto);
        if self.reads_closures {
            batch.closures.push(id);
        } else {
            work.push(function.env);
        }
        self.note_marked_key(value, batch);
        Some(CellKind::Function)
    }

    #[inline(always)]
    fn mark_iterator(
        &self,
        value: Value,
        id: RecordId,
        work: &mut Vec<Value>,
        batch: &mut ParallelMarkBatch,
        parallel_object_roots: Option<&ParallelObjectRootTrace<'_>>,
        trace_kind: GcTraceKind,
        last_proto: &mut PushedProtos,
    ) -> Option<CellKind> {
        if !self.iterators.mark_if_new(id) {
            return None;
        }
        // SAFETY: this view just marked the occupied record.
        let iterator = unsafe { self.iterators.get(id) };
        let mut object_roots = object_roots_sink(parallel_object_roots, &mut batch.object_roots);
        Heap::object_children(value, &iterator.object, self.properties, work, &mut object_roots, trace_kind, last_proto);
        Heap::iterator_edges(iterator, work);
        self.note_marked_key(value, batch);
        Some(CellKind::Iterator)
    }

    #[inline(always)]
    fn mark_promise(
        &self,
        value: Value,
        id: RecordId,
        work: &mut Vec<Value>,
        batch: &mut ParallelMarkBatch,
        parallel_object_roots: Option<&ParallelObjectRootTrace<'_>>,
        trace_kind: GcTraceKind,
        last_proto: &mut PushedProtos,
    ) -> Option<CellKind> {
        if !self.promises.mark_if_new(id) {
            return None;
        }
        // SAFETY: this view just marked the occupied record.
        let promise = unsafe { self.promises.get(id) };
        let mut object_roots = object_roots_sink(parallel_object_roots, &mut batch.object_roots);
        Heap::object_children(value, &promise.object, self.properties, work, &mut object_roots, trace_kind, last_proto);
        promise.record.append_roots(work);
        self.note_marked_key(value, batch);
        Some(CellKind::Object)
    }

    #[inline(always)]
    fn mark_await(&self, id: RecordId, work: &mut Vec<Value>) -> Option<CellKind> {
        if !self.awaits.mark_if_new(id) {
            return None;
        }
        // SAFETY: this view just marked the occupied record.
        unsafe { self.awaits.get(id) }.extend_roots(work);
        Some(CellKind::AwaitState)
    }

    /// A string's parts, or a small string's, once it is new.
    #[inline(always)]
    fn mark_string(&self, id: RecordId, work: &mut Vec<Value>) -> Option<CellKind> {
        if let Some(local) = crate::heap::small_strings::small_string_local(id) {
            if !self.small_strings.mark_if_new(local) {
                return None;
            }
            crate::heap::small_strings::trace_small_if_new(&self.small_strings, &self.strings, local, work);
        } else {
            if !self.strings.mark_if_new(id) {
                return None;
            }
            crate::heap::string_parts::trace_parts_if_new(&self.strings, id, work);
        }
        Some(CellKind::String)
    }

    /// A legacy slot's cell, unless it is an environment the collector visits whole.
    #[inline(always)]
    fn mark_legacy_slot(
        &self,
        value: Value,
        index: usize,
        work: &mut Vec<Value>,
        batch: &mut ParallelMarkBatch,
        parallel_object_roots: Option<&ParallelObjectRootTrace<'_>>,
        trace_kind: GcTraceKind,
        last_proto: &mut PushedProtos,
    ) -> Option<CellKind> {
        let cell = self.cell(index)?;
        if let Cell::Environment { function, .. } = cell
            && (self.reads_closures || self.projection_function == Some(*function))
        {
            batch.environments.push(value);
            return None;
        }
        if !self.mark_slot_if_new(index) {
            return None;
        }
        let mut object_roots = object_roots_sink(parallel_object_roots, &mut batch.object_roots);
        Heap::children(value, cell, self.properties, work, &mut object_roots, trace_kind, last_proto);
        if matches!(cell, Cell::Array { .. }) {
            self.sparse_elements(value, work);
        }
        if matches!(cell, Cell::WeakMap { .. }) {
            batch.ephemeron_owners.push(value);
        } else {
            self.note_marked_key(value, batch);
        }
        Some(Heap::cell_kind(cell))
    }
}
