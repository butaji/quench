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
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

/// Values the marker marks between looks at its stop flag and its deferred handoff.
const MARKER_CHUNK: usize = 4096;
/// Deferred values the marker keeps before publishing them to the mutator mid-chunk.
const MARKER_DEFERRED_PUBLISH: usize = 1024;

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
            HeapReference::ObjectRecord(id) => {
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
            HeapReference::ArrayRecord(id) => {
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
            HeapReference::FunctionRecord(id) => {
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
            HeapReference::StringRecord(id) => {
                if small_strings::small_string_local(id).is_some() {
                    self.leaf_string(id, work, marked);
                } else if self.strings.unmarked(id) && self.strings.mark_if_new(id) {
                    *marked += 1;
                    // SAFETY: as above.
                    unsafe { JsString::concurrent_parts(self.strings.record(id), self.parts_tag, |part| self.flat_part(part, work, marked)) };
                }
            }
            HeapReference::V128Record(id) => {
                if self.v128s.unmarked(id) && self.v128s.mark_if_new(id) {
                    *marked += 1;
                }
            }
            HeapReference::LegacySlot(_)
            | HeapReference::PromiseRecord(_)
            | HeapReference::AwaitRecord(_)
            | HeapReference::IteratorRecord(_) => deferred.push(value),
        }
    }

    /// Marks `id` of `family` for the mutator, which traces it itself; `true` when newly
    /// marked. Fresh ids are black already.
    fn mark_for_mutator<T>(family: &ConcurrentRecordMarks<T>, id: RecordId) -> bool {
        family.unmarked(id) && family.mark_if_new(id)
    }
}

/// What the marker and the mutator exchange, under one lock.
#[derive(Default)]
struct Handoff {
    /// Values for the marker: the barrier's copies, weak reads, and what the mutator's share
    /// reached.
    inbox: Vec<Value>,
    /// Values the marker left to the mutator, unmarked.
    deferred: Vec<Value>,
    /// Whether the marker is waiting with nothing to mark.
    idle: bool,
}

struct MarkerShared {
    view: ConcurrentMarkView,
    handoff: Mutex<Handoff>,
    wake: Condvar,
    stop: AtomicBool,
    /// Whether the mutator assists (`MarkerThread::assist`): the marker then shares half of a
    /// large stack through the inbox.
    assist: AtomicBool,
    /// Marks set by the marker and assists, and the marker's busy time.
    marked: AtomicU64,
    busy_nanos: AtomicU64,
    /// The longest the marker's stack grew, in values.
    peak_work: AtomicU64,
}

/// The marker thread of a running cycle. Dropping it stops and joins the thread.
pub(super) struct MarkerThread {
    shared: Arc<MarkerShared>,
    thread: Option<std::thread::JoinHandle<Vec<Value>>>,
}

impl MarkerThread {
    /// Starts the marker on `work`.
    pub(super) fn start(view: ConcurrentMarkView, work: Vec<Value>) -> Self {
        let shared = Arc::new(MarkerShared {
            view,
            handoff: Mutex::new(Handoff { inbox: work, ..Handoff::default() }),
            wake: Condvar::new(),
            stop: AtomicBool::new(false),
            assist: AtomicBool::new(false),
            marked: AtomicU64::new(0),
            busy_nanos: AtomicU64::new(0),
            peak_work: AtomicU64::new(0),
        });
        let marker = Arc::clone(&shared);
        let thread = std::thread::Builder::new()
            .name("quench-gc-marker".into())
            .spawn(move || marker_loop(&marker))
            .expect("concurrent GC marker thread");
        Self { shared, thread: Some(thread) }
    }

    /// What the marker reads.
    pub(super) fn view(&self) -> &ConcurrentMarkView {
        &self.shared.view
    }

    /// Hands `values` to the marker.
    pub(super) fn send(&self, values: &mut Vec<Value>) {
        if values.is_empty() {
            return;
        }
        let mut handoff = self.shared.handoff.lock().expect("marker handoff poisoned");
        handoff.inbox.append(values);
        handoff.idle = false;
        drop(handoff);
        self.shared.wake.notify_one();
    }

    /// Takes what the marker deferred into `deferred`; whether the marker is idle with nothing
    /// sent and nothing deferred, so its marking is done unless the caller adds more.
    pub(super) fn take_deferred(&self, deferred: &mut Vec<Value>) -> bool {
        let mut handoff = self.shared.handoff.lock().expect("marker handoff poisoned");
        let quiet = handoff.idle && handoff.inbox.is_empty() && handoff.deferred.is_empty();
        deferred.append(&mut handoff.deferred);
        quiet
    }

    /// Marks up to `budget` values of the marker's inbox on the mutator, through the same
    /// view and atomic marks, when allocation outran the cycle: what they reach goes back to
    /// the inbox, and what the view cannot read to `deferred`. From now on the marker shares
    /// half of a large stack through the inbox, so an assist finds work.
    pub(super) fn assist(&self, budget: usize, deferred: &mut Vec<Value>) {
        self.shared.assist.store(true, Ordering::Relaxed);
        let mut work = {
            let mut handoff = self.shared.handoff.lock().expect("marker handoff poisoned");
            let keep = handoff.inbox.len().saturating_sub(budget);
            handoff.inbox.split_off(keep)
        };
        let mut marked = 0;
        for _ in 0..budget {
            let Some(value) = work.pop() else { break };
            self.shared.view.mark_value(value, &mut work, deferred, &mut marked);
        }
        self.shared.marked.fetch_add(marked as u64, Ordering::Relaxed);
        self.send(&mut work);
    }

