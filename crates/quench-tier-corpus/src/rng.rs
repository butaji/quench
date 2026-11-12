//! SplitMix64: a seed fixes the whole stream, so a program is a pure function of its seed.

const GOLDEN_GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;
const MIX_FIRST: u64 = 0xBF58_476D_1CE4_E5B9;
const MIX_SECOND: u64 = 0x94D0_49BB_1331_11EB;
const SHIFT_FIRST: u32 = 30;
const SHIFT_SECOND: u32 = 27;
const SHIFT_LAST: u32 = 31;

pub(crate) struct Rng(u64);

/// One SplitMix64 finalizer round over `state`.
pub(crate) fn mix(state: u64) -> u64 {
    let step = state.wrapping_add(GOLDEN_GAMMA);
    let step = (step ^ (step >> SHIFT_FIRST)).wrapping_mul(MIX_FIRST);
    let step = (step ^ (step >> SHIFT_SECOND)).wrapping_mul(MIX_SECOND);
    step ^ (step >> SHIFT_LAST)
}

impl Rng {
    pub(crate) fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub(crate) fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(GOLDEN_GAMMA);
        mix(self.0.wrapping_sub(GOLDEN_GAMMA))
    }

    /// Uniform in `low..=high`.
    pub(crate) fn range(&mut self, low: u64, high: u64) -> u64 {
        low + self.next() % (high - low + 1)
    }

    pub(crate) fn below(&mut self, bound: usize) -> usize {
        self.range(0, bound as u64 - 1) as usize
    }

    pub(crate) fn chance(&mut self) -> bool {
        self.next() & 1 == 1
    }
}
