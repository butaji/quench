use super::*;

/// Reference counts (`super::rc`).
impl<T, const CAPACITY: usize, const HOLE: usize> RecordSpace<T, CAPACITY, HOLE> {
    /// Turns counting on or off; off forgets the epoch's buffers.
    pub(in crate::heap) fn set_rc(&mut self, on: bool) {
        self.rc = on;
        self.rc_decrements.clear();
        self.rc_logged.clear();
        if on {
            self.rc_ensure();
        } else {
            self.rc_blocks = Vec::new();
        }
    }

    /// Gives every record block its counts, zero for the blocks made since the last call: the
    /// collector calls it before it reads or changes counts.
    pub(in crate::heap) fn rc_ensure(&mut self) {
        while self.rc_blocks.len() < self.blocks.len() {
            self.rc_blocks.push(RcBlock::new());
        }
    }

    /// The count of live record `id`: 0..=3, 3 stuck.
    #[inline(always)]
    pub(in crate::heap) fn rc_count(&self, id: RecordId) -> u8 {
        let (block_index, record_index) = Self::split_id(id);
        let block = &self.rc_blocks[block_index];
        let (word, bit) = (record_index / OCCUPANCY_WORD_BITS, record_index % OCCUPANCY_WORD_BITS);
        let lo = (block.lo[word].load(Ordering::Relaxed) >> bit) & 1;
        let hi = (block.hi[word].load(Ordering::Relaxed) >> bit) & 1;
        (hi << 1 | lo) as u8
    }

    /// The exact count of `id`, with its overflow entry; `u32::MAX` when stuck.
    #[cfg(test)]
    pub(in crate::heap) fn rc_exact_count(&self, id: RecordId) -> u32 {
        if self.rc_stuck(id) {
            return u32::MAX;
        }
        let count = self.rc_count(id) as u32;
        let overflow = self.rc_overflow.lock().unwrap_or_else(|poison| poison.into_inner());
        count + if count == RC_STUCK as u32 { overflow.get(&id).copied().unwrap_or(0) } else { 0 }
    }

    /// Adds a reference to live record `id`; past `RC_STUCK`, in the overflow table. The
    /// count's low bit is set first: 0 -> 1 and 2 -> 3 set it, 1 -> 2 carries into the high bit.
    #[inline(always)]
    pub(in crate::heap) fn rc_increment(&self, id: RecordId) {
        let (block_index, record_index) = Self::split_id(id);
        let block = &self.rc_blocks[block_index];
        let (word, mask) = (record_index / OCCUPANCY_WORD_BITS, 1_u64 << (record_index % OCCUPANCY_WORD_BITS));
        let lo = block.lo[word].load(Ordering::Relaxed);
        if lo & mask == 0 {
            block.lo[word].store(lo | mask, Ordering::Relaxed);
            return;
        }
        let hi = block.hi[word].load(Ordering::Relaxed);
        if hi & mask == 0 {
            block.lo[word].store(lo & !mask, Ordering::Relaxed);
            block.hi[word].store(hi | mask, Ordering::Relaxed);
        } else if block.stuck[word].load(Ordering::Relaxed) & mask == 0 {
            self.rc_overflow_add(id);
        }
    }

    /// Makes `id`'s count stuck (`Heap::rc_pin`), dropping its overflow entry.
    pub(in crate::heap) fn rc_pin(&self, id: RecordId) {
        let (block_index, record_index) = Self::split_id(id);
        let block = &self.rc_blocks[block_index];
        let (word, mask) = (record_index / OCCUPANCY_WORD_BITS, 1_u64 << (record_index % OCCUPANCY_WORD_BITS));
        if block.stuck[word].load(Ordering::Relaxed) & mask != 0 {
            return;
        }
        if block.lo[word].load(Ordering::Relaxed) & block.hi[word].load(Ordering::Relaxed) & mask != 0 {
            self.rc_overflow.lock().unwrap_or_else(|poison| poison.into_inner()).remove(&id);
        }
        block.lo[word].fetch_or(mask, Ordering::Relaxed);
        block.hi[word].fetch_or(mask, Ordering::Relaxed);
        block.stuck[word].fetch_or(mask, Ordering::Relaxed);
    }

    /// Whether `id`'s count is stuck: past the overflow table's limit.
    #[cfg(test)]
    fn rc_stuck(&self, id: RecordId) -> bool {
        let (block_index, record_index) = Self::split_id(id);
        let word = &self.rc_blocks[block_index].stuck[record_index / OCCUPANCY_WORD_BITS];
        word.load(Ordering::Relaxed) & (1_u64 << (record_index % OCCUPANCY_WORD_BITS)) != 0
    }

