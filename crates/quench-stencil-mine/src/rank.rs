//! Scores candidates: dynamic frequency, share of executed instructions covered, and the
//! dispatches a fused stencil saves (one per fused instruction but the first).
use crate::miner::{dynamic_frequency, sequence_site_counts};
use crate::model::{Candidate, Pattern, Shape};
use crate::variants::PlaceTable;
use crate::weights::Weights;

#[derive(Clone, Debug)]
pub struct Ranked {
    pub shape: Shape,
    pub pattern: Pattern,
    pub nodes: usize,
    pub static_sites: usize,
    pub dynamic: f64,
    pub covered_share: f64,
    pub dispatches_saved: f64,
    pub variants: u64,
}

/// One row per candidate, in candidate order.
pub fn rank(candidates: &[Candidate], weights: &Weights, places: &PlaceTable) -> Vec<Ranked> {
    let sequence_sites = sequence_site_counts(candidates);
    let mut ranked: Vec<Ranked> = candidates
        .iter()
        .map(|candidate| {
            let nodes = candidate.pattern.labels.len();
            let dynamic = dynamic_frequency(candidate, weights, &sequence_sites);
            Ranked {
                shape: candidate.shape,
                nodes,
                static_sites: candidate.sites.len(),
                dynamic,
                covered_share: dynamic * nodes as f64 / weights.total.max(1.0),
                dispatches_saved: dynamic * (nodes - 1) as f64,
                variants: places.variants(&candidate.pattern),
                pattern: candidate.pattern.clone(),
            }
        })
        .collect();
    ranked
}

/// Best first: most dispatches saved, ties by pattern.
pub fn best_first(ranked: &[Ranked]) -> Vec<Ranked> {
    let mut sorted = ranked.to_vec();
    sorted.sort_by(|a, b| b.dispatches_saved.total_cmp(&a.dispatches_saved).then_with(|| a.pattern.cmp(&b.pattern)));
    sorted
}
