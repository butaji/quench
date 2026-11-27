//! Reference counting under `QUENCH_DEV=rc-gc`, the foundation of a hybrid collector in the
//! LXR style (Zhao, Blackburn, McKinley, PLDI 2022); see
//! research/nojit/splay/rc-design.txt.
//!
//! An old record's count is the number of references to it from old records' contents (the
//! values their `SatbSnapshot` copies, and a string's parts): two bits per record up to
//! `RC_STUCK`, the references past it in an overflow table.
//! Young records are not counted until they survive a minor collection, which makes every
//! survivor old (`PromotionAge::First`). The barrier copies an old record's children before
//! its first write in an epoch (`RecordSpace::rc_note`); each minor collection, the epoch's
//! end, increments the children of the records it logged and of the young survivors, and
//! decrements the copied ones. A full collection recounts from scratch.
use super::*;

/// The granule of `value` when it names a live class cell, which counts in its own region
/// (`ClassSpace::rc_increment` and its siblings). A departed object is a record, as `locate` says.
#[inline(always)]
fn live_cell(heap: &Heap, value: Value) -> Option<u32> {
    let granule = value.direct_object_index()?.granule();
    heap.class_space.live_class(granule).map(|_| granule)
}

/// The record space of a counted value and its id there (`RecordSpaceId`, the family numbering
/// `Heap::record_families` shares). A live class cell is no record here: `live_cell` names it.
#[inline(always)]
fn locate(heap: &Heap, value: Value) -> Option<(RecordSpaceId, RecordId)> {
    if !value.is_heap() || live_cell(heap, value).is_some() {
        None
    } else if let Some(object) = value.direct_object_index() {
        Some((RecordSpaceId::Object, heap.record_id(object)))
    } else if let Some(id) = value.direct_array_index() {
        Some((RecordSpaceId::Array, id))
    } else if let Some(id) = value.direct_function_index() {
        Some((RecordSpaceId::Function, id))
    } else if let Some(id) = value.direct_promise_index() {
        Some((RecordSpaceId::Promise, id))
    } else if let Some(id) = value.direct_await_index() {
        Some((RecordSpaceId::Await, id))
    } else if let Some(id) = value.direct_iterator_index() {
        Some((RecordSpaceId::Iterator, id))
    } else if let Some(id) = value.direct_collection_index() {
        Some((RecordSpaceId::Collection, id))
    } else if let Some(id) = value.direct_string_index() {
        Some(match small_strings::small_string_local(id) {
            Some(local) => (RecordSpaceId::SmallString, local),
            None => (RecordSpaceId::String, id),
        })
    } else {
        value.direct_v128_index().map(|id| (RecordSpaceId::V128, id))
    }
}

/// `with_space!`'s arms, one per record family (`for_each_record_space!`): `$body` runs with
/// `$space` bound to the family's space, shared.
macro_rules! with_space_arms {
    ($heap:tt, $family:tt, $space:tt, $body:tt ; $($family_id:ident = $field:ident : $space_ty:ty [$elem:ty] $view:ident,)*) => {
        match $family {
            $(RecordSpaceId::$family_id => { let $space = &$heap.$field; $body })*
        }
    };
}

/// `with_space_mut!`'s arms: as `with_space_arms`, with the space mutable.
macro_rules! with_space_mut_arms {
    ($heap:tt, $family:tt, $space:tt, $body:tt ; $($family_id:ident = $field:ident : $space_ty:ty [$elem:ty] $view:ident,)*) => {
        match $family {
            $(RecordSpaceId::$family_id => { let $space = &mut $heap.$field; $body })*
        }
    };
}

/// Runs `$body` with `$space` bound to the record space of family `$family` (`locate`).
macro_rules! with_space {
    ($heap:expr, $family:expr, |$space:ident| $body:expr) => {
        for_each_record_space!(with_space_arms, $heap, $family, $space, $body)
    };
}

/// `with_space!`, mutably.
macro_rules! with_space_mut {
    ($heap:expr, $family:expr, |$space:ident| $body:expr) => {
        for_each_record_space!(with_space_mut_arms, $heap, $family, $space, $body)
    };
}

