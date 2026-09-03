//! Smoke tests for the Quench Node host and representative runtime semantics.

use quench_node::host;
use quench_node_test::runner::NodeTestRunner;

#[test]
fn host_install_returns_arc_and_context() {
    let (_host, ctx) = host::install(quench_runtime::ops::RealmId::ROOT);
    let _ = ctx;
}

#[test]
fn runner_run_source_passes_for_hello() {
    let mut runner = NodeTestRunner::new();
    let outcome = runner.run_source("console.log('hello');");
    assert!(matches!(outcome, quench_node_test::NodeOutcome::Pass));
}

#[test]
fn runner_run_source_requires_node_module() {
    let mut runner = NodeTestRunner::new();
    let outcome =
        runner.run_source("const fs = require('node:fs'); console.log(typeof fs.readFileSync);");
    assert!(matches!(outcome, quench_node_test::NodeOutcome::Pass));
}

#[test]
fn generator_try_yield_inside_loop_resumes_and_catches() {
    let source = r#"
function* values() {
  for (let i = 0; i < 18; i++) {
    try {
      if ((i + 6) % 13 === 0) throw i;
      yield i;
    } catch (error) {
      yield error & 7;
    }
  }
}
let total = 0;
for (const value of values()) total += value;
if (total !== 153) throw new Error("generator loop completion");
"#;
    let mut runner = NodeTestRunner::new();
    let outcome = runner.run_source(source);
    assert!(matches!(outcome, quench_node_test::NodeOutcome::Pass));
}

#[test]
fn generator_injected_throw_continues_loop_after_catch() {
    let source = r#"
function* values() {
  for (let i = 0; i < 3; i++) {
    try { yield i; }
    catch (error) { if (error !== 9) throw error; }
  }
}
const iterator = values();
if (iterator.next().value !== 0) throw new Error("first yield");
const thrown = iterator.throw(9);
if (thrown.value !== 1 || thrown.done) throw new Error("catch must continue at next yield");
if (iterator.next().value !== 2) throw new Error("third yield");
if (!iterator.next().done) throw new Error("loop must finish");
"#;
    let mut runner = NodeTestRunner::new();
    let outcome = runner.run_source(source);
    assert!(matches!(outcome, quench_node_test::NodeOutcome::Pass));
}
