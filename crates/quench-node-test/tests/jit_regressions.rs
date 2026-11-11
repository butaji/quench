//! Guest programs the native tiers once ran differently from the interpreter, compared with Node.

mod support;

use quench_node_test::case_process::RunResult;
use support::{observe_node_source, observe_source};

/// A hot method calling another method of `this` from a nested loop keeps its receiver and
/// callee across the loop's native region (ARES-6 ML's Matrix methods): the region's call
/// boundary once resumed with the method read stale, `this.get is not a function`.
#[test]
fn method_call_in_a_nested_hot_loop_keeps_its_receiver_like_node() {
    let source = r#"
function M(r) { for (var i = 0; i < r; i++) this[i] = [1, 1]; this.rows = r; }
M.prototype.get = function (i, j) { return this[i][j]; };
M.prototype.sum = function () {
  var total = 0;
  for (var i = 0; i < this.rows; i++) for (var j = 0; j < 2; j++) total += this.get(i, j);
  return total;
};
var total = 0;
for (var k = 0; k < 3000; k++) total += new M(4).sum() + new M(1).sum();
console.log(total);
"#;
    let node = observe_node_source(source);
    assert!(node.status.success(), "Node oracle failed: {node:?}");
    let quench = observe_source(source);
    assert_eq!(quench.outcome(), RunResult::Pass, "{:?}", quench.worker);
    assert_eq!(quench.stdout, node.stdout);
}
