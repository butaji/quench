//! Smoke tests for the Quench Node host. These exercise the
//! public host API; they do not run any Node fixture yet.

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
fn runner_run_source_exposes_commonjs_globals() {
    let mut runner = NodeTestRunner::new();
    let outcome = runner.run_source(
        "if (!module || !exports || typeof __filename !== 'string' || typeof __dirname !== 'string') throw new Error('missing CommonJS globals');",
    );
    assert!(matches!(outcome, quench_node_test::NodeOutcome::Pass));
}

#[test]
fn runner_run_source_reassigns_var_bindings() {
    let mut runner = NodeTestRunner::new();
    let outcome = runner.run_source("var value = 1; value = 2; if (value !== 2) throw new Error('var');");
    assert!(matches!(outcome, quench_node_test::NodeOutcome::Pass));
}

#[test]
fn runner_run_source_reassigns_var_inside_factory() {
    let mut runner = NodeTestRunner::new();
    let outcome = runner.run_source("(function(module){ var value = 1; value = 2; module.exports = value; })(module);");
    assert!(matches!(outcome, quench_node_test::NodeOutcome::Pass));
}
