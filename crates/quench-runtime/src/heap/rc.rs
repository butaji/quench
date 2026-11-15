//! Reference counting under `QUENCH_DEV=rc-gc`, the foundation of a hybrid collector in the
//! LXR style (Zhao, Blackburn, McKinley, PLDI 2022); see
//! research/nojit/splay/rc-design.txt.
//!
//! An old record's count is the number of references to it from old records' contents (the
//! values their `SatbSnapshot` copies, and a string's parts), saturating at `RC_STUCK`.
//! Young records are not counted until they survive a minor collection, which makes every
//! survivor old (`PromotionAge::First`). The barrier copies an old record's children before
//! its first write in an epoch (`RecordSpace::rc_note`); each minor collection, the epoch's
//! end, increments the children of the records it logged and of the young survivors, and
//! decrements the copied ones. A full collection recounts from scratch.
use super::*;
use records::SatbSnapshot;

/// The record family of a counted value and its id there: the index into
/// `Heap::record_families`, and the id.
fn locate(value: Value) -> Option<(usize, RecordId)> {
    if let Some(id) = value.direct_object_index() {
        Some((0, id))
    } else if let Some(id) = value.direct_array_index() {
        Some((1, id))
    } else if let Some(id) = value.direct_function_index() {
        Some((2, id))
    } else if let Some(id) = value.direct_promise_index() {
        Some((3, id))
    } else if let Some(id) = value.direct_await_index() {
        Some((4, id))
    } else if let Some(id) = value.direct_iterator_index() {
        Some((5, id))
    } else if let Some(id) = value.direct_string_index() {
        Some(match small_strings::small_string_local(id) {
            Some(local) => (7, local),
            None => (6, id),
        })
    } else {
        value.direct_v128_index().map(|id| (8, id))
    }
}

/// Runs `$body` with `$space` bound to the record space of family `$family` (`locate`).
macro_rules! with_space {
    ($heap:expr, $family:expr, |$space:ident| $body:expr) => {
        match $family {
            0 => { let $space = &$heap.object_records; $body }
            1 => { let $space = &$heap.array_records; $body }
            2 => { let $space = &$heap.function_records; $body }
            3 => { let $space = &$heap.promise_records; $body }
            4 => { let $space = &$heap.await_records; $body }
            5 => { let $space = &$heap.iterator_records; $body }
            6 => { let $space = &$heap.string_records; $body }
            7 => { let $space = &$heap.small_strings; $body }
            _ => { let $space = &$heap.v128_records; $body }
        }
    };
}

/// `with_space!`, mutably.
macro_rules! with_space_mut {
    ($heap:expr, $family:expr, |$space:ident| $body:expr) => {
        match $family {
            0 => { let $space = &mut $heap.object_records; $body }
            1 => { let $space = &mut $heap.array_records; $body }
            2 => { let $space = &mut $heap.function_records; $body }
            3 => { let $space = &mut $heap.promise_records; $body }
            4 => { let $space = &mut $heap.await_records; $body }
            5 => { let $space = &mut $heap.iterator_records; $body }
            6 => { let $space = &mut $heap.string_records; $body }
            7 => { let $space = &mut $heap.small_strings; $body }
            _ => { let $space = &mut $heap.v128_records; $body }
        }
    };
}

/// Records a minor collection makes candidates for freeing, at most: the rest of a cascade
/// carries to the next one.
const RC_FREE_BUDGET: usize = 1 << 16;

/// Live records past this multiple of the last full collection's survivors (at least
/// `RC_FULL_FLOOR`) make the next collection the backup trace, for cycles and stuck counts.
const RC_FULL_GROWTH: usize = 4;
const RC_FULL_FLOOR: usize = 16 * 1024;

/// Runs `$body` once for each record space, mutably, with its value constructor.
macro_rules! for_each_space_mut {
    ($heap:expr, |$space:ident, $handle:ident| $body:expr) => {{
        { let $space = &mut $heap.object_records; let $handle: fn(RecordId) -> Value = Value::direct_object; $body }
        { let $space = &mut $heap.array_records; let $handle: fn(RecordId) -> Value = Value::direct_array; $body }
        { let $space = &mut $heap.function_records; let $handle: fn(RecordId) -> Value = Value::direct_function; $body }
        { let $space = &mut $heap.promise_records; let $handle: fn(RecordId) -> Value = Value::direct_promise; $body }
        { let $space = &mut $heap.await_records; let $handle: fn(RecordId) -> Value = Value::direct_await; $body }
        { let $space = &mut $heap.iterator_records; let $handle: fn(RecordId) -> Value = Value::direct_iterator; $body }
        { let $space = &mut $heap.string_records; let $handle: fn(RecordId) -> Value = Value::direct_string; $body }
        { let $space = &mut $heap.small_strings; let $handle: fn(RecordId) -> Value = small_strings::small_string_value; $body }
        { let $space = &mut $heap.v128_records; let $handle: fn(RecordId) -> Value = Value::direct_v128; $body }
    }};
}

