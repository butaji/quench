use super::{AgedFates, AGED_DEATH_TO_KEEP_AGING, FULLS_FINDING_PROMOTED_GARBAGE_TO_AGE, SURVIVAL_SCALE};

/// Whether minors promote at `PromotionAge::OLDEST`: turned on by promotions dying old, off by
/// aged survivors outliving their second minor collection. Every transition is in
/// `GenerationalPacing::plan`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Aging {
    /// No collection has been planned: minors age as the table's `ages_from_start` says.
    #[default]
    Unstarted,
    /// Minors promote at the table's age.
    Off {
        /// Consecutive full collections that found promotions dying old, which turn aging on
        /// at `FULLS_FINDING_PROMOTED_GARBAGE_TO_AGE`.
        promoted_garbage_fulls: u32,
        /// Full collections left before promotions dying old may turn aging on again, after
        /// aging was turned off for freeing too few of the survivors it aged.
        cooldown: u32,
    },
    On {
        /// Smoothed share, in `SURVIVAL_SCALE` parts, of aged survivors their second minor
        /// collection freed (`AGED_DEATH_TO_KEEP_AGING`); none until the first minor
        /// collection after aging turned on measures it.
        aged_death: Option<u32>,
    },
}

impl Aging {
    /// Whether minors promote at `PromotionAge::OLDEST` now.
    pub(super) fn oldest(self, ages_from_start: bool) -> bool {
        match self {
            Self::On { .. } => true,
            Self::Unstarted => ages_from_start,
            Self::Off { .. } => false,
        }
    }

    /// The first collection aged its survivors as the table says (`oldest`): its next minor
    /// measures their fates.
    pub(super) fn start(&mut self, ages_from_start: bool) {
        if *self == Self::Unstarted {
            *self = if ages_from_start {
                Self::On { aged_death: None }
            } else {
                Self::Off { promoted_garbage_fulls: 0, cooldown: 0 }
            };
        }
    }

    /// A full collection, which found promotions dying old or not. While on, aging is
    /// measured by its aged survivors' fates (`after_minor`).
    pub(super) fn after_full(&mut self, promoted_garbage: bool) {
        let Self::Off { promoted_garbage_fulls, cooldown } = self else {
            return;
        };
        if *cooldown > 0 {
            *cooldown -= 1;
        } else if !promoted_garbage {
            *promoted_garbage_fulls = 0;
        } else {
            *promoted_garbage_fulls += 1;
            if *promoted_garbage_fulls >= FULLS_FINDING_PROMOTED_GARBAGE_TO_AGE {
                // Aging must show it frees what it ages from its first measurement on.
                *self = Self::On { aged_death: None };
            }
        }
    }

    /// A minor collection that found the `aged` survivors' fates. The survivors aging keeps
    /// young mostly outlive their second minor collection: each is traced twice and promoted
    /// anyway, so aging turns off for `cooldown` full collections.
    pub(super) fn after_minor(&mut self, aged: AgedFates, cooldown: u32) {
        let Self::On { aged_death } = self else {
            return;
        };
        let total = aged.died + aged.promoted;
        if total == 0 {
            return;
        }
        let death = (aged.died as u64 * u64::from(SURVIVAL_SCALE) / total as u64) as u32;
        let smoothed = aged_death.map_or(death, |previous| (previous + death) / 2);
        *aged_death = Some(smoothed);
        if smoothed < AGED_DEATH_TO_KEEP_AGING {
            *self = Self::Off { promoted_garbage_fulls: 0, cooldown };
        }
    }
}
