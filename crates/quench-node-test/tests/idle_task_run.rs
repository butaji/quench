//! Idle-task run is a slot-word combinator (S|C), not a Richards clone.
//! Drives the same source as tests/lanes/idle-task-run.js through the host.

use quench_node_test::{NodeFixture, NodeOutcome, NodeRunner};

#[test]
fn idle_task_run_completes() {
    let source = include_str!("../../../tests/lanes/idle-task-run.js");
    let mut runner = NodeRunner::new();
    let outcome = runner.run(&NodeFixture::from_source(
        std::path::PathBuf::from("idle-task-run.js"),
        source.to_string(),
    ));
    assert!(
        matches!(outcome, NodeOutcome::Pass),
        "idle task combinator failed: {outcome:?}"
    );
}
