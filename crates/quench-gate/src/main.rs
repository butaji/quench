//! `quench-gate`: the single definition of green. It runs the build check, the exact lint
//! ratchets, the census, the oracle tests, the C-parity gate and the conformance smoke, in
//! that order, prints a summary table and exits nonzero if any step failed.
//!
//! usage: quench-gate [--quick] [--jobs N] [--record] [--passed]
//! `--quick` runs only the lint, census and C-parity steps; `--record` writes the C-parity
//! ceilings that have no value yet. A full run that passes on a clean tree records the tree
//! (`passed.rs`); `--passed` only answers whether the checked-out tree has that record.

mod build;
mod census;
mod config;
mod conformance;
mod cparity;
mod lint;
mod oracle;
mod passed;
mod pool;
mod process;
mod runtime_tests;
mod step;
mod tiers;

use std::process::ExitCode;

use config::Config;
use step::{Outcome, Step};

/// The exit status of a run that was asked for something it cannot do.
const USAGE_ERROR: u8 = 2;

const STEPS: [Step; 6] = [
    Step { name: "build", quick: false, blocking: true, run: build::run },
    Step { name: "lint", quick: true, blocking: false, run: lint::run },
    Step { name: "census", quick: true, blocking: false, run: census::run },
    Step { name: "oracle", quick: false, blocking: false, run: oracle::run },
    Step { name: "c-parity", quick: true, blocking: false, run: cparity::run },
    Step { name: "conformance", quick: false, blocking: false, run: conformance::run },
];

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
    if config.passed {
        return report(passed::check(&config));
    }
    let outcomes = run(&config);
    step::print(&outcomes);
    let all_passed = outcomes.iter().all(|outcome| outcome.passed);
    let recorded = passed::record(&config, &outcomes);
    println!("\npass record: {}", recorded.as_ref().map_or_else(Clone::clone, |path| path.display().to_string()));
    if all_passed { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

/// Prints where the pass record is, or why there is none, and exits accordingly.
fn report(record: Result<std::path::PathBuf, String>) -> ExitCode {
    match record {
        Ok(path) => {
            println!("passed: {}", path.display());
            ExitCode::SUCCESS
        }
        Err(reason) => {
            eprintln!("quench-gate: {reason}");
            ExitCode::FAILURE
        }
    }
}
