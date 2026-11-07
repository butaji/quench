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

    /// Whether every exit these name is one of `other`'s: nothing new where they are few.
    pub(super) fn within(&self, other: &Self) -> bool {
        matches!(self, Self::Few(_)) && self.exits().all(|exit| other.exits().any(|known| known == exit))
    }

    /// The exits these name: none where scattered.
    pub(super) fn exits(&self) -> impl Iterator<Item = super::super::compiled::LaneExit> + '_ {
        let exits: &[_] = match self { Self::Few(exits) => exits, Self::Scattered => &[] };
        exits.iter().flatten().copied()
    }
}
