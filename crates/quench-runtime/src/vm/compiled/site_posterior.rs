//! Whether a field site's statistics support speculating on it (`tier-decision-rule-v1`,
//! section 4). A site that fails reads in place through the VM's cache (`lane_region::field_probe`,
//! no exit) instead of guarding its shapes and leaving.

use super::tier_policy::{self, SPECULATION};

/// What a site's record holds, with no per-execution counter: its executions are bounded below
/// by the entries its function made before planning (`LaneBody::warmup_entries`), its leaves are
/// the byte its leave path writes (`Vm::record_field_leave`), and each shape its cache holds past
/// the first was a miss of the one before it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct SiteEvidence {
    pub(super) hits: u32,
    pub(super) misses: u32,
}

impl SiteEvidence {
    pub(super) fn of(leaves: u8, shapes: usize) -> Self {
        let shape_misses = u32::try_from(shapes.saturating_sub(1)).unwrap_or(u32::MAX);
        let misses = u32::from(leaves).saturating_add(shape_misses);
        let executions = u32::from(tier_policy::TIERS.lane_body.warmup_entries);
        Self { hits: executions.saturating_sub(misses), misses }
    }

    /// The Beta(1 + hits, 1 + misses) posterior's mean and standard deviation.
    fn posterior(self) -> (f64, f64) {
        let (a, b) = (f64::from(self.hits) + 1.0, f64::from(self.misses) + 1.0);
        let n = a + b;
        (a / n, (a * b / (n * n * (n + 1.0))).sqrt())
    }

    /// The posterior's lower 5% quantile of the rate at which the speculated fact holds.
    pub(super) fn lower_quantile(self) -> f64 {
        let (mean, deviation) = self.posterior();
        mean - SPECULATION.lower_quantile_z * deviation
    }

    /// Speculate (guard and exit) only where the lower quantile exceeds the break-even rate.
    pub(super) fn speculates(self) -> bool {
        self.lower_quantile() > break_even()
    }
}

/// The rate above which a guard and exit beat the in-place read (`tier_policy::Speculation`).
pub(super) fn break_even() -> f64 {
    let exit_over_probe = f64::from(SPECULATION.guard_exit_ns - SPECULATION.probe_read_ns);
    exit_over_probe / f64::from(SPECULATION.guard_exit_ns - SPECULATION.guard_read_ns)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evidence(hits: u32, misses: u32) -> SiteEvidence { SiteEvidence { hits, misses } }

    #[test]
    fn break_even_lies_between_certain_loss_and_certain_gain() {
        assert!(break_even() > 0.5 && break_even() < 1.0);
    }

    #[test]
    fn a_clean_warm_site_speculates() {
        assert!(evidence(400, 0).speculates());
    }

    #[test]
    fn a_site_with_no_evidence_does_not() {
        assert!(!evidence(0, 0).speculates());
        assert!(!evidence(10, 0).speculates());
    }

    #[test]
    fn more_misses_lower_the_quantile() {
        assert!(evidence(400, 1).lower_quantile() > evidence(400, 8).lower_quantile());
        assert!(!evidence(400, 12).speculates());
    }

    #[test]
    fn more_hits_raise_the_quantile() {
        assert!(evidence(4000, 8).lower_quantile() > evidence(400, 8).lower_quantile());
    }

    #[test]
    fn leaves_and_extra_shapes_count_as_misses() {
        assert_eq!(SiteEvidence::of(3, 1).misses, 3);
        assert_eq!(SiteEvidence::of(0, 3).misses, 2);
        assert!(!SiteEvidence::of(u8::MAX, 1).speculates());
    }
}