/// Records a minor collection makes candidates for freeing, at most: the rest of a cascade
/// carries to the next one.
const RC_FREE_BUDGET: usize = 1 << 20;

/// Records the cascade prefetches together (`Heap::rc_pre_mark`).
const RC_CASCADE_BATCH: usize = 16;

/// Live records past this multiple of the last full collection's survivors (at least
/// `RC_FULL_FLOOR`) make the next collection the backup trace, for cycles and stuck counts.
const RC_FULL_GROWTH: usize = 4;
const RC_FULL_FLOOR: usize = 16 * 1024;

/// Runs `$body` once for each record space, mutably, as `$space`; a record's `Value` is
/// `$space.value_of(id)`.
macro_rules! for_each_space_mut {
    ($heap:expr, |$space:ident| $body:expr) => {{
        { let $space = &mut $heap.object_records; $body }
        { let $space = &mut $heap.array_records; $body }
        { let $space = &mut $heap.function_records; $body }
        { let $space = &mut $heap.promise_records; $body }
        { let $space = &mut $heap.await_records; $body }
        { let $space = &mut $heap.iterator_records; $body }
        { let $space = &mut $heap.collection_records; $body }
        { let $space = &mut $heap.string_records; $body }
        { let $space = &mut $heap.small_strings; $body }
        { let $space = &mut $heap.v128_records; $body }
        { let $space = &mut $heap.wasm_gc_records; $body }
        { let $space = &mut $heap.class_space.regions.1; $body }
        { let $space = &mut $heap.class_space.regions.2; $body }
        { let $space = &mut $heap.class_space.regions.3; $body }
        { let $space = &mut $heap.class_space.regions.4; $body }
        { let $space = &mut $heap.class_space.regions.5; $body }
    }};
}

/// Whether any heap of the process has kept reference counts (`Heap::set_rc`): what a stage that
/// holds no heap reads for an immediate store (`staged::Stage::immediate_store_open`). Sticky, so a
/// heap that stops counting leaves it conservatively set.
pub static COUNTS_SEEN: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

impl Heap {
    /// Whether old records keep reference counts.
    pub(crate) fn rc_active(&self) -> bool {
        self.rc_active
    }

    /// Turns counting on, from a recount of the live records, or off.
    pub(crate) fn set_rc(&mut self, on: bool) {
        self.rc_active = on;
        if on {
            COUNTS_SEEN.store(true, core::sync::atomic::Ordering::Relaxed);
        }
        self.refresh_store_watch();
        for_each_space_mut!(self, |space| space.set_rc(on));
        if on {
            self.rc_pin_epoch += 1;
            self.rc_recount();
        }
    }

    /// Bumped when counts restart, which drops every pin.
    pub(crate) fn rc_pin_epoch(&self) -> u64 {
        self.rc_pin_epoch
    }

    /// Pins the records among `values`, edges the VM keeps outside the heap and writes once:
    /// their counts stick, so no epoch makes them candidates, and they are no roots of later
    /// minor collections. A full collection frees them once nothing names them.
    pub(crate) fn rc_pin(&mut self, values: &[Value]) {
        if values.is_empty() {
            return;
        }
        for_each_space_mut!(self, |space| space.rc_ensure());
        for &value in values {
            if let Some(granule) = live_cell(self, value) {
                self.class_space.rc_pin(granule);
            } else if let Some((family, id)) = locate(self, value) {
                with_space!(self, family, |space| space.rc_pin(id));
            }
        }
    }

    /// Appends the values live record `value` keeps alive that counts may name: what its
    /// snapshot copies (`SatbSnapshot`), or a string's parts. Values that are no records
    /// (legacy cells, numbers) are among them; `locate` skips them.
    fn rc_children(&self, value: Value, out: &mut Vec<Value>) {
        // The record families share one dispatch (`Heap::record_children`); a string's parts
        // are the only other children counts name.
        if self.record_children(value, out) {
            return;
        }
        // SAFETY: the caller names live records.
        unsafe {
            if let Some(id) = value.direct_string_index() {
                match small_strings::small_string_local(id) {
                    Some(local) => self.small_strings.get_live(local).for_each_text(|part| out.push(Value::direct_string(part))),
                    None => {
                        if let Some(parts) = self.string_records.get_live(id).parts() {
                            parts.for_each_text(|part| out.push(Value::direct_string(part)));
                        }
                    }
                }
            }
        }
    }

