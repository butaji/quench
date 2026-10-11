//! Complex guest regressions not owned by the upstream conformance suites.

use quench_node_test::case_process::{observe_case, CaseObservation, RunResult};
use std::{fs, path::Path, process::Command, time::Duration};

const SHARED_WORKER: &str = env!("CARGO_BIN_EXE_run-parallel");
const REGRESSION_TIMEOUT: Duration = Duration::from_secs(30);

fn observe_source(source: &str) -> CaseObservation {
    let directory = tempfile::tempdir().unwrap();
    let fixture = directory.path().join("regression.js");
    fs::write(&fixture, source).unwrap();
    observe_case(Path::new(SHARED_WORKER), &fixture, REGRESSION_TIMEOUT).unwrap()
}

fn observe_node_source(source: &str) -> std::process::Output {
    let directory = tempfile::tempdir().unwrap();
    let fixture = directory.path().join("oracle.js");
    fs::write(&fixture, source).unwrap();
    Command::new(std::env::var_os("NODE_BIN").unwrap_or_else(|| "node".into()))
        .arg(fixture)
        .output()
        .unwrap()
}

#[test]
fn strict_call_followed_by_return_of_another_register_matches_node() {
    let source = r#"
function g(a, b, c, x) { return arguments.length + 90; }
function p1(x) {
  "use strict";
  g(1, 2, 3, x);
  return x;
}
console.log(p1(7));
"#;

    let node = observe_node_source(source);
    assert!(node.status.success(), "Node oracle failed: {node:?}");
    let quench = observe_source(source);
    assert_eq!(
        quench.outcome(),
        RunResult::Pass,
        "Quench failed: {:?}",
        quench.worker
    );
    assert_eq!(quench.stdout, node.stdout, "Node and Quench output differ");
    assert_eq!(node.stdout, b"7\n");
}

#[test]
fn deep_named_calls_raise_catchable_range_error_and_recover() {
    let source = r#"
function Box() {}
Box.prototype.walk = function (n, acc) {
  if (n === 0) return acc;
  return this.walk(n - 1, acc + 1);
};
let caught = false;
try {
  new Box().walk(8000, 0);
} catch (error) {
  caught = error instanceof RangeError && error.message === "Maximum call stack size exceeded";
}
if (!caught) throw new Error("deep call did not throw the stack RangeError");
if (new Box().walk(2, 0) !== 2) throw new Error("runtime did not recover after deep call");
"#;
    let observation = observe_source(source);
    assert_eq!(
        observation.outcome(),
        RunResult::Pass,
        "deep recursion policy failed: {:?}",
        observation.worker
    );
}

#[test]
fn idle_task_combinator_finishes_on_the_shared_worker() {
    let observation = observe_source(include_str!("../../../tests/lanes/idle-task-run.js"));
    assert_eq!(
        observation.outcome(),
        RunResult::Pass,
        "idle task combinator failed: {:?}",
        observation.worker
    );
}
