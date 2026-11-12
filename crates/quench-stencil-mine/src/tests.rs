use crate::blocks::split;
use crate::inputs::compile_source;
use crate::miner::mine_corpus;
use crate::model::{Candidate, Shape};
use crate::rank::rank;
use crate::recommend::{Budget, recommend};
use crate::variants::PlaceTable;
use crate::weights::{TraceGrams, Weights, parse_census};
use quench_runtime::{FlowFunction, FlowInstruction};
use std::collections::HashMap;

fn op(opcode: u16, name: &'static str, operator: Option<&'static str>, defs: &[u16], uses: &[u16], successors: &[u32]) -> FlowInstruction {
    FlowInstruction { opcode, name, operator, defs: defs.to_vec(), uses: uses.to_vec(), successors: successors.to_vec() }
}

fn function(instructions: Vec<FlowInstruction>) -> FlowFunction {
    FlowFunction { name: None, instructions, handler_targets: Vec::new() }
}

const LOAD: u16 = 1;
const BINARY: u16 = 2;
const BRANCH: u16 = 3;
const RETURN: u16 = 4;

/// `a + b * c`: two loads, Multiply feeding Add, then return.
fn mul_add() -> FlowFunction {
    function(vec![
        op(LOAD, "LoadLocal", None, &[1], &[], &[1]),
        op(LOAD, "LoadLocal", None, &[2], &[], &[2]),
        op(BINARY, "Binary", Some("*"), &[3], &[1, 2], &[3]),
        op(BINARY, "Binary", Some("+"), &[4], &[3], &[4]),
        op(RETURN, "Return", None, &[], &[4], &[]),
    ])
}

fn find<'a>(candidates: &'a [Candidate], shape: Shape, operators: &[&str]) -> Option<&'a Candidate> {
    candidates.iter().find(|candidate| {
        let found: Vec<&str> = candidate.pattern.labels.iter().filter_map(|label| label.operator).collect();
        candidate.shape == shape && found == operators
    })
}

fn static_weights(corpus: &[FlowFunction]) -> Weights {
    Weights::build(corpus, None, None)
}

#[test]
fn multiply_feeding_add_is_a_tree() {
    let corpus = vec![mul_add()];
    let candidates = mine_corpus(&corpus, &static_weights(&corpus));
    let tree = find(&candidates, Shape::Tree, &["*", "+"]).expect("tree");
    assert_eq!(tree.pattern.labels.len(), 2);
    assert_eq!(tree.pattern.links.len(), 1);
    assert_eq!((tree.pattern.links[0].producer, tree.pattern.links[0].consumer), (0, 1));
    assert_eq!(tree.sites[0].pcs, vec![2, 3]);
}

#[test]
fn tree_reaches_through_nonadjacent_producers() {
    let corpus = vec![function(vec![
        op(BINARY, "Binary", Some("*"), &[3], &[], &[1]),
        op(LOAD, "LoadLocal", None, &[5], &[], &[2]),
        op(BINARY, "Binary", Some("+"), &[4], &[3, 5], &[3]),
        op(RETURN, "Return", None, &[], &[4], &[]),
    ])];
    let candidates = mine_corpus(&corpus, &static_weights(&corpus));
    let three = candidates.iter().find(|c| c.shape == Shape::Tree && c.pattern.labels.len() == 3).expect("3-node tree");
    assert_eq!(three.pattern.links.len(), 2);
}

#[test]
fn register_read_by_another_block_is_not_single_use() {
    let corpus = vec![function(vec![
        op(BINARY, "Binary", Some("*"), &[3], &[], &[1]),
        op(BINARY, "Binary", Some("+"), &[4], &[3], &[2]),
        op(BRANCH, "JumpFalse", None, &[], &[4], &[3, 4]),
        op(RETURN, "Return", None, &[], &[3], &[]),
        op(RETURN, "Return", None, &[], &[], &[]),
    ])];
    let candidates = mine_corpus(&corpus, &static_weights(&corpus));
    assert!(find(&candidates, Shape::Tree, &["*", "+"]).is_none());
    assert!(find(&candidates, Shape::Sequence, &["*", "+"]).is_some());
}

#[test]
fn compare_feeding_branch_is_a_linked_sequence() {
    let corpus = vec![function(vec![
        op(BINARY, "Binary", Some("<"), &[1], &[], &[1]),
        op(BRANCH, "JumpFalse", None, &[], &[1], &[2, 3]),
        op(RETURN, "Return", None, &[], &[], &[]),
        op(RETURN, "Return", None, &[], &[], &[]),
    ])];
    let candidates = mine_corpus(&corpus, &static_weights(&corpus));
    let pair = find(&candidates, Shape::Sequence, &["<"]).expect("compare+branch");
    assert_eq!(pair.pattern.labels[1].name, "JumpFalse");
    assert_eq!(pair.pattern.links.len(), 1);
}