    #[cold]
    #[inline(never)]
    fn rc_overflow_add(&self, id: RecordId) {
        let mut overflow = self.rc_overflow.lock().unwrap_or_else(|poison| poison.into_inner());
        let extra = overflow.entry(id).or_default();
        *extra += 1;
        if *extra > RC_OVERFLOW_LIMIT {
            overflow.remove(&id);
            let (block_index, record_index) = Self::split_id(id);
            let word = &self.rc_blocks[block_index].stuck[record_index / OCCUPANCY_WORD_BITS];
            word.fetch_or(1_u64 << (record_index % OCCUPANCY_WORD_BITS), Ordering::Relaxed);
        }
    }

    /// Drops a reference to live record `id`; whether its count reached 0. A count of 0 (an
    /// over-decrement, which a deletion the barrier missed cannot cause: those only
    /// over-count) stays 0.
    #[inline(always)]
    pub(in crate::heap) fn rc_decrement(&self, id: RecordId) -> bool {
        let (block_index, record_index) = Self::split_id(id);
        let block = &self.rc_blocks[block_index];
        let (word, mask) = (record_index / OCCUPANCY_WORD_BITS, 1_u64 << (record_index % OCCUPANCY_WORD_BITS));
        let lo = block.lo[word].load(Ordering::Relaxed);
        let hi = block.hi[word].load(Ordering::Relaxed);
        match (lo & mask != 0, hi & mask != 0) {
            (false, false) => false,
            (true, false) => {
                block.lo[word].store(lo & !mask, Ordering::Relaxed);
                true
            }
            (false, true) => {
                block.lo[word].store(lo | mask, Ordering::Relaxed);
                block.hi[word].store(hi & !mask, Ordering::Relaxed);
                false
            }
            (true, true) => {
                if block.stuck[word].load(Ordering::Relaxed) & mask == 0 && !self.rc_overflow_take(id) {
                    block.lo[word].store(lo & !mask, Ordering::Relaxed);
                }
                false
            }
        }
    }

    /// Takes one reference from `id`'s overflow entry; `false` when it has none.
    #[cold]
    #[inline(never)]
    fn rc_overflow_take(&self, id: RecordId) -> bool {
        let mut overflow = self.rc_overflow.lock().unwrap_or_else(|poison| poison.into_inner());
        match overflow.get_mut(&id) {
            Some(extra) => {
                *extra -= 1;
                if *extra == 0 {
                    overflow.remove(&id);
                }
                true
            }
            None => false,
        }
    }

    /// Makes old record `id` a candidate for freeing: its mark bit is cleared, so a minor
    /// collection's mark re-marks it if anything it traces names it, and its word goes to the
    /// minor sweep's list (`aged_words`), which frees it otherwise. `false` when it is not
    /// marked: logged by the barrier this epoch, young, or a candidate already.
    pub(in crate::heap) fn rc_unmark(&mut self, id: RecordId) -> bool {
        let (block_index, record_index) = Self::split_id(id);
        let (word_index, mask) = (record_index / OCCUPANCY_WORD_BITS, 1_u64 << (record_index % OCCUPANCY_WORD_BITS));
        let word = &mut self.blocks[block_index].marked[word_index];
        if *word & mask == 0 {
            return false;
        }
        *word &= !mask;
        // Aged, as a newborn is: if the mark re-marks it, it counts its children there. A
        // word aged already is listed already, among the young words or `aged_words`.
        let aged = &mut self.blocks[block_index].aged[word_index];
        if *aged == 0 {
            self.aged_words.push(id - (record_index % OCCUPANCY_WORD_BITS) as RecordId);
        }
        *aged |= mask;
        true
    }

    /// Clears the counts of the ids of a word that `kept` leaves out, as the sweep frees
    /// them, with their overflow entries.
    #[inline(never)]
    fn rc_keep(&mut self, block_index: usize, word_index: usize, kept: u64) {
        let Some(counts) = self.rc_blocks.get_mut(block_index) else { return };
        let mut overflowed = *counts.lo[word_index].get_mut() & *counts.hi[word_index].get_mut()
            & !*counts.stuck[word_index].get_mut() & !kept;
        counts.keep(word_index, kept);
        if overflowed != 0 {
            let overflow = self.rc_overflow.get_mut().unwrap_or_else(|poison| poison.into_inner());
            if !overflow.is_empty() {
                let base = (block_index * RECORDS_PER_BLOCK + word_index * OCCUPANCY_WORD_BITS) as RecordId;
                while overflowed != 0 {
                    overflow.remove(&(base + overflowed.trailing_zeros()));
                    overflowed &= overflowed - 1;
                }
            }
        }
    }

