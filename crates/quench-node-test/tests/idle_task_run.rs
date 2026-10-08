//! Idle-task run is a slot-word combinator (S|C), not a Richards clone.
//! Drives the same source as tests/lanes/idle-task-run.js through the host.

use quench_node::shared_run::{execute_shared, SharedCompletion, SharedInput};

#[test]
fn idle_task_run_completes() {
    let source = include_str!("../../../tests/lanes/idle-task-run.js");
    let outcome = execute_shared(
        SharedInput::Eval(source.to_owned()),
        vec!["quench-node".into()],
    );
    assert_eq!(outcome, Ok(SharedCompletion::Completed));
}
