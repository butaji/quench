//! A record space's claim as a staged row: the fresh-id fast path over the space's live
//! fields, and the loaded free word, free-cursor scan or block growth as the cold path.
//! `Concrete` runs it over `RecordSpace`; an `Emit` frame will run the same row over the
//! payload's pointers (`heap::RecordClaimFacts`) and call the allocator for the cold path.

use super::Stage;

/// The live record-claim fields a stage reads: a loaded free word, a free-cursor scan, the
/// next fresh id, the block array and count, and whether the space is reserved. In a native
/// frame these are loads through the payload's pointers; in `RecordSpace` they are its fields.
#[derive(Clone, Copy)]
pub(crate) struct ClaimWords<W> {
    pub(crate) free_word: W,
    pub(crate) free_cursor: W,
    pub(crate) next_id: W,
    pub(crate) block_count: W,
    pub(crate) reserved: W,
}

/// A record space a claim can act on: reads its live fields, tests-and-sets an occupancy bit,
/// and commits a claimed id (next id, live count) or runs the cold path.
pub(crate) trait ClaimFrame<S: Stage> {
    fn claim_words(&mut self, stage: &mut S) -> ClaimWords<S::Word>;
    /// Bits in one occupancy word, log2 of the records per block, and a block's byte size.
    fn claim_geometry(&self) -> (u32, u32, u64);
    /// Tests and sets `record`'s occupancy bit in `block`; `None` where it was already set
    /// (an id a sweep has not released), which leaves the claim.
    fn claim_block(&mut self, stage: &mut S, words: ClaimWords<S::Word>, block: S::Word,
        record: S::Word) -> Option<()>;
    /// Commits the fresh id `next_id`: the space's next id and live count advance; the id.
    fn commit_fresh(&mut self, stage: &mut S, next_id: S::Word) -> S::Word;
    /// The cold path's id: the loaded free word, the free-cursor scan or block growth, and
    /// the live count advanced.
    fn claim_cold(&mut self, stage: &mut S, words: ClaimWords<S::Word>, miss: S::Miss) -> Option<S::Word>;
}

/// Commands `frame`'s claim in `stage`: a fresh id from `claim_route`'s condition, else the
/// cold path. `capacity` is the space's record capacity.
pub(crate) fn claim<S: Stage, F: ClaimFrame<S>>(stage: &mut S, frame: &mut F, miss: S::Miss,
    capacity: u64) -> Option<S::Word> {
    let words = frame.claim_words(stage);
    let (_, block_log2, _) = frame.claim_geometry();
    let capacity = stage.constant(capacity);
    let free = stage.eq_imm(words.free_word, 0u64);
    let cursor = stage.eq_imm(words.free_cursor, 0u64);
    let reserved = stage.ne_imm(words.reserved, 0u64);
    let room = stage.ult(words.next_id, capacity);
    let block = stage.shr_imm(words.next_id, u64::from(block_log2));
    let has_block = stage.ult(block, words.block_count);
    // free && cursor && reserved && room && has_block
    let all = |stage: &mut S, a: S::Cond, b: S::Cond| {
        let (not_a, not_b) = (stage.not(a), stage.not(b));
        let joined = stage.any(not_a, not_b);
        stage.not(joined)
    };
    let fast = all(stage, free, cursor);
    let fast = all(stage, fast, reserved);
    let fast = all(stage, fast, room);
    let fast = all(stage, fast, has_block);
    let mask = stage.and_imm(words.next_id, (1_u64 << block_log2) - 1);
    // Both arms may be emitted (a stage that cannot know `fast`); the frame is shared through
    // a cell so one closure may borrow it at a time.
    let frame = std::cell::RefCell::new(frame);
    stage.choose_leaving(fast,
        |stage| {
            let mut frame = frame.borrow_mut();
            frame.claim_block(stage, words, block, mask)?;
            Some(frame.commit_fresh(stage, words.next_id))
        },
        |stage| frame.borrow_mut().claim_cold(stage, words, miss))
}
