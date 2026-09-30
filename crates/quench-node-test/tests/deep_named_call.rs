//! Deep guest recursion must become a catchable RangeError before the worker
//! stack is exhausted, then leave the runtime usable for later calls.

use quench_node_test::{NodeOutcome, NodeTestRunner};

#[test]
fn deep_named_calls_throw_catchable_range_errors_on_the_shared_worker_stack() {
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
    let outcome = NodeTestRunner::new().run_source(source);
    assert!(
        matches!(outcome, NodeOutcome::Pass),
        "deep recursion policy failed: {outcome:?}"
    );
}