    /// Marks set and the marker's busy time so far, in nanoseconds.
    pub(super) fn stats(&self) -> (u64, u64) {
        (self.shared.marked.load(Ordering::Relaxed), self.shared.busy_nanos.load(Ordering::Relaxed))
    }

    /// The longest the marker's stack grew, in values.
    pub(super) fn peak_work(&self) -> u64 {
        self.shared.peak_work.load(Ordering::Relaxed)
    }

    /// Stops and joins the marker: every value it held or was sent, and what it deferred.
    pub(super) fn stop(mut self) -> Vec<Value> {
        self.stop_and_join()
    }

    fn stop_and_join(&mut self) -> Vec<Value> {
        let Some(thread) = self.thread.take() else { return Vec::new() };
        self.shared.stop.store(true, Ordering::Release);
        {
            // Under the lock, so a marker about to wait sees the flag first.
            let _handoff = self.shared.handoff.lock().expect("marker handoff poisoned");
            self.shared.wake.notify_one();
        }
        let mut left = thread.join().expect("concurrent GC marker panicked");
        let mut handoff = self.shared.handoff.lock().expect("marker handoff poisoned");
        left.append(&mut handoff.inbox);
        left.append(&mut handoff.deferred);
        left
    }
}

impl Drop for MarkerThread {
    fn drop(&mut self) {
        self.stop_and_join();
    }
}

/// The marker thread: marks what it is sent until stopped, publishing what it defers, and
/// returns the work it held when stopped.
fn marker_loop(shared: &MarkerShared) -> Vec<Value> {
    let mut work = Vec::new();
    let mut deferred = Vec::new();
    let mut marked = 0;
    let mut busy_since = std::time::Instant::now();
    let publish = |marked: &mut usize, busy_since: std::time::Instant| {
        shared.marked.fetch_add(*marked as u64, Ordering::Relaxed);
        shared.busy_nanos.fetch_add(busy_since.elapsed().as_nanos() as u64, Ordering::Relaxed);
        *marked = 0;
    };
    loop {
        if shared.stop.load(Ordering::Acquire) {
            publish(&mut marked, busy_since);
            work.append(&mut deferred);
            return work;
        }
        if work.is_empty() {
            publish(&mut marked, busy_since);
            let mut handoff = shared.handoff.lock().expect("marker handoff poisoned");
            handoff.deferred.append(&mut deferred);
            loop {
                if shared.stop.load(Ordering::Acquire) {
                    return work;
                }
                if !handoff.inbox.is_empty() {
                    // A large inbox (the initial mark's snapshot) is taken a chunk at a time,
                    // so the marker's stack grows from a chunk, not from the whole inbox.
                    let len = handoff.inbox.len();
                    if len > 2 * MARKER_CHUNK {
                        work.extend(handoff.inbox.drain(len - MARKER_CHUNK..));
                    } else {
                        std::mem::swap(&mut work, &mut handoff.inbox);
                    }
                    handoff.idle = false;
                    busy_since = std::time::Instant::now();
                    break;
                }
                handoff.idle = true;
                handoff = shared.wake.wait(handoff).expect("marker handoff poisoned");
            }
        }
        for _ in 0..MARKER_CHUNK {
            let Some(value) = work.pop() else { break };
            shared.view.mark_value(value, &mut work, &mut deferred, &mut marked);
        }
        shared.peak_work.fetch_max(work.capacity() as u64, Ordering::Relaxed);
        let share = shared.assist.load(Ordering::Relaxed) && work.len() > 2 * MARKER_CHUNK;
        if deferred.len() >= MARKER_DEFERRED_PUBLISH || share {
            let mut handoff = shared.handoff.lock().expect("marker handoff poisoned");
            handoff.deferred.append(&mut deferred);
            if share && handoff.inbox.is_empty() {
                handoff.inbox = work.split_off(work.len() / 2);
            }
        }
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
        let mut no_roots = |_: GcTraceKind, _: Value, _: u32, _: bool, _: &mut Vec<Value>| {};
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
                Some(HeapReference::ObjectRecord(id)) => {
                    if ConcurrentMarkView::mark_for_mutator(&view.objects, id) {
                        // SAFETY: the id is occupied and the mutator owns the heap.
                        let object = unsafe { self.object_records.get_live(id) };
                        Self::object_children(value, object, &self.properties, &mut children, &mut no_roots, GcTraceKind::Collection, last_proto);
                    }
                }
                Some(HeapReference::ArrayRecord(id)) => {
                    if ConcurrentMarkView::mark_for_mutator(&view.arrays, id) {
                        // SAFETY: as above.
                        let array = unsafe { self.array_records.get_live(id) };
                        Self::array_children(value, array, &self.properties, |_| false, &mut children, &mut no_roots, GcTraceKind::Collection, last_proto);
                    }
                }
                Some(HeapReference::FunctionRecord(id)) => {
                    if ConcurrentMarkView::mark_for_mutator(&view.functions, id) {
                        // SAFETY: as above.
                        let function = unsafe { self.function_records.get_live(id) };
                        Self::function_children(value, function, &self.properties, &mut children, &mut no_roots, GcTraceKind::Collection, last_proto);
                        children.push(function.env);
                    }
                }
                // The other records are the mutator's alone during a cycle: the serial mark.
                Some(HeapReference::PromiseRecord(_) | HeapReference::AwaitRecord(_) | HeapReference::IteratorRecord(_)) => {
                    self.mark_value(value, &mut children, &mut no_roots, ephemerons, GcTraceKind::Collection, &mut environments, last_proto);
                }
                Some(HeapReference::StringRecord(_) | HeapReference::V128Record(_)) => for_marker.push(value),
                None => {}
            }
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
        debug_assert!(!environments.has_pending());
    }
}
