//! A concurrent cycle's marker thread: what it reads of the heap while the mutator runs
//! (`ConcurrentMarkView`), its handoff with the mutator, and the mutator's share of the
//! marking (`Heap::trace_deferred`).
//!
//! The marker visits only the record families whose records it can read racily: plain
//! objects, arrays, functions, strings and vectors, each from a copy of its words
//! (`Object::load_racy`), whose property run is bounded by the arena and shape lengths the
//! cycle snapshotted. A torn or stale copy only adds edges: the snapshot barrier preserved
//! every child the record held at the snapshot before its first write. Array backings stay
//! readable through the cycle's quarantine. Everything else (legacy cells, promise, await and
//! iterator records, objects with extras or dictionary storage, runs past the snapshot) goes
//! back to the mutator unmarked, which marks and traces it at its next poll and hands the
//! records it reaches back to the marker.
//!
//! Mark bits are shared through atomics: the marker's, the mutator's for deferred records,
//! and black allocation's (`RecordSpace::satb_blacken_free_word`). Legacy mark bits are the
//! mutator's alone.

use super::records::ConcurrentRecordMarks;
use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

#[path = "concurrent_marker.rs"]
mod marker;
pub(super) use marker::MarkerThread;


/// The heap as the marker thread reads it (see the module documentation).
pub(super) struct ConcurrentMarkView {
    objects: ConcurrentRecordMarks<Object>,
    arrays: ConcurrentRecordMarks<ArrayObject>,
    functions: ConcurrentRecordMarks<FunctionRecord>,
    strings: ConcurrentRecordMarks<JsString>,
    small: ConcurrentRecordMarks<small_strings::SmallParts>,
    v128s: ConcurrentRecordMarks<[u8; crate::wasm::V128_BYTES]>,
    properties: *const Value,
    properties_len: usize,
    shape_lengths: Box<[u32]>,
    parts_tag: u8,
}

// SAFETY: the records, blocks and property values the view names sit in fixed reservations
// (the cycle's gates) that stay allocated until the marker is joined, which every end of a
// cycle does first; the view only reads them and sets mark bits atomically.
unsafe impl Send for ConcurrentMarkView {}
// SAFETY: as above.
unsafe impl Sync for ConcurrentMarkView {}

impl ConcurrentMarkView {
    /// The view of `heap` at its cycle's start, the snapshot barrier on.
    pub(super) fn new(heap: &mut Heap) -> Self {
        let (properties, properties_len, shape_lengths) = heap.properties.concurrent_snapshot();
        Self {
            objects: heap.object_records.concurrent_marks(),
            arrays: heap.array_records.concurrent_marks(),
            functions: heap.function_records.concurrent_marks(),
            strings: heap.string_records.concurrent_marks(),
            small: heap.small_strings.concurrent_marks(),
            v128s: heap.v128_records.concurrent_marks(),
            properties,
            properties_len,
            shape_lengths,
            parts_tag: JsString::parts_tag(),
        }
    }

    /// Takes `child`, a value a marked record holds: a string is marked where it is found,
    /// as the stop-the-world mark marks leaf strings (`string_parts::mark_leaf_string_if_new`),
    /// with its flat parts; anything else, and a string of parts of parts, goes to `work`.
    #[inline(always)]
    fn child(&self, child: Value, work: &mut Vec<Value>, marked: &mut usize) {
        match child.direct_string_index() {
            Some(id) => self.leaf_string(id, work, marked),
            None => work.push(child),
        }
    }

    /// Marks the string `id` and its parts (`child`).
    #[inline(always)]
    fn leaf_string(&self, id: u32, work: &mut Vec<Value>, marked: &mut usize) {
        if let Some(local) = small_strings::small_string_local(id) {
            if self.small.unmarked(local) && self.small.mark_if_new(local) {
                *marked += 1;
                // SAFETY: an unmarked id below the frontier names an initialized record.
                unsafe { small_strings::concurrent_small_parts(self.small.record(local), |part| self.flat_part(part, work, marked)) };
            }
        } else {
            self.flat_part(id, work, marked);
        }
    }

    /// Marks the string record `id` when it holds no parts; else leaves it to `work`.
    #[inline(always)]
    fn flat_part(&self, id: u32, work: &mut Vec<Value>, marked: &mut usize) {
        if small_strings::small_string_local(id).is_some() {
            return work.push(Value::direct_string(id));
        }
        if !self.strings.unmarked(id) {
            return;
        }
        // SAFETY: as above.
        if unsafe { JsString::concurrent_has_parts(self.strings.record(id), self.parts_tag) } {
            work.push(Value::direct_string(id));
        } else if self.strings.mark_if_new(id) {
            *marked += 1;
        }
    }

