//! Microtask and Promise job order, each fixture compared with Node.

mod support;

use quench_node_test::case_process::RunResult;
use support::{observe_node_source, observe_source};

fn assert_matches_node(source: &str) {
    let node = observe_node_source(source);
    assert!(node.status.success(), "Node oracle failed: {node:?}");
    let quench = observe_source(source);
    assert_eq!(quench.outcome(), RunResult::Pass, "Quench failed: {:?}", quench.worker);
    assert_eq!(quench.stdout, node.stdout, "Node and Quench output differ");
}

/// `queueMicrotask` takes Node's fast path (the callback is the microtask, with no
/// `AsyncResource`) while no store is visible and no hook is enabled, and its `AsyncResource`
/// path otherwise: execution ids, `this`, order with promise jobs, the validation error and
/// an `AsyncLocalStorage` store carried through the queue all match Node.
#[test]
fn queue_microtask_fast_path_and_resource_path_match_node() {
    assert_matches_node(include_str!("fixtures/queue_microtask.js"));
}

/// Promise combinators and capabilities (`all`, `race`, `allSettled`, `any`, `withResolvers`,
/// subclasses, patched `resolve`/`then`, a `resolve` getter, a poisoned `Array.prototype.then`)
/// settle in Node's job order, whether the intrinsic fast paths or the general protocol runs.
#[test]
fn promise_combinators_keep_job_order_like_node() {
    assert_matches_node(include_str!("fixtures/promise_combinators.js"));
}
