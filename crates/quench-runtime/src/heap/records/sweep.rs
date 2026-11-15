use super::*;

pub(in crate::heap) enum RecordSweep<'a, T> {
    Keep(RecordId, &'a T),
    Dead(RecordId, T),
}

/// A sweep's two callbacks (`RecordSpace::sweep_disposing`): what frees a dead record in
/// place, and what it shows the records left to drop and its survivors.
struct Sweeper<'v, D, V> {
    dispose: D,
    visit: &'v mut V,
}

impl<T, const CAPACITY: usize, const HOLE: usize> RecordSpace<T, CAPACITY, HOLE> {
    /// Frees every occupied, unmarked record through `visit`. Marked records survive and keep
    /// their mark bit, which makes them old for the next minor collection; they reach `visit`
    /// only when the sweep asks for them (`visit_live`): every one in a full sweep, the young
    /// ones (newborn or aged, `prepare_minor`) in a minor one. A full sweep visits every word; a
    /// minor one only the words young records can occupy (`young_word_bases`) and the aged
    /// ones, so it pays for its allocation, not for the old records it keeps. Under `age`
    /// `PromotionAge::Second` a minor sweep ages its newborn survivors instead and promotes its
    /// aged ones (`sweep_aging_word`). Every minor sweep frees or promotes the aged records
    /// it finds, whatever its age, so the age may change between collections.
    pub(in crate::heap) fn sweep(
        &mut self,
        full: bool,
        age: PromotionAge,
        visit_live: bool,
        visit: impl FnMut(RecordSweep<'_, T>),
    ) -> (usize, usize) {
        self.sweep_disposing(full, age, visit_live, |_, _| true, visit)
    }

    /// `sweep`, which first hands each dead record in place to `dispose`, which frees what it
    /// owns and answers whether anything is left for its drop: a record it answers `false`
    /// for is forgotten, neither moved out of the space nor dropped, nor shown to `visit`.
    pub(in crate::heap) fn sweep_disposing(
        &mut self,
        full: bool,
        age: PromotionAge,
        visit_live: bool,
        dispose: impl FnMut(RecordId, &T) -> bool,
        mut visit: impl FnMut(RecordSweep<'_, T>),
    ) -> (usize, usize) {
        let mut visit = Sweeper { dispose, visit: &mut visit };
        let (live_count, freed_count) = if full {
            self.sweep_full(visit_live, &mut visit)
        } else if age.ages() {
            self.sweep_minor_aging(visit_live, &mut visit)
        } else {
            self.sweep_minor_aged_here(visit_live, &mut visit)
        };
        self.live = live_count;
        self.young_floor = self.next_id;
        self.young_words.clear();
        self.free_cursor = if live_count < self.next_id as usize {
            self.next_id
        } else {
            0
        };
        self.free_word = 0;
        #[cfg(debug_assertions)]
        self.debug_assert_swept();
        (live_count, freed_count)
    }

    /// A full sweep: every occupancy word, freeing its occupied unmarked records. Returns the
    /// live and freed counts.
    fn sweep_full(&mut self, visit_live: bool, visit: &mut Sweeper<impl FnMut(RecordId, &T) -> bool, impl FnMut(RecordSweep<'_, T>)>) -> (usize, usize) {
        debug_assert!(self.aged_words.is_empty(), "a full collection forgets ages first");
        let (mut live_count, mut freed_count) = (0, 0);
        for block_index in 0..self.blocks.len() {
            for word_index in 0..BLOCK_WORDS {
                self.warm_ahead(block_index, word_index);
                let (live, freed) = self.sweep_word(block_index, word_index, if visit_live { u64::MAX } else { 0 }, visit);
                (live_count, freed_count) = (live_count + live, freed_count + freed);
            }
        }
        (live_count, freed_count)
    }

    /// A minor sweep under `PromotionAge::Second`: the words `prepare_minor` listed, whose
    /// newborn and aged records age or die (`sweep_aging_word`). Returns the live and freed
    /// counts.
    fn sweep_minor_aging(&mut self, visit_live: bool, visit: &mut Sweeper<impl FnMut(RecordId, &T) -> bool, impl FnMut(RecordSweep<'_, T>)>) -> (usize, usize) {
        let words = std::mem::take(&mut self.minor_words);
        let mut freed_count = 0;
        for &(base, newborn) in &words {
            freed_count += self.sweep_aging_word(base, newborn, visit_live, visit);
        }
        self.minor_words = words;
        self.minor_words.clear();
        (self.live - freed_count, freed_count)
    }

    /// A minor sweep under `PromotionAge::First`. Newborn records are aged here
    /// (`prepare_minor`): the young survivors are the marked aged ones, and the aged records
    /// the sweep kept are marked, so old. Returns the live and freed counts.
    fn sweep_minor_aged_here(&mut self, visit_live: bool, visit: &mut Sweeper<impl FnMut(RecordId, &T) -> bool, impl FnMut(RecordSweep<'_, T>)>) -> (usize, usize) {
        let young_words = std::mem::take(&mut self.young_words);
        let mut aged_words = std::mem::take(&mut self.aged_words);
        let mut freed_count = 0;
        for base in Self::young_word_bases(&young_words, self.young_floor, self.next_id)
            .chain(aged_words.drain(..))
        {
            let (block_index, word_index) = Self::word_position(base);
            let young = if visit_live { self.blocks[block_index].aged[word_index] } else { 0 };
            freed_count += self.sweep_word(block_index, word_index, young, visit).1;
            self.blocks[block_index].aged[word_index] = 0;
        }
        self.young_words = young_words;
        self.aged_words = aged_words;
        (self.live - freed_count, freed_count)
    }

    /// Sweeps one occupancy word: frees its occupied, unmarked records and shows `visit` its
    /// marked ones among `shown`. Returns the live and freed counts it saw.
    #[inline(always)]
    fn sweep_word(
        &mut self,
        block_index: usize,
        word_index: usize,
        shown: u64,
        visit: &mut Sweeper<impl FnMut(RecordId, &T) -> bool, impl FnMut(RecordSweep<'_, T>)>,
    ) -> (usize, usize) {
        let block = &mut self.blocks[block_index];
        let occupied = block.occupied[word_index];
        let marked = block.marked[word_index];
        debug_assert_eq!(marked & !occupied, 0);
        let dead = occupied & !marked;
        let id_base = (block_index * RECORDS_PER_BLOCK + word_index * OCCUPANCY_WORD_BITS) as RecordId;
        let mut live = marked & shown;
        while live != 0 {
            let id = id_base + live.trailing_zeros();
            // SAFETY: a mark bit is set only for an occupied record.
            (visit.visit)(RecordSweep::Keep(id, unsafe {
                (*self.base.as_ptr().add(id as usize)).assume_init_ref()
            }));
            live &= live - 1;
        }
        block.occupied[word_index] = marked;
        if self.rc {
            self.rc_keep(block_index, word_index, marked);
        }
        (marked.count_ones() as usize, self.release(id_base, dead, visit))
    }

    /// Sweeps one word a minor collection under `PromotionAge::Second` visits, whose `newborn`
    /// records `prepare_minor` named: the newborn and aged records the mark left unmarked die,
    /// the marked aged ones are promoted (kept marked, and listed in `promoted`), and the
    /// marked newborn ones stay young, aged and unmarked. Old records are left as they are.
    /// Not idempotent: `prepare_minor` lists each word once. Returns the freed count. Out of
    /// line, so the sweep's visitor keeps the two call sites of `sweep_word` it is inlined at.
    #[inline(never)]
    fn sweep_aging_word(
        &mut self,
        base: RecordId,
        newborn: u64,
        visit_live: bool,
        visit: &mut Sweeper<impl FnMut(RecordId, &T) -> bool, impl FnMut(RecordSweep<'_, T>)>,
    ) -> usize {
        let (block_index, word_index) = Self::word_position(base);
        let block = &mut self.blocks[block_index];
        let marked = block.marked[word_index];
        let aged = block.aged[word_index];
        debug_assert_eq!(newborn & aged, 0);
        let dead = (newborn | aged) & !marked;
        let promoted = aged & marked;
        self.aged_fates.add(AgedFates {
            died: (aged & !marked).count_ones() as usize,
            promoted: promoted.count_ones() as usize,
        });
        let stays = newborn & marked;
        block.aged[word_index] = stays;
        block.marked[word_index] = marked & !stays;
        block.occupied[word_index] &= !dead;
        if self.rc {
            self.rc_keep(block_index, word_index, !dead);
        }
        if stays != 0 {
            self.aged_words.push(base);
        }
        self.list_promotions(base, promoted);
        self.show_kept(base, if visit_live { stays | promoted } else { 0 }, visit);
        self.release(base, dead, visit)
    }

    /// Lists the `promoted` records of the word at `base` (`promoted`).
    fn list_promotions(&mut self, base: RecordId, mut promoted: u64) {
        while promoted != 0 {
            self.promoted.push(base + promoted.trailing_zeros());
            promoted &= promoted - 1;
        }
    }

    /// Shows the `kept` records of the word at `base`, which are marked or aged, so occupied.
    fn show_kept(&self, base: RecordId, mut kept: u64, visit: &mut Sweeper<impl FnMut(RecordId, &T) -> bool, impl FnMut(RecordSweep<'_, T>)>) {
        while kept != 0 {
            let id = base + kept.trailing_zeros();
            // SAFETY: a kept record is marked or aged, so occupied.
            (visit.visit)(RecordSweep::Keep(id, unsafe { (*self.base.as_ptr().add(id as usize)).assume_init_ref() }));
            kept &= kept - 1;
        }
    }

    /// Hands `visit` the `dead` records of the word starting at `id_base`, whose occupancy
    /// bits the caller cleared, once its `dispose` leaves them something to drop; the others
    /// are forgotten. Returns how many there were.
    fn release(
        &mut self,
        id_base: RecordId,
        mut dead: u64,
        visit: &mut Sweeper<impl FnMut(RecordId, &T) -> bool, impl FnMut(RecordSweep<'_, T>)>,
    ) -> usize {
        let freed = dead.count_ones() as usize;
        while dead != 0 {
            let id = id_base + dead.trailing_zeros();
            // SAFETY: the occupied bit proved initialization, and clearing it transferred the
            // record to the sweep, which forgets one disposed of and moves any other out to
            // the drop callback.
            let record = unsafe { &mut *self.base.as_ptr().add(id as usize) };
            if (visit.dispose)(id, unsafe { record.assume_init_ref() }) {
                (visit.visit)(RecordSweep::Dead(id, unsafe { record.assume_init_read() }));
            }
            dead &= dead - 1;
        }
        freed
    }
}
