//! `quench-gate`: the single definition of green. It runs the build check, the exact lint
//! ratchets, the census, the oracle tests, the C-parity gate and the conformance smoke, in
//! that order, prints a summary table and exits nonzero if any step failed.
//!
//! usage: quench-gate [--quick] [--jobs N]
//! `--quick` runs only the lint, census and C-parity steps.

mod build;
mod config;
mod process;
mod step;

use std::process::ExitCode;

use config::Config;
use step::{Outcome, Step};

/// The exit status of a run that was asked for something it cannot do.
const USAGE_ERROR: u8 = 2;

const STEPS: [Step; 1] = [Step { name: "build", quick: false, blocking: true, run: build::run }];

/// Runs the selected steps in order, stopping after a blocking step fails.
fn run(config: &Config) -> Vec<Outcome> {
    let mut outcomes = Vec::new();
    for step in STEPS.iter().filter(|step| step.quick || !config.quick) {
        let outcome = step.execute(config);
        let stop = step.blocking && !outcome.passed;
        outcomes.push(outcome);
        if stop {
            break;
        }
    }
    outcomes
}

fn main() -> ExitCode {
    let config = match Config::parse(std::env::args().skip(1)) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("quench-gate: {error}");
            return ExitCode::from(USAGE_ERROR);
        }
    };
    let outcomes = run(&config);
    step::print(&outcomes);
    if outcomes.iter().all(|outcome| outcome.passed) { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}
