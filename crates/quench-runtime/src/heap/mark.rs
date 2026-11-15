//! Marking: the worklist that traces each reachable cell once, and the mark bits it sets.
use super::*;

/// What a visit of one marked value threads through the cells it reaches.
struct MarkVisit<'a, 's, F> {
    work: &'a mut Vec<Value>,
    object_roots: &'a mut F,
    ephemerons: &'a mut weak::EphemeronWork,
    trace_kind: GcTraceKind,
    environments: &'a mut EnvironmentTrace<'s>,
    last_proto: &'a mut PushedProtos,
}

/// Counts a marked cell of `kind` (`profile-aggregate`).
#[cfg(feature = "profile-aggregate")]
#[inline(always)]
fn profile_marked(profile: &mut GcProfile, kind: CellKind) {
    profile.marked += 1;
    profile.marked_kinds[kind as usize] += 1;
}

impl Heap {
    #[inline(never)]
    pub(super) fn mark_work(
        &mut self,
        work: &mut Vec<Value>,
        object_roots: &mut impl FnMut(GcTraceKind, Value, u32, bool, &mut Vec<Value>),
        parallel: Option<parallel_mark::ParallelMark<'_>>,
        ephemerons: &mut weak::EphemeronWork,
        trace_kind: GcTraceKind,
        environments: &mut EnvironmentTrace<'_>,
    ) {
        let Some(parallel) = parallel else {
            let mut last_proto = PushedProtos::default();
            self.drain_work::<false>(work, object_roots, ephemerons, trace_kind, environments, &mut last_proto, 0);
            return;
        };
        self.mark_work_in_rounds(work, object_roots, parallel, ephemerons, trace_kind, environments);
    }

    #[inline(never)]
    pub(super) fn mark_work_in_rounds(
        &mut self,
        work: &mut Vec<Value>,
        object_roots: &mut impl FnMut(GcTraceKind, Value, u32, bool, &mut Vec<Value>),
        parallel: parallel_mark::ParallelMark<'_>,
        ephemerons: &mut weak::EphemeronWork,
        trace_kind: GcTraceKind,
        environments: &mut EnvironmentTrace<'_>,
    ) {
        let mut last_proto = PushedProtos::default();
        let mut left_to_collector = Vec::new();
        while self.drain_work::<true>(
            work,
            object_roots,
            ephemerons,
            trace_kind,
            environments,
            &mut last_proto,
            parallel.round_minimum,
        ) {
            self.mark_parallel_round(
                parallel,
                work,
                &mut left_to_collector,
                object_roots,
                ephemerons,
                trace_kind,
                environments,
            );
            // A worker counted the visit of an environment it left.
            while let Some(environment) = left_to_collector.pop() {
                self.mark_value(environment, work, object_roots, ephemerons, trace_kind, environments, &mut last_proto);
            }
        }
    }

    /// Marks `work` value by value until no work and no environment edge is left (`false`)
    /// or, with `ROUNDS`, until more than `round_minimum` values wait (`true`): a parallel
    /// round then takes them. Without `ROUNDS` the loop tests nothing per value but the
    /// stack, as the serial mark did before rounds existed.
    #[inline(always)]
    #[allow(clippy::too_many_arguments)]
    pub(super) fn drain_work<const ROUNDS: bool>(
        &mut self,
        work: &mut Vec<Value>,
        object_roots: &mut impl FnMut(GcTraceKind, Value, u32, bool, &mut Vec<Value>),
        ephemerons: &mut weak::EphemeronWork,
        trace_kind: GcTraceKind,
        environments: &mut EnvironmentTrace<'_>,
        last_proto: &mut PushedProtos,
        round_minimum: usize,
    ) -> bool {
        loop {
            if ROUNDS && work.len() > round_minimum {
                return true;
            }
            let Some(value) = work.pop() else {
                if !environments.has_pending() {
                    return false;
                }
                self.trace_environment_edges(environments, work);
                continue;
            };
            #[cfg(feature = "profile-aggregate")]
            {
                self.gc_profile.work_items += 1;
            }
            self.mark_value(value, work, object_roots, ephemerons, trace_kind, environments, last_proto);
            #[cfg(feature = "profile-aggregate")]
            {
                self.gc_profile.max_worklist = self.gc_profile.max_worklist.max(work.len() as u64);
            }
        }
    }

    /// Counts, as survivors do, the values a snapshot pushed onto `work` from `first` on.
    fn rc_count_pushed(&mut self, first: usize, work: &[Value]) {
        for &child in &work[first..] {
            self.rc_increment(child);
        }
        self.rc_counted += 1;
    }

