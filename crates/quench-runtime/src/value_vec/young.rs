//! The nursery: young runs bumped from its range, their reservations, and the collection that evacuates them.
use super::*;

impl ValueArena {
    /// A young run (`Nursery`) for a new object, holding the first `len` of `values`, whose
    /// values past `len` are `undefined`, as a run's unused values start; `None` once the
    /// arena's positions are spent. Its owner must be a direct object record, whose survival
    /// the sweep shows. The run's whole capacity is copied, as the smallest class and then the
    /// pairs of values each larger class adds (`capacity_class`), moves of fixed size rather
    /// than a copy of variable length, which is a call.
    #[inline(always)] pub(crate) fn young_run_padded<const N: usize>(&mut self, values: &[Value; N], len: usize) -> Option<u32> {
        let capacity = capacity_class(len);
        debug_assert!(len <= N && capacity <= N && values[len..].iter().all(|value| *value == Value::UNDEFINED));
        let start = self.young_run(capacity)?;
        let run = &mut self.values[start..start + capacity];
        let (first, mut rest) = run.split_at_mut(MIN_CAPACITY);
        <&mut [Value; MIN_CAPACITY]>::try_from(first).unwrap().copy_from_slice(&values[..MIN_CAPACITY]);
        let mut copied = MIN_CAPACITY;
        while let Some((pair, tail)) = rest.split_first_chunk_mut::<CAPACITY_CLASS_STEP>() {
            *pair = *values[copied..].first_chunk::<CAPACITY_CLASS_STEP>().unwrap();
            copied += CAPACITY_CLASS_STEP;
            rest = tail;
        }
        Some(start as u32)
    }

    /// Whether `vector`, a young run outside dictionary and narrow storage, may hold `len`
    /// values in place, past the capacity its current length gives it: its reservation
    /// (`reserve_predicted_run`) still holds, from its start, the whole capacity of `len` values
    /// (`ordered_capacity`), which every later reader takes as the run's.
    #[inline(always)]
    pub(crate) fn reserved_room(&self, vector: ValueVec, len: usize) -> bool {
        if vector.is_narrow() || vector.is_dictionary() || vector.has_inline_property_storage() || !self.holds_young_run(vector) {
            return false;
        }
        let start = vector.start();
        let entry = self.reservations[Reservation::index(start)];
        entry.cycle == self.reservation_cycle && entry.start as usize == start && start + capacity_class(len) <= entry.end as usize
    }

    /// Ends every reservation (`Reservation`): the young runs they name may move or be reused.
    #[inline(always)]
    pub(super) fn forget_reservations(&mut self) {
        self.reservation_cycle = self.reservation_cycle.wrapping_add(1);
    }

    /// A young run of `capacity`, bumped from the nursery's range.
    #[inline(always)]
    pub(super) fn young_run(&mut self, capacity: usize) -> Option<usize> {
        let start = self.nursery.top;
        if capacity <= self.nursery.end - start {
            self.nursery.top = start + capacity;
            return Some(start);
        }
        self.young_run_slow(capacity)
    }

    /// A young run for the elements of a new array of `len` values (`ArrayElements::young`):
    /// a header and the values, bumped from the nursery, which grows as `young_run_slow`
    /// grows it; `None` for an empty array or one longer than `YOUNG_ELEMENTS_MAX`, once the
    /// nursery cannot take the run, or where the arena's values may move as it grows, which
    /// would move the run beneath its array. Never an ordinary run: a young array's death
    /// releases nothing, and the collection it survives copies its elements out
    /// (`ArrayElements::leave_nursery`).
    #[inline(always)] pub(crate) fn young_elements(&mut self, len: usize) -> Option<std::ptr::NonNull<Value>> {
        // An empty array holds the shared empty header.
        if len.wrapping_sub(1) >= YOUNG_ELEMENTS_MAX || !self.values.keeps_addresses() {
            return None;
        }
        let capacity = crate::heap::ARRAY_HEADER_VALUES + len;
        let start = self.nursery.top;
        let start = if capacity <= self.nursery.end - start {
            self.nursery.top = start + capacity;
            start
        } else {
            self.grow_nursery(capacity)?
        };
        // SAFETY: the run lies within the arena's initialized values.
        Some(unsafe { std::ptr::NonNull::new_unchecked(self.values.as_mut_ptr().add(start)) })
    }

    /// `young_run` past the nursery's end. A released run of its class comes first: it is
    /// resident already, where the nursery's growth extends the arena. While the free lists
    /// held what a full collection released, a nursery that kept growing extended the arena
    /// past them (acorn-wtb: 43K values more at the peak, 0.4 MB of max RSS). Its owner's
    /// death releases it, as before. Within its range the nursery bumps whatever the free
    /// lists hold: preferring them there let the dead young objects' releases refill them,
    /// and kept new objects off the nursery for good (construct 416 -> 436 Ir). Else a range that ends the arena grows by
    /// `NURSERY_GROWTH`, and an empty one, as a compaction leaves it, starts at the arena's
    /// end; else the run is an ordinary one (`allocate`).
    #[inline(never)]
    pub(super) fn young_run_slow(&mut self, capacity: usize) -> Option<usize> {
        if !self.free[Self::bucket(capacity)].is_empty() {
            return self.allocate(capacity, MAX_ARENA_START);
        }
        self.grow_nursery(capacity).or_else(|| self.allocate(capacity, MAX_ARENA_START))
    }

    /// A young run of `capacity` past the nursery's end, where its range ends the arena and
    /// can grow by `NURSERY_GROWTH`; an empty range, as a compaction leaves it, starts again
    /// at the arena's end.
    #[inline(never)]
    pub(super) fn grow_nursery(&mut self, capacity: usize) -> Option<usize> {
        let tail = self.values.len();
        if self.nursery.end == self.nursery.floor {
            self.nursery = Nursery { floor: tail, top: tail, end: tail };
            self.forget_reservations();
        }
        let end = self.nursery.top + capacity.max(NURSERY_GROWTH);
        // Every young run is written whole before it is read, as a range reused after a young
        // cycle holds the dead runs' values: the range's values are not filled, so only the
        // runs taken make its pages resident (1 MB of `undefined` before, for any program
        // that took one young run: V8-v7 RegExp's harness check).
        if self.nursery.end == tail && end <= MAX_ARENA_START && self.values.extend_unfilled(end) {
            self.nursery.end = end;
            let start = self.nursery.top;
            self.nursery.top = start + capacity;
            return Some(start);
        }
        None
    }

    /// The young runs taken since the last collection, as a sweep reads them beside its
    /// releases.
    #[inline(always)] pub(crate) fn young_runs(&self) -> YoungRuns {
        YoungRuns { floor: self.nursery.floor, top: self.nursery.top }
    }

    /// Whether `vector` holds a young run (`Nursery`).
    #[inline(always)] pub(crate) fn holds_young_run(&self, vector: ValueVec) -> bool {
        self.young_runs().hold(vector)
    }

    /// Moves the young run of `vector`, whose owner survives the collection under way, out of
    /// the young range: to a released run of its size class, else down to the nursery's
    /// floor, which rises past it. Survivors must come in the order of their starts, so each
    /// one moved to the floor lands at or below its source.
    pub(crate) fn evacuate_young(&mut self, vector: &mut ValueVec, narrows: bool) {
        debug_assert!(self.holds_young_run(*vector));
        // Survivors land below their sources, perhaps at a reserved run's start.
        self.forget_reservations();
        let length = self.len(*vector);
        let source = vector.start();
        if narrows && length > 0 && self.narrows_to_fewer_words(*vector, length) {
            let mut narrow = *vector;
            narrow.auxiliary |= NARROW_FORM;
            let capacity = Self::ordered_capacity(narrow, length);
            let target = self.young_target(capacity, source);
            // The target lies before the source (a free range or the nursery floor below it),
            // so word `k` is written after values `2k` and `2k + 1` are read only when apart:
            // read the values first.
            let words = self.narrow_words_of(source, length);
            self.values[target..target + words.len()].copy_from_slice(&words);
            self.values[target + words.len()..target + capacity].fill(Value::UNDEFINED);
            narrow.relocate(target);
            *vector = narrow;
            return;
        }
        let capacity = Self::ordered_capacity(*vector, length);
        let target = self.young_target(capacity, source);
        self.values.copy_within(source..source + capacity, target);
        vector.relocate(target);
    }

    /// `vector`, an ordinary run of a plain object whose values all have a narrow form and
    /// fill a smaller class narrow, moved into a released range of the narrow class, its own
    /// range released in turn: what a full collection without compaction does for the runs
    /// it keeps (`Heap::narrow_live_runs`). Without such a range the run narrows in place and
    /// its unused tail is released, where the tail is a class of its own. The arena never
    /// grows for it. `None` changes nothing.
    pub(crate) fn narrow_into_released(&mut self, vector: ValueVec) -> Option<ValueVec> {
        debug_assert!(!vector.is_narrow() && !vector.is_dictionary() && !vector.has_inline_property_storage());
        let length = self.len(vector);
        if length == 0 || !self.narrows_to_fewer_words(vector, length) {
            return None;
        }
        let mut narrow = vector;
        narrow.auxiliary |= NARROW_FORM;
        let capacity = Self::ordered_capacity(narrow, length);
        let tail = Self::ordered_capacity(vector, length) - capacity;
        if self.free[Self::bucket(capacity)].is_empty() && capacity_class(tail) != tail {
            return None;
        }
        let words = self.narrow_words_of(vector.start(), length);
        let target = match self.pop_free(Self::bucket(capacity)) {
            Some(target) => {
                self.released_values -= capacity;
                self.release_held(vector);
                target as usize
            }
            None => {
                // No released range of the narrow class: the run narrows where it is, and the
                // words it no longer fills go free as a range of their own class.
                let source = vector.start();
                self.push_free(Self::bucket(tail), (source + capacity) as u32);
                self.released_values += tail;
                source
            }
        };
        self.values[target..target + words.len()].copy_from_slice(&words);
        self.values[target + words.len()..target + capacity].fill(Value::UNDEFINED);
        narrow.relocate(target);
        Some(narrow)
    }

    /// Where a surviving young run of `capacity` words at `source` moves: a free range of its
    /// class, or the nursery floor, which never passes the source.
    pub(super) fn young_target(&mut self, capacity: usize, source: usize) -> usize {
        match self.pop_free(Self::bucket(capacity)) {
            Some(target) => {
                self.released_values -= capacity;
                target as usize
            }
            None => {
                let target = self.nursery.floor;
                debug_assert!(target <= source);
                self.nursery.floor += capacity;
                target
            }
        }
    }

    /// The narrow words of the `length` values at `source`, each of which has a narrow form.
    pub(super) fn narrow_words_of(&self, source: usize, length: usize) -> smallvec::SmallVec<[Value; 16]> {
        (0..narrow_words(length)).map(|word| {
            let low = self.values[source + 2 * word].to_narrow().expect("checked narrow");
            let high = if 2 * word + 1 < length { self.values[source + 2 * word + 1].to_narrow().expect("checked narrow") } else { 0 };
            let (first, second) = if cfg!(target_endian = "little") { (low, high) } else { (high, low) };
            Value::from_bits(u64::from(first) | u64::from(second) << 32)
        }).collect()
    }

    /// Ends a collection's young cycle once its survivors have left (`evacuate_young`): the
    /// dead young runs are free at once.
    pub(crate) fn finish_young_cycle(&mut self) {
        self.nursery.top = self.nursery.floor;
        self.forget_reservations();
    }
}