#[test]
fn dataflow_shape_distinguishes_otherwise_equal_opcodes() {
    let linked = function(vec![
        op(BINARY, "Binary", Some("+"), &[1], &[], &[1]),
        op(BINARY, "Binary", Some("+"), &[2], &[1], &[2]),
        op(RETURN, "Return", None, &[], &[2], &[]),
    ]);
    let independent = function(vec![
        op(BINARY, "Binary", Some("+"), &[1], &[], &[1]),
        op(BINARY, "Binary", Some("+"), &[2], &[], &[2]),
        op(RETURN, "Return", None, &[], &[2], &[]),
    ]);
    let corpus = vec![linked, independent];
    let candidates = mine_corpus(&corpus, &static_weights(&corpus));
    let pairs: Vec<_> = candidates
        .iter()
        .filter(|c| c.shape == Shape::Sequence && c.pattern.labels.len() == 2 && c.pattern.labels[0].name == "Binary" && c.pattern.labels[1].name == "Binary")
        .collect();
    assert_eq!(pairs.len(), 2);
}

#[test]
fn blocks_split_at_targets_and_after_branches() {
    let blocks = split(&function(vec![
        op(LOAD, "LoadLocal", None, &[1], &[], &[1]),
        op(BRANCH, "JumpFalse", None, &[], &[1], &[2, 4]),
        op(LOAD, "LoadLocal", None, &[2], &[], &[3]),
        op(LOAD, "LoadLocal", None, &[3], &[], &[4]),
        op(RETURN, "Return", None, &[], &[], &[]),
    ]));
    assert_eq!(blocks, vec![0..2, 2..4, 4..5]);
}

#[test]
fn census_weights_scale_savings_and_shares() {
    let census = parse_census("noise\n{\"kind\":\"quench-dispatch-opcode-census-all\",\"counts\":{\"LoadLocal\":200,\"Binary\":300,\"Return\":100}}\n");
    assert_eq!(census["Binary"], 300);
    let corpus = vec![mul_add()];
    let weights = Weights::build(&corpus, Some(census), None);
    assert_eq!(weights.total, 600.0);
    let candidates = mine_corpus(&corpus, &weights);
    let ranked = rank(&candidates, &weights, &PlaceTable::new());
    let index = candidates.iter().position(|c| c.shape == Shape::Tree && c.pattern.labels.len() == 2 && c.pattern.labels[0].operator == Some("*")).unwrap();
    // Binary executes 300 times over two static sites: 150 each, so the pair runs 150 times.
    assert_eq!(ranked[index].dynamic, 150.0);
    assert_eq!(ranked[index].dispatches_saved, 150.0);
    assert_eq!(ranked[index].covered_share, 150.0 * 2.0 / 600.0);
}

#[test]
fn trace_gives_exact_ngram_counts() {
    let trace = TraceGrams::from_opcodes(&[1, 2, 1, 2, 1, 3]);
    assert_eq!(trace.gram(&[1, 2]), 2);
    assert_eq!(trace.gram(&[2, 1, 3]), 1);
    assert_eq!(trace.gram(&[3, 1]), 0);
    let bytes: Vec<u8> = [1u32, 2, 1].iter().flat_map(|op| op.to_le_bytes()).collect();
    assert_eq!(TraceGrams::from_bytes(&bytes).gram(&[1, 2]), 1);
}

#[test]
fn recommendation_does_not_count_covered_sites_twice() {
    let corpus = vec![mul_add()];
    let census = HashMap::from([("LoadLocal".to_string(), 100), ("Binary".to_string(), 100), ("Return".to_string(), 50)]);
    let weights = Weights::build(&corpus, Some(census), None);
    let candidates = mine_corpus(&corpus, &weights);
    let ranked = rank(&candidates, &weights, &PlaceTable::new());
    let picks = recommend(&candidates, &ranked, weights.total, &Budget { supernodes: 8, variants: u64::MAX });
    let covered: f64 = picks.iter().map(|pick| pick.covered_share * weights.total).sum();
    let instructions: f64 = corpus[0].instructions.len() as f64;
    // Each static instruction executes at most its opcode's per-site count; no instruction is covered twice.
    assert!(covered <= instructions * 50.0 + 1e-9, "covered {covered}");
    assert!(!picks.is_empty());
}

#[test]
fn variants_multiply_over_places_less_fused_intermediates() {
    let places = PlaceTable::new();
    assert!(places.library_rows > 0);
    let corpus = vec![mul_add()];
    let candidates = mine_corpus(&corpus, &static_weights(&corpus));
    let tree = find(&candidates, Shape::Tree, &["*", "+"]).unwrap();
    let alone = places.variants(&crate::model::Pattern { labels: vec![tree.pattern.labels[1]], links: Vec::new() });
    assert!(places.variants(&tree.pattern) > alone, "a fused pair is costlier than one row");
}

#[test]
fn compiled_javascript_yields_candidates() {
    let functions = compile_source("function f(a, b, c) { return a + b * c; } f(1, 2, 3);", "mine-test.js").expect("compiles");
    assert!(functions.iter().any(|function| !function.instructions.is_empty()));
    let weights = static_weights(&functions);
    let candidates = mine_corpus(&functions, &weights);
    assert!(!candidates.is_empty());
}
