//! A gate step and the outcome the summary table shows.

use std::time::{Duration, Instant};

use crate::config::Config;

/// A step passes with a one-line detail, or fails with the report of what broke.
pub type Verdict = Result<String, String>;

pub struct Step {
    pub name: &'static str,
    /// Whether `--quick` runs it.
    pub quick: bool,
    /// Whether a failure leaves the steps after it without anything to run on.
    pub blocking: bool,
    pub run: fn(&Config) -> Verdict,
}

pub struct Outcome {
    pub name: &'static str,
    pub passed: bool,
    pub detail: String,
    pub elapsed: Duration,
}

impl Step {
    pub fn execute(&self, config: &Config) -> Outcome {
        let started = Instant::now();
        let verdict = (self.run)(config);
        let elapsed = started.elapsed();
        let passed = verdict.is_ok();
        let (Ok(detail) | Err(detail)) = verdict;
        Outcome { name: self.name, passed, detail, elapsed }
    }
}

/// The summary table, then the report of every failed step.
pub fn print(outcomes: &[Outcome]) {
    println!("{:<14}{:<7}{:>8}  detail", "step", "result", "seconds");
    for outcome in outcomes {
        let result = if outcome.passed { "ok" } else { "FAIL" };
        let first = outcome.detail.lines().next().unwrap_or_default();
        println!("{:<14}{:<7}{:>8.1}  {first}", outcome.name, result, outcome.elapsed.as_secs_f64());
    }
    for outcome in outcomes.iter().filter(|outcome| !outcome.passed || outcome.detail.contains('\n')) {
        println!("\n--- {} ---\n{}", outcome.name, outcome.detail);
    }
}
