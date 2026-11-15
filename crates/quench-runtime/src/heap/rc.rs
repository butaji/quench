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
use records::SatbSnapshot;

/// The record families `locate` names, by index: the order of `with_space!` and of
/// `Heap::record_families`.
const FAMILY_OBJECT: usize = 0;
const FAMILY_ARRAY: usize = 1;
const FAMILY_FUNCTION: usize = 2;
const FAMILY_PROMISE: usize = 3;
const FAMILY_AWAIT: usize = 4;
const FAMILY_ITERATOR: usize = 5;
const FAMILY_STRING: usize = 6;
const FAMILY_SMALL_STRING: usize = 7;
const FAMILY_V128: usize = 8;

/// The record family of a counted value and its id there: the index into
/// `Heap::record_families`, and the id.
#[inline(always)]
fn locate(value: Value) -> Option<(usize, RecordId)> {
    if !value.is_heap() {
        None
    } else if let Some(id) = value.direct_object_index() {
        Some((FAMILY_OBJECT, id))
    } else if let Some(id) = value.direct_array_index() {
        Some((FAMILY_ARRAY, id))
    } else if let Some(id) = value.direct_function_index() {
        Some((FAMILY_FUNCTION, id))
    } else if let Some(id) = value.direct_promise_index() {
        Some((FAMILY_PROMISE, id))
    } else if let Some(id) = value.direct_await_index() {
        Some((FAMILY_AWAIT, id))
    } else if let Some(id) = value.direct_iterator_index() {
        Some((FAMILY_ITERATOR, id))
    } else if let Some(id) = value.direct_string_index() {
        Some(match small_strings::small_string_local(id) {
            Some(local) => (FAMILY_SMALL_STRING, local),
            None => (FAMILY_STRING, id),
        })
    } else {
        value.direct_v128_index().map(|id| (FAMILY_V128, id))
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
const RC_FREE_BUDGET: usize = 1 << 20;

/// Records the cascade prefetches together (`Heap::rc_pre_mark`).
const RC_CASCADE_BATCH: usize = 16;

/// Live records past this multiple of the last full collection's survivors (at least
/// `RC_FULL_FLOOR`) make the next collection the backup trace, for cycles and stuck counts.
const RC_FULL_GROWTH: usize = 4;
const RC_FULL_FLOOR: usize = 16 * 1024;
/// Carried records past this many are sorted and deduplicated before the next epoch.
const RC_CARRY_SORT_LEN: usize = 1024;
/// Every this many collections a full collection verifies the counts (`QUENCH_DEV=rc-verify`).
const RC_VERIFY_PERIOD: usize = 8;

/// Slots of `Heap::rc_trace`: the microseconds and sizes of one collection's counting.
const TRACE_LOG_MICROS: usize = 6;
const TRACE_CANDIDATES: usize = 2;
const TRACE_CARRIED: usize = 3;
const TRACE_SURVIVORS: usize = 4;
const TRACE_YOUNG_CANDIDATES: usize = 5;
const TRACE_CASCADE_MICROS: usize = 7;
const TRACE_PIN_MICROS: usize = 8;

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

mod cascade;
mod verify;

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
        for_each_space_mut!(self, |space, _handle| space.rc_ensure());
        for &value in values {
            if let Some((family, id)) = locate(value) {
                with_space!(self, family, |space| space.rc_pin(id));
            }
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
    #[inline(always)]
    pub(super) fn rc_increment(&self, value: Value) {
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

    /// Increments the counts of the record children of each of `owners`.
    fn rc_increment_children(&self, owners: impl IntoIterator<Item = Value>, children: &mut Vec<Value>) {
        for owner in owners {
            self.rc_children(owner, children);
            for child in children.drain(..) {
                self.rc_increment(child);
            }
        }
    }

    /// The end of an epoch, after a minor collection's mark and before its sweep: the young
    /// survivors count their children, and so do again the candidates the mark re-marked,
    /// whose cascade decremented them. Survivors and re-marked candidates left at 0 are tried
    /// at the next minor collection.
    pub(super) fn rc_epoch(&mut self) {
        // The object, array and string survivors counted their children in the mark
        // (`rc_counted`); functions, promises, awaits and iterators (with the candidates the
        // mark re-marked, aged) count theirs here, and vectors hold none.
        let counted = std::mem::take(&mut self.rc_counted);
        let mut survivors = Vec::new();
        let mut family = 0..;
        for_each_space_mut!(self, |space, handle| {
            space.rc_ensure();
            if family.next().is_some_and(|family| (FAMILY_FUNCTION..FAMILY_STRING).contains(&family)) {
                space.for_each_young_survivor(|id| survivors.push(handle(id)));
            }
        });
        let mut children = Vec::new();
        self.rc_increment_children(survivors.iter().copied(), &mut children);
        let candidates = std::mem::take(&mut self.rc_candidates);
        self.rc_trace[TRACE_SURVIVORS] = (counted + survivors.len()) as u64;
        if crate::trace::Channel::Gc.on() {
            self.rc_trace[TRACE_YOUNG_CANDIDATES] = candidates.iter().filter(|&&value| !self.rc_is_old(value)).count() as u64;
        }
        // Every survivor has counted its children now; those left at 0, re-marked candidates
        // among them (aged), are found a word of counts at a time.
        let mut zero = std::mem::take(&mut self.rc_zero);
        for_each_space_mut!(self, |space, handle| space.rc_zero_survivors(|id| zero.push(handle(id))));
        self.rc_zero = zero;
    }

    /// A full collection's start under counts, before its marks are cleared: the epoch's
    /// counts change as at a minor collection (no record is unmarked), and the records old
    /// now are noted (`rc_old`) for `rc_full_end`.
    pub(super) fn rc_full_start(&mut self) {
        let mut decrements: Vec<Value> = Vec::new();
        let mut logged: Vec<Value> = Vec::new();
        let mut old = Vec::new();
        for_each_space_mut!(self, |space, handle| {
            let (copied, written) = space.take_rc_epoch();
            old.push(space.rc_old_bits(&written));
            decrements.extend(copied);
            logged.extend(written.into_iter().map(handle));
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
        for_each_space_mut!(self, |space, handle| {
            space.rc_ensure();
            if let Some(bits) = olds.next() {
                space.rc_full_changes(bits, |id, survived| if survived { survivors.push(handle(id)) } else { dead.push(handle(id)) });
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

    /// The next collection under counts: minor ones, the epochs, at the young budget, and the
    /// backup trace once the live records pass `RC_FULL_GROWTH` times the last full's.
    pub(super) fn rc_plan(&self, live: usize) -> pacing::NextCollection {
        let pacing = self.pacing();
        pacing::NextCollection {
            threshold: pacing.young_budget(live),
            full: live > RC_FULL_GROWTH * self.live_after_full.max(RC_FULL_FLOOR)
                || crate::trace::Dev::RcVerify.on() && self.collections % RC_VERIFY_PERIOD == RC_VERIFY_PERIOD - 1,
        }
    }

    /// Recounts every live record's references from scratch, as counting starts, and
    /// forgets the epoch's buffers.
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

}