    /// Increments the count of `value` if it is a record.
    #[inline(always)]
    pub(super) fn rc_increment(&self, value: Value) {
        if let Some(granule) = live_cell(self, value) {
            self.class_space.rc_increment(granule);
        } else if let Some((family, id)) = locate(self, value) {
            with_space!(self, family, |space| space.rc_increment(id));
        }
    }

    /// Decrements the count of `value` if it is a record; whether it reached 0.
    fn rc_decrement(&self, value: Value) -> bool {
        if let Some(granule) = live_cell(self, value) {
            return self.class_space.rc_decrement(granule);
        }
        match locate(self, value) {
            Some((family, id)) => with_space!(self, family, |space| space.rc_decrement(id)),
            None => false,
        }
    }

    /// The count of record `value`, if it is one.
    fn rc_count_of(&self, value: Value) -> Option<u8> {
        if let Some(granule) = live_cell(self, value) {
            return Some(self.class_space.rc_count(granule));
        }
        locate(self, value).map(|(family, id)| with_space!(self, family, |space| space.rc_count(id)))
    }

    /// Whether record `value` is old: marked between collections.
    fn rc_is_old(&self, value: Value) -> bool {
        if let Some(granule) = live_cell(self, value) {
            return self.class_space.is_marked(granule);
        }
        locate(self, value).is_some_and(|(family, id)| with_space!(self, family, |space| space.is_marked(id)))
    }

    /// Increments the counts of the record children of each of `owners`.
    fn rc_increment_children(&self, owners: impl IntoIterator<Item = Value>, children: &mut Vec<Value>) {
        for owner in owners {
            self.rc_children(owner, children);
            for child in children.drain(..) {
                self.rc_increment(child);
            }
        }
    }

