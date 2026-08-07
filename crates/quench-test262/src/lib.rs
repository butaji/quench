//! Standalone ECMA-262 conformance runner boundary.
//!
//! This crate owns runner outcomes and dispatch. The engine is accessed only
//! through [`quench_runtime::Test262Host`]; parser, IR, heap, and builtin
//! implementation details remain in `quench-runtime`.

use quench_runtime::Test262Host;

/// Result of dispatching one test source to the engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestOutcome {
    /// The engine completed without an abrupt completion.
    Pass,
    /// The engine rejected or threw while running the test.
    Fail { reason: String },
}

/// Runner parameterized over the engine host implementation.
pub struct Test262Runner<H: Test262Host> {
    host: H,
}

impl<H: Test262Host> Test262Runner<H> {
    /// Create a runner around an engine host.
    pub fn new(host: H) -> Self {
        Self { host }
    }

    /// Run one script test source.
    pub fn run_script(&mut self, source: &str) -> TestOutcome {
        map_result(self.host.run_script(source))
    }

    /// Run one module test source.
    pub fn run_module_script(&mut self, source: &str) -> TestOutcome {
        map_result(self.host.run_module_script(source))
    }
}

fn map_result(result: Result<(), String>) -> TestOutcome {
    match result {
        Ok(()) => TestOutcome::Pass,
        Err(reason) => TestOutcome::Fail { reason },
    }
}
