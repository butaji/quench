//! Runs both miners over every block of the corpus.
use crate::blocks;
use crate::dataflow::{Exposure, analyze};
use crate::model::{Candidate, Mined, Shape};
use crate::ngram::{self, BlockView};
use crate::trees;
use crate::weights::Weights;
use quench_runtime::FlowFunction;
use std::collections::HashMap;

pub fn mine_corpus(corpus: &[FlowFunction], weights: &Weights) -> Vec<Candidate> {
    let mut mined = Mined::new();
    for (function_id, function) in corpus.iter().enumerate() {
        let blocks = blocks::split(function);
        let exposure = Exposure::of(&function.instructions, &blocks);
        for (index, range) in blocks.iter().enumerate() {
            let instructions = &function.instructions[range.clone()];
            let flow = analyze(instructions, |register| exposure.escapes(index, register));
            let view = BlockView { function: function_id as u32, start: range.start, instructions, links_in: &flow.links_in };
            ngram::mine(&view, weights, &mut mined);
            trees::mine(&view, &flow, weights, &mut mined);
        }
    }
    let mut candidates: Vec<Candidate> =
        mined.into_iter().map(|((shape, pattern), sites)| Candidate { shape, pattern, sites }).collect();
    candidates.sort_by(|a, b| (a.shape, &a.pattern).cmp(&(b.shape, &b.pattern)));
    candidates
}

/// Static sites per opcode sequence, across dataflow variants.
pub fn sequence_site_counts(candidates: &[Candidate]) -> HashMap<Vec<u16>, usize> {
    let mut counts = HashMap::new();
    for candidate in candidates.iter().filter(|candidate| candidate.shape == Shape::Sequence) {
        *counts.entry(candidate.pattern.opcodes()).or_insert(0) += candidate.sites.len();
    }
    counts
}

/// Estimated executions of a candidate: the sum of its sites' weights; for a sequence with an
/// exact trace count, that count shared among the static sites of the same opcode sequence
/// in proportion to their number, never above the rarest member opcode.
pub fn dynamic_frequency(candidate: &Candidate, weights: &Weights, sequence_sites: &HashMap<Vec<u16>, usize>) -> f64 {
    let estimate: f64 = candidate.sites.iter().map(|site| site.weight).sum();
    let (Some(trace), Shape::Sequence) = (&weights.trace, candidate.shape) else {
        return estimate;
    };
    let opcodes = candidate.pattern.opcodes();
    let share = candidate.sites.len() as f64 / sequence_sites[&opcodes] as f64;
    let rarest = opcodes.iter().map(|op| weights.dynamic.get(*op)).fold(f64::INFINITY, f64::min);
    (trace.gram(&opcodes) as f64 * share).min(rarest)
}
