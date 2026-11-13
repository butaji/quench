//! The young cell slots, a bit per slot as in `Heap::marks`, so a minor sweep decides the
//! fate of 64 slots with a few word operations (dead, promoted, aged) and hands the dead
//! slots to the free list as one word, instead of visiting each young slot through a list.

use super::pacing::{AgedFates, PromotionAge};
use crate::bitset::{WORD_BITS, word_members};
use crate::value::Value;
use std::sync::atomic::{AtomicU64, Ordering};


#[derive(Default)]
pub(super) struct YoungSlots {
    words: Vec<u64>,
}

/// What a minor sweep of the young slots found.
#[derive(Default)]
pub(super) struct YoungSweep {
    pub(super) swept: usize,
    pub(super) promoted: usize,
    pub(super) stays_young: usize,
    pub(super) aged_fates: AgedFates,
}

/// The fates of one word of young slots: `dead` slots are freed, `stays` are aged (unmarked,
/// still young) and `promoted` become old. `marked` survivors that were `aged` already, and
/// the aged ones that died, are the aged fates.
struct Fates {
    dead: u64,
    stays: u64,
    promoted: u64,
    aged_marked: u64,
    aged_dead: u64,
}

impl YoungSweep {
    fn add(&mut self, young: u64, fates: &Fates) {
        let count = |bits: u64| bits.count_ones() as usize;
        self.swept += count(young);
        self.promoted += count(fates.promoted);
        self.stays_young += count(fates.stays);
        self.aged_fates.promoted += count(fates.aged_marked);
        self.aged_fates.died += count(fates.aged_dead);
    }
}

impl Fates {
    /// `ages`: whether a first-time survivor stays young (`PromotionAge::ages`). An aged
    /// survivor is promoted, and an aged slot is young.
    fn of(young: u64, marked: u64, aged: u64, ages: bool) -> Self {
        let survivors = young & marked;
        let stays = if ages { survivors & !aged } else { 0 };
        Self { dead: young & !marked, stays, promoted: survivors & !stays, aged_marked: aged & marked, aged_dead: aged & !marked }
    }
}

impl YoungSlots {
    #[inline(always)]
    pub(super) fn insert(&mut self, index: usize) {
        let word = index / WORD_BITS;
        if word >= self.words.len() {
            self.words.resize(word + 1, 0);
        }
        self.words[word] |= 1_u64 << (index % WORD_BITS);
    }

    pub(super) fn clear(&mut self) {
        self.words.fill(0);
    }

    #[cfg(any(test, feature = "profile-aggregate"))]
    pub(super) fn count(&self) -> usize {
        self.words.iter().map(|word| word.count_ones() as usize).sum()
    }

    #[cfg(any(test, debug_assertions))]
    pub(super) fn indices(&self) -> impl Iterator<Item = usize> + '_ {
        self.words.iter().enumerate().flat_map(|(word, &bits)| word_members(word, bits))
    }

    /// Sweeps the young slots by `marks`: `release(index)` frees a dead slot's cell and says
    /// whether it had one, and `free(word, bits)` lists the released slots of one word of
    /// slots as free. A survivor `age` ages stays young with its mark cleared (`aged` keeps
    /// it); one promoted is left marked and, when `age` ages, listed in `promoted_cells`.
    pub(super) fn sweep(
        &mut self,
        marks: &[AtomicU64],
        aged: &mut Vec<u64>,
        age: PromotionAge,
        promoted_cells: &mut Vec<Value>,
        mut release: impl FnMut(usize) -> bool,
        mut free: impl FnMut(usize, u64),
    ) -> YoungSweep {
        let mut sweep = YoungSweep::default();
        for (word, young) in self.words.iter_mut().enumerate().filter(|(_, young)| **young != 0) {
            let was_aged = aged.get(word).copied().unwrap_or(0);
            let fates = Fates::of(*young, marks[word].load(Ordering::Relaxed), was_aged & *young, age.ages());
            sweep.add(*young, &fates);
            if age.ages() {
                promoted_cells.extend(word_members(word, fates.promoted).map(|index| Value::heap(index as u32)));
            }
            let now_aged = (was_aged & !*young) | fates.stays;
            if now_aged != was_aged {
                aged.resize(aged.len().max(marks.len()), 0);
                aged[word] = now_aged;
            }
            if fates.stays != 0 {
                marks[word].fetch_and(!fates.stays, Ordering::Relaxed);
            }
            *young = fates.stays;
            let freed = word_members(word, fates.dead).filter(|&index| release(index)).fold(0, |bits, index| bits | 1_u64 << (index % WORD_BITS));
            if freed != 0 {
                free(word, freed);
            }
        }
        sweep
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fates_split_young_slots_by_mark_and_age() {
        let (young, marked, aged) = (0b1111_1100, 0b0101_0101, 0b0001_0100);
        let first = Fates::of(young, marked, aged & young, false);
        assert_eq!((first.dead, first.stays, first.promoted), (0b1010_1000, 0, 0b0101_0100));
        let second = Fates::of(young, marked, aged & young, true);
        assert_eq!((second.dead, second.stays, second.promoted), (0b1010_1000, 0b0100_0000, 0b0001_0100));
        assert_eq!((second.aged_marked, second.aged_dead), (0b0001_0100, 0));
    }

    #[test]
    fn sweep_frees_dead_ages_first_survivors_and_promotes_aged_ones() {
        let marks = [AtomicU64::new(0b0110), AtomicU64::new(1)];
        let mut young = YoungSlots::default();
        [1, 2, 3, 5, 64, 65].into_iter().for_each(|index| young.insert(index));
        let (mut aged, mut promoted, mut freed) = (vec![0b0100], Vec::new(), Vec::new());
        let sweep = young.sweep(&marks, &mut aged, PromotionAge::Second, &mut promoted, |index| index != 3, |word, bits| freed.push((word, bits)));
        assert_eq!(freed, vec![(0, 0b10_0000), (1, 0b10)]);
        assert_eq!(promoted, vec![Value::heap(2)]);
        assert_eq!((sweep.swept, sweep.promoted, sweep.stays_young), (6, 1, 2));
        assert_eq!(young.indices().collect::<Vec<_>>(), vec![1, 64]);
        assert_eq!(aged, vec![0b0010, 0b1]);
        assert_eq!((marks[0].load(Ordering::Relaxed), marks[1].load(Ordering::Relaxed)), (0b0100, 0));
        assert_eq!((sweep.aged_fates.promoted, sweep.aged_fates.died), (1, 0));
    }
}
