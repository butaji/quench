//! JetStream sub-scores (First, Worst, Average) and the board's per-test kind.
//!
//! A kind compares quench's sub-scores with the best peer's on the same test:
//! a test is `cold` when quench pays for its first iteration, `degrading` when
//! its worst iteration falls further behind its first, `no-reuse` when its
//! average falls further behind its first, and `steady` otherwise.

use crate::jetstream::SubScores;
use serde::Serialize;

/// Quench's First/Average ratio must reach this fraction of the peer's ratio;
/// below it the test is `cold`.
pub(crate) const COLD_FIRST_AVERAGE_RATIO: f64 = 0.85;

/// How quench's sub-scores compare with the best peer's on one test.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum SubScoreKind {
    Cold,
    Degrading,
    NoReuse,
    Steady,
}

/// A fixture's kind, with the peer whose sub-scores it was computed against.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct PeerKind {
    pub(crate) peer: String,
    pub(crate) kind: SubScoreKind,
}

impl SubScores {
    fn first_average(self) -> f64 {
        self.first / self.average
    }

    fn worst_first(self) -> f64 {
        self.worst / self.first
    }

    fn average_first(self) -> f64 {
        self.average / self.first
    }
}

/// Classifies `quench` against `peer`, checking cold, then degrading, then no-reuse.
pub(crate) fn classify(quench: SubScores, peer: SubScores) -> SubScoreKind {
    if quench.first_average() < COLD_FIRST_AVERAGE_RATIO * peer.first_average() {
        SubScoreKind::Cold
    } else if quench.worst_first() < peer.worst_first() {
        SubScoreKind::Degrading
    } else if quench.average_first() < peer.average_first() {
        SubScoreKind::NoReuse
    } else {
        SubScoreKind::Steady
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PEER: SubScores = SubScores {
        first: 100.0,
        worst: 50.0,
        average: 200.0,
    };

    fn quench(first: f64, worst: f64, average: f64) -> SubScores {
        SubScores {
            first,
            worst,
            average,
        }
    }

    #[test]
    fn matching_sub_scores_are_steady() {
        assert_eq!(classify(PEER, PEER), SubScoreKind::Steady);
    }

    #[test]
    fn first_average_below_the_threshold_is_cold() {
        // The peer's First/Average is 0.5, so the cold boundary is 0.5 * 0.85 = 0.425.
        let just_above = quench(90.0, 45.0, 200.0);
        assert_eq!(classify(just_above, PEER), SubScoreKind::Steady);
        let colder = quench(70.0, 35.0, 200.0);
        assert_eq!(classify(colder, PEER), SubScoreKind::Cold);
    }

    #[test]
    fn cold_is_checked_before_degrading_and_no_reuse() {
        let everything_worse = quench(50.0, 10.0, 400.0);
        assert_eq!(classify(everything_worse, PEER), SubScoreKind::Cold);
    }

    #[test]
    fn worst_below_the_peers_worst_over_first_is_degrading() {
        let degrading = quench(100.0, 40.0, 200.0);
        assert_eq!(classify(degrading, PEER), SubScoreKind::Degrading);
    }

    #[test]
    fn average_below_the_peers_average_over_first_is_no_reuse() {
        let no_reuse = quench(100.0, 50.0, 150.0);
        assert_eq!(classify(no_reuse, PEER), SubScoreKind::NoReuse);
    }

    #[test]
    fn kinds_serialize_as_board_labels() {
        let labels: Vec<String> = [
            SubScoreKind::Cold,
            SubScoreKind::Degrading,
            SubScoreKind::NoReuse,
            SubScoreKind::Steady,
        ]
        .iter()
        .map(|kind| serde_json::to_string(kind).expect("kind serializes"))
        .collect();
        assert_eq!(
            labels,
            ["\"cold\"", "\"degrading\"", "\"no-reuse\"", "\"steady\""]
        );
    }

    #[test]
    fn sub_scores_must_be_finite_and_positive() {
        assert!(PEER.is_measured());
        assert!(!quench(0.0, 1.0, 1.0).is_measured());
        assert!(!quench(1.0, f64::NAN, 1.0).is_measured());
    }
}