impl Heap {
    /// Whether old records keep reference counts.
    pub(crate) fn rc_active(&self) -> bool {
        self.rc_active
    }

    /// Turns counting on, from a recount of the live records, or off.
    pub(crate) fn set_rc(&mut self, on: bool) {
        self.rc_active = on;
        for_each_space_mut!(self, |space, _handle| space.set_rc(on));
        if on {
            self.rc_recount();
        }
    }

    /// Appends the values live record `value` keeps alive that counts may name: what its
    /// snapshot copies (`SatbSnapshot`), or a string's parts. Values that are no records
    /// (legacy cells, numbers) are among them; `locate` skips them.
    fn rc_children(&self, value: Value, out: &mut Vec<Value>) {
        let properties = &self.properties;
        // SAFETY: the caller names live records.
        unsafe {
            if let Some(id) = value.direct_object_index() {
                self.object_records.get_live(id).snapshot(properties, out);
            } else if let Some(id) = value.direct_array_index() {
                self.array_records.get_live(id).snapshot(properties, out);
            } else if let Some(id) = value.direct_function_index() {
                self.function_records.get_live(id).snapshot(properties, out);
            } else if let Some(id) = value.direct_promise_index() {
                self.promise_records.get_live(id).snapshot(properties, out);
            } else if let Some(id) = value.direct_await_index() {
                self.await_records.get_live(id).snapshot(properties, out);
            } else if let Some(id) = value.direct_iterator_index() {
                self.iterator_records.get_live(id).snapshot(properties, out);
            } else if let Some(id) = value.direct_string_index() {
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
    fn rc_increment(&self, value: Value) {
        if let Some((family, id)) = locate(value) {
            with_space!(self, family, |space| space.rc_increment(id));
        }
    }

    /// Decrements the count of `value` if it is a record; whether it reached 0.
    fn rc_decrement(&self, value: Value) -> bool {
        match locate(value) {
            Some((family, id)) => with_space!(self, family, |space| space.rc_decrement(id)),
            None => false,
        }
    }

    /// The count of record `value`, if it is one.
    fn rc_count_of(&self, value: Value) -> Option<u8> {
        locate(value).map(|(family, id)| with_space!(self, family, |space| space.rc_count(id)))
    }

    /// Whether record `value` is old: marked between collections.
    fn rc_is_old(&self, value: Value) -> bool {
        locate(value).is_some_and(|(family, id)| with_space!(self, family, |space| space.is_marked(id)))
    }

    /// `rc_count_of`, for tests.
    #[cfg(test)]
    pub(super) fn rc_count(&self, value: Value) -> Option<u8> {
        locate(value).map(|(family, id)| with_space!(self, family, |space| space.rc_count(id)))
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
        for_each_space_mut!(self, |space, handle| {
            let (copied, written) = space.take_rc_epoch();
            decrements.extend(copied);
            logged.extend(written.into_iter().map(handle));
        });
        let mut children = Vec::new();
        self.rc_increment_children(logged, &mut children);
        let mut zero = std::mem::take(&mut self.rc_zero);
        for value in decrements {
            if self.rc_decrement(value) {
                zero.push(value);
            }
        }
        let mut carried = Vec::new();
        let mut candidates = Vec::new();
        while let Some(value) = zero.pop() {
            if candidates.len() >= RC_FREE_BUDGET {
                carried.push(value);
                carried.append(&mut zero);
                break;
            }
            let Some((family, id)) = locate(value) else { continue };
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
        if carried.len() > 1024 {
            carried.sort_unstable_by_key(|value| value.to_bits());
            carried.dedup();
        }
        self.rc_zero = carried;
        if !candidates.is_empty() {
            self.rc_pin_cell_references(&candidates, work);
        }
        self.rc_candidates = candidates;
    }

    /// Adds to `work` each of `candidates` a live legacy cell names, weakly or not, or a
    /// sparse array holds: their references are not counted, and a minor collection traces
    /// old cells only where the cell barrier logged them.
    fn rc_pin_cell_references(&self, candidates: &[Value], work: &mut Vec<Value>) {
        let candidates: rustc_hash::FxHashSet<Value> = candidates.iter().copied().collect();
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
            work.extend(held.drain(..).filter(|value| candidates.contains(value)));
        }
        if let Some(arrays) = &self.sparse_arrays {
            for elements in arrays.values() {
                work.extend(elements.values.values().copied().filter(|value| candidates.contains(value)));
            }
        }
    }

    /// The end of an epoch, after a minor collection's mark and before its sweep: the young
    /// survivors count their children, and so do again the candidates the mark re-marked,
    /// whose cascade decremented them. Survivors and re-marked candidates left at 0 are tried
    /// at the next minor collection.
    pub(super) fn rc_epoch(&mut self) {
        let mut survivors: Vec<Value> = Vec::new();
        for_each_space_mut!(self, |space, handle| {
            space.rc_ensure();
            space.for_each_young_survivor(|id| survivors.push(handle(id)));
        });
        let mut children = Vec::new();
        self.rc_increment_children(survivors.iter().copied(), &mut children);
        let candidates = std::mem::take(&mut self.rc_candidates);
        for &candidate in &candidates {
            let Some((family, id)) = locate(candidate) else { continue };
            if with_space!(self, family, |space| space.is_marked(id)) {
                self.rc_increment_children([candidate], &mut children);
            }
        }
        let mut zero = std::mem::take(&mut self.rc_zero);
        for value in candidates.into_iter().chain(survivors) {
            let Some((family, id)) = locate(value) else { continue };
            if with_space!(self, family, |space| space.is_marked(id) && space.rc_count(id) == 0) {
                zero.push(value);
            }
        }
        self.rc_zero = zero;
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
        for (family, handle) in self.record_families() {
            family.for_each_live_id(&mut |id| {
                let holder = handle(id);
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

    /// Recounts every live record's references from scratch, after a full collection's sweep:
    /// the backup trace resets stuck counts and forgets the epoch's buffers.
    pub(super) fn rc_recount(&mut self) {
        for_each_space_mut!(self, |space, _handle| {
            space.rc_clear();
            space.take_rc_epoch();
        });
        // Only old (marked) records hold counted references and have counts: young ones are
        // counted when they survive a minor collection.
        let mut children = Vec::new();
        for (family, handle) in self.record_families() {
            family.for_each_live_id(&mut |id| {
                let value = handle(id);
                if self.rc_is_old(value) {
                    self.rc_increment_children([value], &mut children);
                }
            });
        }
        let mut zero = Vec::new();
        for (family, handle) in self.record_families() {
            family.for_each_live_id(&mut |id| {
                let value = handle(id);
                if self.rc_is_old(value) && self.rc_count_of(value) == Some(0) {
                    zero.push(value);
                }
            });
        }
        self.rc_zero = zero;
        self.rc_candidates.clear();
    }

    /// The records whose count differs from the references the live records' contents hold
    /// (saturating at `RC_STUCK`), with the count and the references: none when counting is
    /// right. A stuck count may stand for fewer references, which the barrier's missed
    /// deletions leave.
    #[cfg(test)]
    pub(super) fn rc_mismatches(&self) -> Vec<(Value, u8, usize)> {
        let mut references: std::collections::HashMap<Value, usize> = std::collections::HashMap::new();
        let mut children = Vec::new();
        for (family, handle) in self.record_families() {
            family.for_each_live_id(&mut |id| {
                self.rc_children(handle(id), &mut children);
                for child in children.drain(..) {
                    if locate(child).is_some() {
                        *references.entry(child).or_default() += 1;
                    }
                }
            });
        }
        let mut mismatches = Vec::new();
        for (family, handle) in self.record_families() {
            family.for_each_live_id(&mut |id| {
                let value = handle(id);
                let count = self.rc_count(value).expect("a record");
                let held = references.get(&value).copied().unwrap_or(0);
                if count != held.min(records::RC_STUCK as usize) as u8 && count != records::RC_STUCK {
                    mismatches.push((value, count, held));
                }
            });
        }
        mismatches
    }
}
