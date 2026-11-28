//! Growing a vector to its next size class, and allocating ranges by size class.
use super::*;

impl ValueArena {
    /// Moves `vector` to a range of the next size class. A young run's owner is a newborn
    /// object (`Nursery`), so its next range is a young run too: grown in place when it ends
    /// the young range, as a new object's runs mostly do while its constructor adds
    /// properties, else bumped. An ordinary range for it left the young one dead in the
    /// nursery until the collection, and pushed the arena's end past the nursery's.
    ///
    /// `want` is the capacity an add that moves the run predicts for it. A young run takes it
    /// in one jump where the nursery has the room, and reserves the tail past the class
    /// (`Reservation`), so the adds up to `want` append in place. An ordinary run takes the
    /// next class alone: its capacity is a fact of its length (`ordered_capacity`), so a tail
    /// it could not release would be lost to the free lists.
    #[inline(never)]
    pub(super) fn grow(&mut self, vector: &mut ValueVec, max_start: usize, want: usize) {
        assert!(!vector.is_narrow(), "growth widens a narrow vector first");
        let len = self.len(*vector);
        let capacity = self.capacity(*vector);
        let class = grown_capacity(capacity, capacity + 1, GrowthPolicy::SizeClass);
        let young = self.holds_young_run(*vector);
        // A reserved run (`Reservation`) takes the next class from its reservation.
        if young && self.reserved_room(*vector, capacity + 1) {
            return;
        }
        // The predicted tail is bumped from the nursery's room only, so it never grows the arena.
        let in_place = young && vector.start() + capacity == self.nursery.top;
        let room = if in_place { vector.start() } else { self.nursery.top };
        let predicted = young && want > class && room + want <= self.nursery.end;
        let next = if predicted { want } else { class };
        if in_place && vector.start() + next <= self.nursery.end {
            self.nursery.top = vector.start() + next;
            self.reserve_predicted_run(vector.start(), next, class);
            return;
        }
        let start = if young { self.young_run(next) } else { self.allocate(next, max_start) };
        let Some(start) = start else {
            let values = if self.has_compact_range(*vector) {
                self.decoded_run(vector.start(), len)
            } else {
                vec![Value::UNDEFINED; len]
            };
            self.release(*vector);
            let id = self.allocate_dictionary(values);
            vector.use_dictionary(id);
            return;
        };
        if predicted {
            self.reserve_predicted_run(start, next, class);
        }
        if len != 0 {
            self.values
                .copy_within(vector.start()..vector.start() + len, start);
            self.release(*vector);
        }
        vector.start = start as u32 | (vector.start & !START_MASK);
    }

    /// Records the young run at `start` as `capacity` values long, past its `class`, so its owner
    /// appends in place up to that length (`reserved_room`). The tail is the owner's alone until
    /// the young cycle ends, which `forget_reservations` marks, and an entry another run evicts
    /// only loses that room.
    #[inline(always)]
    pub(super) fn reserve_predicted_run(&mut self, start: usize, capacity: usize, class: usize) {
        if capacity > class {
            self.reservations[Reservation::index(start)] =
                Reservation { start: start as u32, end: (start + capacity) as u32, cycle: self.reservation_cycle };
        }
    }

    #[cfg(test)]
    pub(crate) fn migrate_to_dictionary_for_test(&mut self, vector: &mut ValueVec) {
        self.grow(vector, 0, 0);
    }

    /// A range of `capacity`: a released one of its size class, else, out of line, one past
    /// the arena's end (`allocate_fresh`).
    #[inline(always)]
    pub(super) fn allocate(&mut self, capacity: usize, max_start: usize) -> Option<usize> {
        let bucket = Self::bucket(capacity);
        if let Some(start) = self.pop_free(bucket) {
            self.released_values -= capacity;
            return Some(start as usize);
        }
        self.allocate_fresh(capacity, max_start)
    }

    #[inline(never)]
    pub(super) fn allocate_fresh(&mut self, capacity: usize, max_start: usize) -> Option<usize> {
        let start = self.values.len();
        if start >= max_start {
            return None;
        }
        let required = start.checked_add(capacity)?;
        if !self.values.resize(required) { return None; }
        Some(start)
    }

    /// The free list of a size class (`capacity_class`): `MIN_CAPACITY` and its step first,
    /// then four per doubling, the power of two and the quarter steps above it.
    #[inline(always)]
    pub(super) fn bucket(capacity: usize) -> usize {
        debug_assert_eq!(capacity_class(capacity), capacity, "a range capacity is a size class");
        let bucket = if capacity < 2 * MIN_CAPACITY {
            usize::from(capacity != MIN_CAPACITY)
        } else {
            let octave = capacity.ilog2();
            let floor = 1usize << octave;
            2 + 4 * (octave - (2 * MIN_CAPACITY).ilog2()) as usize + (capacity - floor) / (floor / 4)
        };
        assert!(bucket < BUCKETS, "property arena bucket overflow");
        bucket
    }

    /// The size class `bucket` holds: the inverse of `bucket`, so a released run's length
    /// follows from its list alone.
    #[inline] pub(super) fn bucket_capacity(bucket: usize) -> usize {
        match bucket {
            0 => MIN_CAPACITY,
            1 => MIN_CAPACITY + MIN_CAPACITY / 2,
            _ => {
                let octave = 3 + (bucket - 2) / 4;
                let floor = 1usize << octave;
                floor + (bucket - 2) % 4 * (floor / 4)
            }
        }
    }
}