    /// The end of an epoch, in a minor collection before its mark (`PromotionAge::First`):
    /// the records the barrier logged count their current children and the children it
    /// copied before their first writes are decremented; then each old record whose count is
    /// 0 becomes a candidate (`RecordSpace::rc_unmark`) and decrements its children, in a
    /// cascade of at most `RC_FREE_BUDGET`. The mark that follows re-marks the candidates its
    /// roots (the VM's, with every shape edge), the young records and the logged ones name;
    /// the candidates old legacy cells name, which a minor collection does not trace, are
    /// added to `work` here. The sweep frees the rest.
    pub(super) fn rc_pre_mark(&mut self, work: &mut Vec<Value>) {
        let mut decrements: Vec<Value> = Vec::new();
        let mut logged: Vec<Value> = Vec::new();
        for_each_space_mut!(self, |space| {
            let (copied, written) = space.take_rc_epoch();
            decrements.extend(copied);
            logged.extend(written.into_iter().map(|id| space.value_of(id)));
        });
        let started = std::time::Instant::now();
        let mut children = Vec::new();
        self.rc_increment_children(logged, &mut children);
        self.rc_trace[6] = started.elapsed().as_micros() as u64;
        let mut zero = std::mem::take(&mut self.rc_zero);
        for value in decrements {
            if self.rc_decrement(value) {
                zero.push(value);
            }
        }
        let mut carried = Vec::new();
        let mut candidates = Vec::new();
        // The cascade reads dead records, long out of cache: it takes them from the stack
        // `RC_CASCADE_BATCH` at a time, all prefetched before the first is read.
        let mut batch: Vec<Value> = Vec::with_capacity(RC_CASCADE_BATCH);
        'cascade: loop {
            let take = zero.len().saturating_sub(RC_CASCADE_BATCH);
            batch.extend(zero.drain(take..).rev());
            if batch.is_empty() {
                break;
            }
            for &value in &batch {
                if let Some((family, id)) = locate(self, value) {
                    with_space!(self, family, |space| space.prefetch(id));
                }
            }
            for index in 0..batch.len() {
                let value = batch[index];
                if candidates.len() >= RC_FREE_BUDGET {
                    carried.extend_from_slice(&batch[index..]);
                    carried.append(&mut zero);
                    break 'cascade;
                }
                let Some((family, id)) = locate(self, value) else { continue };
                if !with_space!(self, family, |space| space.is_live(id) && space.rc_count(id) == 0) {
                    continue;
                }
                if !with_space_mut!(self, family, |space| space.rc_unmark(id)) {
                    // Logged this epoch, young, or a candidate already: tried again next epoch.
                    carried.push(value);
                    continue;
                }
                candidates.push(value);
                self.rc_children(value, &mut children);
                for child in children.drain(..) {
                    if self.rc_decrement(child) {
                        zero.push(child);
                    }
                }
            }
            batch.clear();
        }
        if carried.len() > 1024 {
            carried.sort_unstable_by_key(|value| value.to_bits());
            carried.dedup();
        }
        self.rc_trace[2] = candidates.len() as u64;
        self.rc_trace[3] = carried.len() as u64;
        self.rc_trace[7] = started.elapsed().as_micros() as u64 - self.rc_trace[6];
        self.rc_zero = carried;
        let started = std::time::Instant::now();
        if !candidates.is_empty() {
            self.rc_pin_cell_references(work);
        }
        self.rc_trace[8] = started.elapsed().as_micros() as u64;
        self.rc_candidates = candidates;
    }

    /// Adds to `work` each candidate a live legacy cell names, weakly or not, or a
    /// sparse array holds: their references are not counted, and a minor collection traces
    /// old cells only where the cell barrier logged them.
    ///
    /// An unmarked record a cell names is a candidate: a newborn could be named only by a
    /// young cell or one the barrier logged, which the mark traces anyway. So each record
    /// child left unmarked is pushed, with no set of the candidates to test.
    fn rc_pin_cell_references(&self, work: &mut Vec<Value>) {
        let unmarked = |value: &Value| (live_cell(self, *value).is_some() || locate(self, *value).is_some()) && !self.rc_is_old(*value);
        let mut held = Vec::new();
        let mut last_proto = PushedProtos::default();
        for (index, slot) in self.slots.iter().enumerate() {
            let Some(cell) = &slot.cell else { continue };
            Self::children(
                Value::heap(index as u32),
                cell,
                &self.properties,
                &mut held,
                &mut |_, _, _, _, _| {},
                GcTraceKind::Collection,
                &mut last_proto,
            );
            match cell {
                Cell::WeakMap { entries, .. } => held.extend(entries.iter().flat_map(|(key, value)| [key, value])),
                Cell::WeakSet { entries, .. } => held.extend(entries.iter()),
                Cell::WeakRef { target: Some(target), .. } => held.push(target.value),
                _ => {}
            }
            work.extend(held.drain(..).filter(unmarked));
        }
        if let Some(arrays) = &self.sparse_arrays {
            for elements in arrays.values() {
                work.extend(elements.values.values().copied().filter(unmarked));
            }
        }
    }

    /// The end of an epoch, after a minor collection's mark and before its sweep: the young
    /// survivors count their children, and so do again the candidates the mark re-marked,
    /// whose cascade decremented them. Survivors and re-marked candidates left at 0 are tried
    /// at the next minor collection.
    pub(super) fn rc_epoch(&mut self) {
        // The object, array and string survivors counted their children in the mark
        // (`rc_counted`); functions, promises, awaits, iterators and collections (with the
        // candidates the mark re-marked, aged) count theirs here, and vectors hold none.
        let counted = std::mem::take(&mut self.rc_counted);
        let mut survivors = Vec::new();
        let mut family = 0..;
        for_each_space_mut!(self, |space| {
            space.rc_ensure();
            if family.next().is_some_and(|family| (2..7).contains(&family)) {
                let mut young = Vec::new();
                space.for_each_young_survivor(|id| young.push(id));
                survivors.extend(young.into_iter().map(|id| space.value_of(id)));
            }
        });
        let mut children = Vec::new();
        self.rc_increment_children(survivors.iter().copied(), &mut children);
        let candidates = std::mem::take(&mut self.rc_candidates);
        self.rc_trace[4] = (counted + survivors.len()) as u64;
        if crate::trace::Channel::Gc.on() {
            self.rc_trace[5] = candidates.iter().filter(|&&value| !self.rc_is_old(value)).count() as u64;
        }
        // Every survivor has counted its children now; those left at 0, re-marked candidates
        // among them (aged), are found a word of counts at a time.
        let mut zero = std::mem::take(&mut self.rc_zero);
        for_each_space_mut!(self, |space| {
            let mut zeroed = Vec::new();
            space.rc_zero_survivors(|id| zeroed.push(id));
            zero.extend(zeroed.into_iter().map(|id| space.value_of(id)));
        });
        self.rc_zero = zero;
    }

    /// A full collection's start under counts, before its marks are cleared: the epoch's
    /// counts change as at a minor collection (no record is unmarked), and the records old
    /// now are noted (`rc_old`) for `rc_full_end`.
    pub(super) fn rc_full_start(&mut self) {
        let mut decrements: Vec<Value> = Vec::new();
        let mut logged: Vec<Value> = Vec::new();
        let mut old = Vec::new();
        for_each_space_mut!(self, |space| {
            let (copied, written) = space.take_rc_epoch();
            old.push(space.rc_old_bits(&written));
            decrements.extend(copied);
            logged.extend(written.into_iter().map(|id| space.value_of(id)));
        });
        let mut children = Vec::new();
        self.rc_increment_children(logged, &mut children);
        for value in decrements {
            if self.rc_decrement(value) {
                self.rc_zero.push(value);
            }
        }
        self.rc_old = old;
    }

    /// A full collection's end under counts, after its mark and before its sweep, in place of
    /// a recount: the old records it frees drop their references, and the young records it
    /// keeps count theirs, as the survivors of a minor collection do. A record's count then
    /// stands for its holders among the survivors, as a recount would make it, at the cost of
    /// the records that changed age, not of every live one.
    pub(super) fn rc_full_end(&mut self) {
        let old = std::mem::take(&mut self.rc_old);
        let mut survivors = Vec::new();
        let mut dead = Vec::new();
        let mut olds = old.iter();
        for_each_space_mut!(self, |space| {
            space.rc_ensure();
            if let Some(bits) = olds.next() {
                let mut kept = Vec::new();
                let mut gone = Vec::new();
                space.rc_full_changes(bits, |id, survived| if survived { kept.push(id) } else { gone.push(id) });
                survivors.extend(kept.into_iter().map(|id| space.value_of(id)));
                dead.extend(gone.into_iter().map(|id| space.value_of(id)));
            }
        });
        let mut children = Vec::new();
        self.rc_increment_children(survivors.iter().copied(), &mut children);
        let mut zero = std::mem::take(&mut self.rc_zero);
        for owner in dead {
            self.rc_children(owner, &mut children);
            for child in children.drain(..) {
                if self.rc_decrement(child) {
                    zero.push(child);
                }
            }
        }
        zero.extend(survivors);
        zero.retain(|&value| self.rc_is_old(value) && self.rc_count_of(value) == Some(0));
        zero.sort_unstable_by_key(|value| value.to_bits());
        zero.dedup();
        self.rc_zero = zero;
        self.rc_candidates.clear();
    }

    /// The candidates the minor mark left unmarked: what the sweep would free.
    pub(super) fn rc_unmarked_candidates(&self) -> Vec<Value> {
        self.rc_candidates.iter().copied().filter(|&value| !self.rc_is_old(value)).collect()
    }

    /// Reports, after a full collection's mark, each kept suspect (`rc_suspects`) the trace
    /// reached, which counts would have freed while reachable, with the live holders that
    /// name it: a count or a pin that missed an edge.
    pub(super) fn rc_check_suspects(&mut self) {
        let mut suspects = std::mem::take(&mut self.rc_suspects);
        suspects.sort_unstable_by_key(|value| value.to_bits());
        suspects.dedup();
        let reached: rustc_hash::FxHashSet<Value> =
            suspects.iter().copied().filter(|&value| self.rc_is_old(value)).collect();
        eprintln!("rc-verify: {} suspects, {} reachable", suspects.len(), reached.len());
        if reached.is_empty() {
            return;
        }
        for value in reached.iter().take(4) {
            eprintln!("rc-verify: reachable suspect {value:?} count {:?}", self.rc_count_of(*value));
        }
        let mut reported = 0;
        let mut children = Vec::new();
        for family in self.record_families() {
            family.for_each_live_id(&mut |id| {
                let holder = family.value_of(id);
                if !self.rc_is_old(holder) {
                    return;
                }
                self.rc_children(holder, &mut children);
                for child in children.drain(..) {
                    if reached.contains(&child) && reported < 8 {
                        reported += 1;
                        eprintln!("rc-verify: record {holder:?} (count {:?}) names {child:?}", self.rc_count_of(holder));
                    }
                }
            });
        }
        let mut last_proto = PushedProtos::default();
        for (index, slot) in self.slots.iter().enumerate() {
            let Some(cell) = &slot.cell else { continue };
            Self::children(Value::heap(index as u32), cell, &self.properties, &mut children, &mut |_, _, _, _, _| {}, GcTraceKind::Collection, &mut last_proto);
            for child in children.drain(..) {
                if reached.contains(&child) && reported < 16 {
                    reported += 1;
                    eprintln!("rc-verify: cell {index} ({:?}) names {child:?}", std::mem::discriminant(cell));
                }
            }
        }
        if self.roots.values().any(|value| reached.contains(&value)) {
            eprintln!("rc-verify: the root table names a reachable suspect");
        }
    }

    /// The next collection under counts: minor ones, the epochs, at the young budget, and the
    /// backup trace once the live records pass `RC_FULL_GROWTH` times the last full's.
    pub(super) fn rc_plan(&self, live: usize) -> pacing::NextCollection {
        let pacing = self.pacing();
        pacing::NextCollection {
            threshold: pacing.young_budget(live),
            full: live > RC_FULL_GROWTH * self.live_after_full.max(RC_FULL_FLOOR)
                || crate::trace::Dev::RcVerify.on() && self.collections % 8 == 7,
        }
    }

    /// Recounts every live record's references from scratch, as counting starts, and
    /// forgets the epoch's buffers.
    pub(super) fn rc_recount(&mut self) {
        for_each_space_mut!(self, |space| {
            space.rc_clear();
            space.take_rc_epoch();
        });
        // Only old (marked) records hold counted references and have counts: young ones are
        // counted when they survive a minor collection.
        let mut children = Vec::new();
        for family in self.record_families() {
            family.for_each_live_id(&mut |id| {
                let value = family.value_of(id);
                if self.rc_is_old(value) {
                    self.rc_increment_children([value], &mut children);
                }
            });
        }
        let mut zero = Vec::new();
        for family in self.record_families() {
            family.for_each_live_id(&mut |id| {
                let value = family.value_of(id);
                if self.rc_is_old(value) && self.rc_count_of(value) == Some(0) {
                    zero.push(value);
                }
            });
        }
        self.rc_zero = zero;
        self.rc_candidates.clear();
    }

    /// The records whose count differs from the references the live records' contents hold
    /// with the count and the references: none when counting is right.
    #[cfg(test)]
    pub(super) fn rc_mismatches(&self) -> Vec<(Value, u32, usize)> {
        let mut references: std::collections::HashMap<Value, usize> = std::collections::HashMap::new();
        let mut children = Vec::new();
        for family in self.record_families() {
            family.for_each_live_id(&mut |id| {
                self.rc_children(family.value_of(id), &mut children);
                for child in children.drain(..) {
                    if live_cell(self, child).is_some() || locate(self, child).is_some() {
                        *references.entry(child).or_default() += 1;
                    }
                }
            });
        }
        let mut mismatches = Vec::new();
        for family in self.record_families() {
            family.for_each_live_id(&mut |id| {
                let value = family.value_of(id);
                let count = match live_cell(self, value) {
                    Some(granule) => self.class_space.rc_exact_count(granule),
                    None => {
                        let (family, id) = locate(self, value).expect("a record");
                        with_space!(self, family, |space| space.rc_exact_count(id))
                    }
                };
                let held = references.get(&value).copied().unwrap_or(0);
                if count != u32::MAX && count as usize != held {
                    mismatches.push((value, count, held));
                }
            });
        }
        mismatches
    }
}
