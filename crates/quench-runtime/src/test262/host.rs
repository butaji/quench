//! Trait boundary between the test262 runner and the engine under test.

use crate::test262::harness::try_inject_harness;
use crate::test262::skip::is_feature_supported;
use crate::Context;

/// Implement this for your engine to plug it into the test262 runner.
pub trait Test262Host {
    /// Execute a complete JS script (harness + test source).
    /// `Ok(())` if execution completes without throwing,
    /// `Err(message)` if it throws or fails to evaluate.
    fn run_script(&mut self, source: &str) -> Result<(), String>;

    /// Whether a test262 feature (frontmatter `features:` entry) is
    /// implemented. Returning false skips tests that require it.
    fn has_feature(&self, feature: &str) -> bool;
}

/// What happened when we tried to run a test.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum TestOutcome {
    Pass,
    Fail { reason: String },
    Skip { reason: String },
}

impl std::fmt::Display for TestOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TestOutcome::Pass => write!(f, "PASS"),
            TestOutcome::Fail { reason } => write!(f, "FAIL: {}", reason),
            TestOutcome::Skip { reason } => write!(f, "SKIP: {}", reason),
        }
    }
}

/// Host backed by quench: fresh `Context` per script with builtins and harness injected.
pub struct QuenchHost;

impl QuenchHost {
    pub fn new() -> Self {
        QuenchHost
    }
}

impl Default for QuenchHost {
    fn default() -> Self {
        Self::new()
    }
}

impl Test262Host for QuenchHost {
    fn run_script(&mut self, source: &str) -> Result<(), String> {
        let mut ctx = Context::new().map_err(|e| format!("{:?}", e))?;
        crate::builtins::register_builtins(&mut ctx);
        try_inject_harness(&mut ctx).map_err(|e| format!("harness load failure: {}", e))?;
        ctx.eval(source).map(|_| ()).map_err(|e| format!("{:?}", e))
    }

    fn has_feature(&self, feature: &str) -> bool {
        is_feature_supported(feature)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quench_host_runs_and_throws() {
        let mut host = QuenchHost::new();
        assert!(host.run_script("var x = 1 + 1;").is_ok());
        assert!(host.run_script("throw new Error('boom')").is_err());
    }
}
