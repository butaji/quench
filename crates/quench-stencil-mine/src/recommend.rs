//! The recommended supernode set under a budget: greedy by marginal dispatches saved, where a
//! site already covered by an earlier pick (any shared pc) no longer counts.
use crate::model::Candidate;
use crate::rank::Ranked;
use std::collections::HashSet;

/// Most supernodes to recommend.
pub const MAX_SUPERNODES: usize = 32;
/// Most stencil variants (`variants::PlaceTable::variants`) the picks may add together.
pub const MAX_TOTAL_VARIANTS: u64 = 20_000;
/// Candidates, best by unshared savings, the greedy pass considers.
pub const POOL: usize = 1000;

pub struct Budget {
    pub supernodes: usize,
    pub variants: u64,
}

impl Default for Budget {
    fn default() -> Self {
        Self { supernodes: MAX_SUPERNODES, variants: MAX_TOTAL_VARIANTS }
    }
}

type Covered = HashSet<(u32, u32)>;

/// Executions of the sites no pick covers yet, scaled so the whole candidate is its
/// ranked frequency (an exact trace count replaces the census estimate of the sites).
fn uncovered_weight(candidate: &Candidate, covered: &Covered, ranked: &Ranked) -> f64 {
    let all: f64 = candidate.sites.iter().map(|site| site.weight).sum();
    let scale = if all > 0.0 { ranked.dynamic / all } else { 0.0 };
    scale * candidate
        .sites
        .iter()
        .filter(|site| !site.pcs.iter().any(|&pc| covered.contains(&(site.function, pc))))
        .map(|site| site.weight)
        .sum()
}

fn cover(candidate: &Candidate, covered: &mut Covered) {
    for site in &candidate.sites {
        if !site.pcs.iter().any(|&pc| covered.contains(&(site.function, pc))) {
            covered.extend(site.pcs.iter().map(|&pc| (site.function, pc)));
        }
    }
}

/// `ranked` and `candidates` are parallel (same order); returns the picks with their
/// marginal frequency, share and savings.
pub fn recommend(candidates: &[Candidate], ranked: &[Ranked], total: f64, budget: &Budget) -> Vec<Ranked> {
    let mut pool: Vec<usize> = (0..ranked.len()).collect();
    pool.sort_by(|&a, &b| ranked[b].dispatches_saved.total_cmp(&ranked[a].dispatches_saved));
    pool.truncate(POOL);
    let (mut covered, mut picks, mut spent) = (Covered::new(), Vec::new(), 0u64);
    while picks.len() < budget.supernodes {
        let best = pool
            .iter()
            .filter(|&&i| spent + ranked[i].variants <= budget.variants)
            .map(|&i| (i, uncovered_weight(&candidates[i], &covered, &ranked[i])))
            .filter(|&(_, weight)| weight > 0.0)
            .max_by(|a, b| (a.1 * (ranked[a.0].nodes - 1) as f64).total_cmp(&(b.1 * (ranked[b.0].nodes - 1) as f64)));
        let Some((index, weight)) = best else { break };
        let nodes = ranked[index].nodes;
        cover(&candidates[index], &mut covered);
        spent += ranked[index].variants;
        pool.retain(|&i| i != index);
        picks.push(Ranked {
            dynamic: weight,
            covered_share: weight * nodes as f64 / total.max(1.0),
            dispatches_saved: weight * (nodes - 1) as f64,
            ..ranked[index].clone()
        });
    }
    picks
}