    /// Marks `value` when it is a heap value not marked yet, pushing the values it keeps
    /// alive onto `work`: the serial mark's visit of one value.
    #[inline(always)]
    pub(super) fn mark_value<F: FnMut(GcTraceKind, Value, u32, bool, &mut Vec<Value>)>(
        &mut self,
        value: Value,
        work: &mut Vec<Value>,
        object_roots: &mut F,
        ephemerons: &mut weak::EphemeronWork,
        trace_kind: GcTraceKind,
        environments: &mut EnvironmentTrace<'_>,
        last_proto: &mut PushedProtos,
    ) {
        let mut visit = MarkVisit { work, object_roots, ephemerons, trace_kind, environments, last_proto };
        if let Some(record_id) = value.direct_object_index() {
            self.mark_direct_object(value, record_id, &mut visit);
        } else if let Some(record_id) = value.direct_array_index() {
            self.mark_direct_array(value, record_id, &mut visit);
        } else if let Some(record_id) = value.direct_function_index() {
            self.mark_direct_function(value, record_id, &mut visit);
        } else if let Some(record_id) = value.direct_iterator_index() {
            self.mark_direct_iterator(value, record_id, &mut visit);
        } else if let Some(record_id) = value.direct_string_index() {
            self.mark_direct_string(record_id, &mut visit);
        } else if let Some(record_id) = value.direct_v128_index() {
            self.mark_direct_v128(record_id);
        } else if let Some(record_id) = value.direct_await_index() {
            self.mark_direct_await(record_id, &mut visit);
        } else if let Some(record_id) = value.direct_promise_index() {
            self.mark_direct_promise(value, record_id, &mut visit);
        } else {
            self.mark_legacy_cell(value, &mut visit);
        }
    }

    /// A direct object's visit: its children, then the ephemerons it settles.
    #[inline(always)]
    fn mark_direct_object<F: FnMut(GcTraceKind, Value, u32, bool, &mut Vec<Value>)>(&mut self, value: Value, record_id: u32, visit: &mut MarkVisit<'_, '_, F>) {
        if !self.object_records.mark(record_id) {
            return;
        }
        #[cfg(feature = "profile-aggregate")]
        profile_marked(&mut self.gc_profile, CellKind::Object);
        // SAFETY: the direct handle is live and its mark bit was newly set.
        let object = unsafe { self.object_records.get_live(record_id) };
        if self.rc_marking && self.object_records.is_aged(record_id) {
            // A survivor (or a re-marked candidate) counts what its snapshot names
            // (`SatbSnapshot`), pushed here. Its shape's edges are pinned or this
            // collection's roots, and its owned roots are roots (`Vm::collect_inner`).
            let first = visit.work.len();
            Self::object_children(value, object, &self.properties, visit.work, &mut |_, _, _, _, _| {}, visit.trace_kind, &mut PushedProtos::default());
            self.rc_count_pushed(first, visit.work);
        } else {
            Self::object_children(value, object, &self.properties, visit.work, visit.object_roots, visit.trace_kind, visit.last_proto);
        }
        let mark_bits = mark_bits!(self);
        visit.ephemerons.newly_marked(value, None, |target| mark_bits.is_marked(target), visit.work);
    }

    /// A direct array's visit: its elements, its sparse elements, then the ephemerons it settles.
    #[inline(always)]
    fn mark_direct_array<F: FnMut(GcTraceKind, Value, u32, bool, &mut Vec<Value>)>(&mut self, value: Value, record_id: u32, visit: &mut MarkVisit<'_, '_, F>) {
        if !self.array_records.mark(record_id) {
            return;
        }
        #[cfg(feature = "profile-aggregate")]
        profile_marked(&mut self.gc_profile, CellKind::Array);
        // SAFETY: a direct Array Value names an initialized record.
        let array = unsafe { self.array_records.get_live(record_id) };
        if self.rc_marking && self.array_records.is_aged(record_id) {
            // As for an object: the snapshot's values are counted as they are pushed.
            let first = visit.work.len();
            Self::array_children(value, array, &self.properties, |_| false, visit.work, &mut |_, _, _, _, _| {}, visit.trace_kind, &mut PushedProtos::default());
            self.rc_count_pushed(first, visit.work);
        } else {
            let string_records = &mut self.string_records;
            Self::array_children(
                value,
                array,
                &self.properties,
                |string| string_parts::mark_flat(string_records, string),
                visit.work,
                visit.object_roots,
                visit.trace_kind,
                visit.last_proto,
            );
        }
        if let Some(arrays) = self.sparse_arrays.as_ref()
            && let Some(elements) = arrays.get(&value)
        {
            visit.work.extend(elements.values.values().copied());
        }
        let mark_bits = mark_bits!(self);
        visit.ephemerons.newly_marked(value, None, |target| mark_bits.is_marked(target), visit.work);
    }