    /// Takes the values of the arena run `(start, len)` (`child`).
    #[inline(always)]
    fn run_values(&self, (start, len): (usize, usize), work: &mut Vec<Value>, marked: &mut usize) {
        for index in start..start + len {
            // SAFETY: the run lies inside the snapshot's arena length (`Object::concurrent_run`).
            let bits = unsafe { AtomicU64::from_ptr(self.properties.add(index).cast::<u64>().cast_mut()) }.load(Ordering::Relaxed);
            let value = Value::from_bits(bits);
            if value.is_heap() && !value.is_deleted() {
                self.child(value, work, marked);
            }
        }
    }

    /// Marks `value` if this view may and it is new, taking what it keeps alive; a value the
    /// marker cannot read goes to `deferred`, unmarked. Counts the marks it sets in `marked`.
    pub(super) fn mark_value(&self, value: Value, work: &mut Vec<Value>, deferred: &mut Vec<Value>, marked: &mut usize) {
        let Some(reference) = value.heap_reference() else { return };
        match reference {
            HeapReference::ObjectRecord(id) => self.mark_object(value, id, work, deferred, marked),
            HeapReference::ArrayRecord(id) => self.mark_array(value, id, work, deferred, marked),
            HeapReference::FunctionRecord(id) => self.mark_function(value, id, work, deferred, marked),
            HeapReference::StringRecord(id) => self.mark_string(id, work, marked),
            HeapReference::V128Record(id) => self.mark_v128(id, marked),
            HeapReference::LegacySlot(_)
            | HeapReference::PromiseRecord(_)
            | HeapReference::AwaitRecord(_)
            | HeapReference::IteratorRecord(_) => deferred.push(value),
        }
    }

    /// Marks string record `id`: a small string is a leaf, and a heap string marks its parts.
    fn mark_string(&self, id: u32, work: &mut Vec<Value>, marked: &mut usize) {
        if small_strings::small_string_local(id).is_some() {
            self.leaf_string(id, work, marked);
        } else if self.strings.unmarked(id) && self.strings.mark_if_new(id) {
            *marked += 1;
            // SAFETY: as above.
            unsafe { JsString::concurrent_parts(self.strings.record(id), self.parts_tag, |part| self.flat_part(part, work, marked)) };
        }
    }

    /// Marks vector record `id`, which has no children.
    fn mark_v128(&self, id: u32, marked: &mut usize) {
        if self.v128s.unmarked(id) && self.v128s.mark_if_new(id) {
            *marked += 1;
        }
    }

    /// Marks object record `id`, whose value is `value`, and its inline children; a record
    /// whose shape the cycle cannot read yet is deferred.
    fn mark_object(&self, value: Value, id: u32, work: &mut Vec<Value>, deferred: &mut Vec<Value>, marked: &mut usize) {
        if !self.objects.unmarked(id) {
            return;
        }
        // SAFETY: an unmarked id below the frontier names an initialized record.
        let object = unsafe { Object::load_racy(self.objects.record(id)) };
        let Some(run) = object.concurrent_run(&self.shape_lengths, self.properties_len) else {
            return deferred.push(value);
        };
        if self.objects.mark_if_new(id) {
            *marked += 1;
            object.concurrent_inline_children(|child| self.child(child, work, marked));
            self.run_values(run, work, marked);
        }
    }

    /// Marks array record `id`, whose value is `value`: its object part, its inline children
    /// and its element backing; a record whose shape the cycle cannot read yet is deferred.
    fn mark_array(&self, value: Value, id: u32, work: &mut Vec<Value>, deferred: &mut Vec<Value>, marked: &mut usize) {
        if !self.arrays.unmarked(id) {
            return;
        }
        // SAFETY: as above; an array record is its object part (`repr(C)`).
        let object = unsafe { Object::load_racy(self.arrays.record(id).cast::<Object>()) };
        let Some(run) = object.concurrent_run(&self.shape_lengths, self.properties_len) else {
            return deferred.push(value);
        };
        if self.arrays.mark_if_new(id) {
            *marked += 1;
            object.concurrent_inline_children(|child| self.child(child, work, marked));
            self.run_values(run, work, marked);
            // SAFETY: without extras the payload word is a backing's header pointer,
            // readable through the cycle's quarantine.
            unsafe { array_elements::concurrent_heap_values(object.concurrent_elements_word(), |child| self.child(child, work, marked)) };
        }
    }

    /// Marks function record `id`, whose value is `value`: its object part, its inline
    /// children, its realm and its environment; a record whose shape the cycle cannot read
    /// yet is deferred.
    fn mark_function(&self, value: Value, id: u32, work: &mut Vec<Value>, deferred: &mut Vec<Value>, marked: &mut usize) {
        if !self.functions.unmarked(id) {
            return;
        }
        let record = self.functions.record(id);
        // SAFETY: as above.
        let (object, realm, env) = unsafe {
            let load = |field: *const Value| Value::from_bits(AtomicU64::from_ptr(field.cast::<u64>().cast_mut()).load(Ordering::Relaxed));
            (
                Object::load_racy(std::ptr::addr_of!((*record).object)),
                load(std::ptr::addr_of!((*record).realm)),
                load(std::ptr::addr_of!((*record).env)),
            )
        };
        let Some(run) = object.concurrent_run(&self.shape_lengths, self.properties_len) else {
            return deferred.push(value);
        };
        if self.functions.mark_if_new(id) {
            *marked += 1;
            object.concurrent_inline_children(|child| self.child(child, work, marked));
            self.run_values(run, work, marked);
            work.extend([realm, env].into_iter().filter(|value| value.is_heap()));
        }
    }

