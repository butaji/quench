//! A function's settled rewrite entry (`RewriteRanges::settled`): where the sites its deferred
//! rewrites added start, or that they stayed unrun, in two words.

use super::replay::RewriteStart;

/// A function's entry in `RewriteRanges::settled`, in two words: a start, or one of two
/// sentinels in the field-site word, which no table reaches (its count stays below them).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Settled {
    field_sites: u32,
    superinstructions: u32,
}

impl Settled {
    /// Neither a replay nor a first read has settled the function's rewrites.
    pub(crate) const UNSETTLED: Self = Self { field_sites: u32::MAX, superinstructions: 0 };
    /// The rewrites stayed unrun.
    pub(crate) const UNRUN: Self = Self { field_sites: u32::MAX - 1, superinstructions: 0 };

    /// The entry of rewrites whose sites start at `start`.
    pub(crate) fn placed(start: RewriteStart) -> Self {
        assert!(start.field_sites < Self::UNRUN.field_sites, "a table reaches the settled sentinels");
        Self { field_sites: start.field_sites, superinstructions: start.superinstructions }
    }

    /// The settled state: `None` before settling, `Some(None)` when the rewrites stayed unrun,
    /// else where their sites start.
    pub(crate) fn get(self) -> Option<Option<RewriteStart>> {
        match self.field_sites {
            field_sites if field_sites == Self::UNSETTLED.field_sites => None,
            field_sites if field_sites == Self::UNRUN.field_sites => Some(None),
            field_sites => Some(Some(RewriteStart { field_sites, superinstructions: self.superinstructions })),
        }
    }
}