    /// A direct function's visit: its edges, its closure environment, then the ephemerons it settles.
    #[inline(always)]
    fn mark_direct_function<F: FnMut(GcTraceKind, Value, u32, bool, &mut Vec<Value>)>(&mut self, value: Value, record_id: u32, visit: &mut MarkVisit<'_, '_, F>) {
        if !self.function_records.mark(record_id) {
            return;
        }
        #[cfg(feature = "profile-aggregate")]
        profile_marked(&mut self.gc_profile, CellKind::Function);
        // SAFETY: a direct function Value names an initialized record.
        let function = unsafe { self.function_records.get_live(record_id) };
        Self::function_children(value, function, &self.properties, visit.work, visit.object_roots, visit.trace_kind, visit.last_proto);
        visit.environments.closure(function.env, &function.kind, visit.work);
        let mark_bits = mark_bits!(self);
        visit.ephemerons.newly_marked(value, None, |target| mark_bits.is_marked(target), visit.work);
    }

    /// A direct iterator's visit: its object's children and its own edges, then the ephemerons it settles.
    #[inline(always)]
    fn mark_direct_iterator<F: FnMut(GcTraceKind, Value, u32, bool, &mut Vec<Value>)>(&mut self, value: Value, record_id: u32, visit: &mut MarkVisit<'_, '_, F>) {
        if !self.iterator_records.mark(record_id) {
            return;
        }
        #[cfg(feature = "profile-aggregate")]
        profile_marked(&mut self.gc_profile, CellKind::Iterator);
        // SAFETY: a direct iterator Value names an initialized record.
        let iterator = unsafe { self.iterator_records.get_live(record_id) };
        Self::object_children(value, &iterator.object, &self.properties, visit.work, visit.object_roots, visit.trace_kind, visit.last_proto);
        Self::iterator_edges(iterator, visit.work);
        let mark_bits = mark_bits!(self);
        visit.ephemerons.newly_marked(value, None, |target| mark_bits.is_marked(target), visit.work);
    }

    /// A direct string's visit: a small string's text, or a string's parts, which are its edges.
    #[inline(always)]
    fn mark_direct_string<F: FnMut(GcTraceKind, Value, u32, bool, &mut Vec<Value>)>(&mut self, record_id: u32, visit: &mut MarkVisit<'_, '_, F>) {
        if let Some(local) = small_strings::small_string_local(record_id) {
            if self.small_strings.mark(local) {
                small_strings::trace_small(&self.small_strings, &mut self.string_records, local, visit.work);
                if self.rc_marking && self.small_strings.is_aged(local) {
                    // A survivor counts its parts as it marks them (`Heap::rc_epoch`).
                    // SAFETY: just marked, so occupied.
                    unsafe { self.small_strings.get_live(local) }.for_each_text(|part| self.rc_increment(Value::direct_string(part)));
                    self.rc_counted += 1;
                }
                #[cfg(feature = "profile-aggregate")]
                profile_marked(&mut self.gc_profile, CellKind::String);
            }
            return;
        }
        // A string holds no edges but its parts': marking it and tracing them is the visit.
        if self.string_records.mark(record_id) {
            string_parts::trace_parts(&mut self.string_records, record_id, visit.work);
            if self.rc_marking && self.string_records.is_aged(record_id) {
                // As for a small string; a flat one, marked in place wherever its holder
                // is traced, has no parts to count.
                // SAFETY: just marked, so occupied.
                if let Some(parts) = unsafe { self.string_records.get_live(record_id) }.parts() {
                    parts.for_each_text(|part| self.rc_increment(Value::direct_string(part)));
                }
                self.rc_counted += 1;
            }
            #[cfg(feature = "profile-aggregate")]
            profile_marked(&mut self.gc_profile, CellKind::String);
        }
    }

    /// A vector holds no heap edges: marking it is the whole visit.
    #[inline(always)]
    fn mark_direct_v128(&mut self, record_id: u32) {
        if self.v128_records.mark(record_id) {
            #[cfg(feature = "profile-aggregate")]
            profile_marked(&mut self.gc_profile, CellKind::WasmV128);
        }
    }

