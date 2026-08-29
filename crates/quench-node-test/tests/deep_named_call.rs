//! Compact CallN must trampoline on the heap, not the Rust stack.
//! Earley-Boyer scheme recursion SIGTRAPs (~exit 133) without this.

use quench_node_test::{NodeFixture, NodeOutcome, NodeRunner};

#[test]
fn deep_named_calls_survive_default_rust_stack() {
    let source = r#"
function Box() {}
Box.prototype.walk = function (n, acc) {
  if (n === 0) return acc;
  return this.walk(n - 1, acc + 1);
};
if (new Box().walk(8000, 0) !== 8000) throw new Error("deep call result");
"#;
    let mut runner = NodeRunner::new();
    let outcome = runner.run(&NodeFixture::from_source(
        std::path::PathBuf::from("deep-named-call.js"),
        source.to_string(),
    ));
    assert!(
        matches!(outcome, NodeOutcome::Pass),
        "deep CallN trampoline failed: {outcome:?}"
    );
}
