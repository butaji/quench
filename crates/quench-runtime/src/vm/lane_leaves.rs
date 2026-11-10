//! Where a specialized lane body's leaves went: its specialization's feedback.

/// The most distinct places a body's leaves may go for them to be its specialization's
/// feedback; past them they are scattered.
const LANE_LEAVE_SITES: usize = 4;

/// Where a specialized body's leaves went: at few exits, each once named, or scattered.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum LaneLeaves {
    Few([Option<super::super::compiled::LaneExit>; LANE_LEAVE_SITES]),
    Scattered,
}

impl Default for LaneLeaves {
    fn default() -> Self {
        Self::NONE
    }
}

impl LaneLeaves {
    pub(super) const NONE: Self = Self::Few([None; LANE_LEAVE_SITES]);

    pub(super) fn with(self, exit: super::super::compiled::LaneExit) -> Self {
        let Self::Few(mut exits) = self else { return self };
        if exits.contains(&Some(exit)) { return self; }
        match exits.iter_mut().find(|slot| slot.is_none()) {
            Some(slot) => { *slot = Some(exit); Self::Few(exits) }
            None => Self::Scattered,
        }
    }

    /// These and `other`'s exits together.
    pub(super) fn union(self, other: Self) -> Self {
        other.exits().fold(self, Self::with)
    }

    /// The exits these name: none where scattered.
    pub(super) fn exits(&self) -> impl Iterator<Item = super::super::compiled::LaneExit> + '_ {
        let exits: &[_] = match self { Self::Few(exits) => exits, Self::Scattered => &[] };
        exits.iter().flatten().copied()
    }

    /// The exits these name, listed.
    pub(super) fn listed(&self) -> Vec<super::super::compiled::LaneExit> {
        self.exits().collect()
    }
}

/// What a function's lane bodies so far learned, which the next body plans around: where their
/// runs left (`stuck`), which it plans generic, and the inlining budget halvings the last plan
/// needed to fit (`PendingLaneMethod::shrinks`).
#[derive(Clone, Copy, Default)]
pub(super) struct LaneFeedback {
    pub(super) stuck: LaneLeaves,
    pub(super) shrinks: u8,
}

/// The places the next lane body leaves generic, planned from `past`: where its predecessors'
/// runs left and, where the speculative bodies are spent, every record of the function of
/// `length` records, so the one permanent body speculates on nothing.
pub(super) fn generic_exits(past: Option<&super::super::tier_state::Past<LaneFeedback>>, length: usize) -> Vec<super::super::compiled::LaneExit> {
    use super::super::compiled::LaneExit;
    match past {
        Some(past) if past.is_spent() => (0..length).map(LaneExit::At).chain([LaneExit::Entry]).collect(),
        Some(past) => past.feedback.stuck.listed(),
        None => Vec::new(),
    }
}