    /// Marks `id` of `family` for the mutator, which traces it itself; `true` when newly
    /// marked. Fresh ids are black already.
    fn mark_for_mutator<T>(family: &ConcurrentRecordMarks<T>, id: RecordId) -> bool {
        family.unmarked(id) && family.mark_if_new(id)
    }
}

impl Heap {
    /// The mutator's share of a cycle's marking while its marker runs: marks and traces
    /// `deferred` (see the module documentation), legacy cells to their end, and adds the
    /// records they reach to `for_marker`.
    pub(super) fn trace_deferred(
        &mut self,
        view: &ConcurrentMarkView,
        deferred: &mut Vec<Value>,
        for_marker: &mut Vec<Value>,
        ephemerons: &mut weak::EphemeronWork,
        last_proto: &mut PushedProtos,
    ) {
        let mut environments = EnvironmentTrace::new(None, EnvironmentReads::Projections);
        let mut children = Vec::new();
        while let Some(value) = deferred.pop() {
            match value.heap_reference() {
                // The cycle's start pushed every live cell's children, and a cell made since is
                // black: a cell needs only its mark.
                Some(HeapReference::LegacySlot(index)) => {
                    let index = index as usize;
                    if self.slots.get_mut(index).is_some_and(|slot| slot.cell.is_some()) {
                        Self::mark_serial(&self.marks, index);
                    }
                }
                Some(HeapReference::ObjectRecord(_) | HeapReference::ArrayRecord(_) | HeapReference::FunctionRecord(_)) => {
                    self.trace_record_children(view, value, &mut children, last_proto);
                }
                // The other records are the mutator's alone during a cycle: the serial mark.
                Some(HeapReference::PromiseRecord(_) | HeapReference::AwaitRecord(_) | HeapReference::IteratorRecord(_)) => {
                    self.mark_value(value, &mut children, &mut no_roots, ephemerons, GcTraceKind::Collection, &mut environments, last_proto);
                }
                Some(HeapReference::StringRecord(_) | HeapReference::V128Record(_)) => for_marker.push(value),
                None => {}
            }
            route_children(&mut children, deferred, for_marker);
        }
        debug_assert!(!environments.has_pending());
    }

    /// Traces the children of an object, array or function record the mutator marks for the
    /// cycle (`mark_for_mutator`), leaving the children to the caller's routing.
    fn trace_record_children(&self, view: &ConcurrentMarkView, value: Value, children: &mut Vec<Value>, last_proto: &mut PushedProtos) {
        match value.heap_reference() {
            Some(HeapReference::ObjectRecord(id)) => {
                if ConcurrentMarkView::mark_for_mutator(&view.objects, id) {
                    // SAFETY: the id is occupied and the mutator owns the heap.
                    let object = unsafe { self.object_records.get_live(id) };
                    Self::object_children(value, object, &self.properties, children, &mut no_roots, GcTraceKind::Collection, last_proto);
                }
            }
            Some(HeapReference::ArrayRecord(id)) => {
                if ConcurrentMarkView::mark_for_mutator(&view.arrays, id) {
                    // SAFETY: as above.
                    let array = unsafe { self.array_records.get_live(id) };
                    Self::array_children(value, array, &self.properties, |_| false, children, &mut no_roots, GcTraceKind::Collection, last_proto);
                }
            }
            Some(HeapReference::FunctionRecord(id)) => {
                if ConcurrentMarkView::mark_for_mutator(&view.functions, id) {
                    // SAFETY: as above.
                    let function = unsafe { self.function_records.get_live(id) };
                    Self::function_children(value, function, &self.properties, children, &mut no_roots, GcTraceKind::Collection, last_proto);
                    children.push(function.env);
                }
            }
            _ => {}
        }
    }
}

/// The roots a mutator-side trace adds: none, since a cycle's roots are the marker's.
fn no_roots(_: GcTraceKind, _: Value, _: u32, _: bool, _: &mut Vec<Value>) {}

/// Routes the children a deferred record traced: the cells and records the mutator still
/// owns go back to `deferred`, the rest to the marker.
fn route_children(children: &mut Vec<Value>, deferred: &mut Vec<Value>, for_marker: &mut Vec<Value>) {
    for child in children.drain(..) {
        match child.heap_reference() {
            Some(
                HeapReference::LegacySlot(_)
                | HeapReference::PromiseRecord(_)
                | HeapReference::AwaitRecord(_)
                | HeapReference::IteratorRecord(_),
            ) => deferred.push(child),
            Some(_) => for_marker.push(child),
            None => {}
        }
    }
}