    /// A direct await state's visit: the roots it holds.
    #[inline(always)]
    fn mark_direct_await<F: FnMut(GcTraceKind, Value, u32, bool, &mut Vec<Value>)>(&mut self, record_id: u32, visit: &mut MarkVisit<'_, '_, F>) {
        if !self.await_records.mark(record_id) {
            return;
        }
        #[cfg(feature = "profile-aggregate")]
        profile_marked(&mut self.gc_profile, CellKind::AwaitState);
        // SAFETY: a direct await Value names an initialized record.
        unsafe { self.await_records.get_live(record_id) }.extend_roots(visit.work);
    }

    /// A direct promise's visit: its object's children and its record's roots, then the
    /// ephemerons it settles.
    #[inline(always)]
    fn mark_direct_promise<F: FnMut(GcTraceKind, Value, u32, bool, &mut Vec<Value>)>(&mut self, value: Value, record_id: u32, visit: &mut MarkVisit<'_, '_, F>) {
        if !self.promise_records.mark(record_id) {
            return;
        }
        #[cfg(feature = "profile-aggregate")]
        profile_marked(&mut self.gc_profile, CellKind::Object);
        // SAFETY: a direct promise Value names an initialized record.
        let promise = unsafe { self.promise_records.get_live(record_id) };
        Self::object_children(value, &promise.object, &self.properties, visit.work, visit.object_roots, visit.trace_kind, visit.last_proto);
        promise.record.append_roots(visit.work);
        let mark_bits = mark_bits!(self);
        visit.ephemerons.newly_marked(value, None, |target| mark_bits.is_marked(target), visit.work);
    }

    /// A legacy heap cell's visit, unless an environment's whole trace already took it or it is
    /// marked: its children, its sparse elements, then the ephemerons it settles.
    #[inline(always)]
    fn mark_legacy_cell<F: FnMut(GcTraceKind, Value, u32, bool, &mut Vec<Value>)>(&mut self, value: Value, visit: &mut MarkVisit<'_, '_, F>) {
        let Some(index) = value.legacy_heap_index().map(|value| value as usize) else {
            return;
        };
        let Some(slot) = self.slots.get_mut(index) else {
            return;
        };
        if slot.cell.is_none() {
            return;
        }
        // A full collection traces environments by what their readers read.
        if let Some(Cell::Environment { function, .. }) = &slot.cell
            && visit.environments.whole(value, index as u32, *function)
        {
            return;
        }
        if !Self::mark_serial(&self.marks, index) {
            return;
        }
        let cell = slot.cell.as_ref().unwrap();
        #[cfg(feature = "profile-aggregate")]
        profile_marked(&mut self.gc_profile, Self::cell_kind(cell));
        Self::children(value, cell, &self.properties, visit.work, visit.object_roots, visit.trace_kind, visit.last_proto);
        let mark_bits = mark_bits!(self);
        visit.ephemerons.newly_marked(value, Some(cell), |target| mark_bits.is_marked(target), visit.work);
        if let Some(arrays) = self.sparse_arrays.as_ref()
            && matches!(cell, Cell::Array { .. })
            && let Some(elements) = arrays.get(&Value::heap(index as u32))
        {
            visit.work.extend(elements.values.values().copied());
        }
    }

    #[inline(always)]
    pub(super) fn marked(marks: &[AtomicU64], index: usize) -> bool {
        marks[index / 64].load(Ordering::Relaxed) & (1 << (index % 64)) != 0
    }

    #[inline(always)]
    pub(super) fn mark_serial(marks: &[AtomicU64], index: usize) -> bool {
        let mask = 1_u64 << (index % u64::BITS as usize);
        mark_word_if_new(&marks[index / u64::BITS as usize], mask)
    }

    #[cfg(debug_assertions)]
    pub(super) fn marked_words(marks: &[u64], index: usize) -> bool {
        marks[index / 64] & (1 << (index % 64)) != 0
    }

    #[cfg(debug_assertions)]
    pub(super) fn snapshot_marks(marks: &[AtomicU64]) -> Vec<u64> {
        marks
            .iter()
            .map(|word| word.load(Ordering::Relaxed))
            .collect()
    }

    pub(super) fn clear_marks(marks: &[AtomicU64]) {
        for word in marks {
            word.store(0, Ordering::Relaxed);
        }
    }

    #[cfg(debug_assertions)]
    pub(super) fn restore_marks(marks: &[AtomicU64], words: &[u64]) {
        debug_assert_eq!(marks.len(), words.len());
        for (mark, value) in marks.iter().zip(words) {
            mark.store(*value, Ordering::Relaxed);
        }
    }
}
