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

    /// The end of an epoch, in a minor collection after its mark and before its sweep: the
    /// records the barrier logged and the young survivors count their current children, and
    /// the children the barrier copied before the logged records' first writes are
    /// decremented.
    pub(super) fn rc_epoch(&mut self) {
        let mut owners: Vec<Value> = Vec::new();
        let mut decrements: Vec<Value> = Vec::new();
        for_each_space_mut!(self, |space, handle| {
            let (copied, logged) = space.take_rc_epoch();
            decrements.extend(copied);
            owners.extend(logged.into_iter().map(handle));
            space.for_each_young_survivor(|id| owners.push(handle(id)));
        });
        let mut children = Vec::new();
        self.rc_increment_children(owners, &mut children);
        for value in decrements {
            self.rc_decrement(value);
        }
    }

    /// Recounts every live record's references from scratch, after a full collection's sweep:
    /// the backup trace resets stuck counts and forgets the epoch's buffers.
    pub(super) fn rc_recount(&mut self) {
        for_each_space_mut!(self, |space, _handle| {
            space.rc_clear();
            space.take_rc_epoch();
        });
        let mut children = Vec::new();
        for (family, handle) in self.record_families() {
            family.for_each_live_id(&mut |id| self.rc_increment_children([handle(id)], &mut children));
        }
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