    /// Every count becomes 0, as a recount starts.
    pub(in crate::heap) fn rc_clear(&mut self) {
        self.rc_overflow.get_mut().unwrap_or_else(|poison| poison.into_inner()).clear();
        self.rc_ensure();
        for block in &mut self.rc_blocks {
            for word in 0..BLOCK_WORDS {
                block.keep(word, 0);
            }
        }
    }

    /// The records old as a full collection starts, one bit per id, `BLOCK_WORDS` words per
    /// block: the marked ones and the barrier's `logged` (old, though it unmarked them).
    pub(in crate::heap) fn rc_old_bits(&self, logged: &[RecordId]) -> Vec<u64> {
        let mut bits = Vec::with_capacity(self.blocks.len() * BLOCK_WORDS);
        for block in &self.blocks {
            bits.extend_from_slice(&block.marked);
        }
        for &id in logged {
            let (block_index, record_index) = Self::split_id(id);
            bits[block_index * BLOCK_WORDS + record_index / OCCUPANCY_WORD_BITS] |= 1 << (record_index % OCCUPANCY_WORD_BITS);
        }
        bits
    }

    /// After a full collection's mark, before its sweep: shows `visit` each record old at its
    /// start (`old`, from `rc_old_bits`) the mark left (`false`), and each record young then
    /// the mark kept (`true`).
    pub(in crate::heap) fn rc_full_changes(&self, old: &[u64], mut visit: impl FnMut(RecordId, bool)) {
        for (block_index, block) in self.blocks.iter().enumerate() {
            for word_index in 0..BLOCK_WORDS {
                let was = old.get(block_index * BLOCK_WORDS + word_index).copied().unwrap_or(0);
                let (occupied, marked) = (block.occupied[word_index], block.marked[word_index]);
                let base = (block_index * RECORDS_PER_BLOCK + word_index * OCCUPANCY_WORD_BITS) as RecordId;
                let mut changed = occupied & (was ^ marked);
                while changed != 0 {
                    let bit = changed.trailing_zeros();
                    visit(base + bit, marked & (1 << bit) != 0);
                    changed &= changed - 1;
                }
            }
        }
    }

    /// The children the barrier copied and the records it logged this epoch, emptied.
    pub(in crate::heap) fn take_rc_epoch(&mut self) -> (Vec<Value>, Vec<RecordId>) {
        self.rc_ensure();
        (std::mem::take(&mut self.rc_decrements), std::mem::take(&mut self.rc_logged))
    }

    /// Under `PromotionAge::First`, after a minor collection's mark and before its sweep: the
    /// young records the mark reached (the marked aged ones of the young words), which the
    /// sweep makes old.
    pub(in crate::heap) fn for_each_young_survivor(&self, mut visit: impl FnMut(RecordId)) {
        self.for_each_young_survivor_word(|base, _block_index, _word_index, mut survivors| {
            while survivors != 0 {
                visit(base + survivors.trailing_zeros());
                survivors &= survivors - 1;
            }
        });
    }

    /// `for_each_young_survivor` a word at a time: each word's first id, block and word
    /// position, and its survivors' bits.
    fn for_each_young_survivor_word(&self, mut visit: impl FnMut(RecordId, usize, usize, u64)) {
        // A word reloaded for allocation is listed once per load; a candidate's word is in
        // `aged_words`.
        let mut bases: Vec<RecordId> = Self::young_word_bases(&self.young_words, self.young_floor, self.next_id).collect();
        bases.extend_from_slice(&self.aged_words);
        bases.sort_unstable();
        bases.dedup();
        for base in bases {
            let (block_index, word_index) = Self::word_position(base);
            let block = &self.blocks[block_index];
            let survivors = block.aged[word_index] & block.marked[word_index];
            if survivors != 0 {
                visit(base, block_index, word_index, survivors);
            }
        }
    }

    /// The young survivors (`for_each_young_survivor`) whose count is 0, a word of counts at
    /// a time; the counts cover every block (`rc_ensure`).
    pub(in crate::heap) fn rc_zero_survivors(&self, mut visit: impl FnMut(RecordId)) {
        self.for_each_young_survivor_word(|base, block_index, word_index, survivors| {
            let counts = &self.rc_blocks[block_index];
            let mut zero = survivors
                & !(counts.lo[word_index].load(Ordering::Relaxed) | counts.hi[word_index].load(Ordering::Relaxed));
            while zero != 0 {
                visit(base + zero.trailing_zeros());
                zero &= zero - 1;
            }
        });
    }
}
