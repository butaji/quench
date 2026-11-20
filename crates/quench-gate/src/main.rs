//! `quench-gate`: the single definition of green. It runs the build check, the exact lint
//! ratchets, the census, every library test (with and without debug assertions), the C-parity gate and the conformance suites, in
//! that order, prints a summary table and exits nonzero if any step failed.
//!
//! usage: quench-gate [--quick] [--jobs N] [--record] [--passed] [--only STEP] [--strict] [--suites]
//! `--quick` runs only the lint, census and C-parity steps; `--record` writes the C-parity
//! ceilings that have no value yet; `--only STEP` (or a bare step name, `quench-gate aot`) runs one
//! step, e.g. `--only ir-budget`. `--strict` and `--suites` are the AOT step's options (`aot.rs`).
//! A full run that passes on a clean tree records the tree (`passed.rs`); `--passed` only answers
//! whether the checked-out tree has that record.

mod aot;
mod build;
mod census;
mod config;
mod conformance;
mod cparity;
mod debug;
mod ir_budget;
mod ir_floor;
mod js3_budgets;
mod known_lists;
mod lint;
mod no_jit_ir;
mod oracle;
mod owners;
mod passed;
mod process;
mod ratchet;
mod runtime_tests;
mod startup;
mod step;
mod tiers;

use std::process::ExitCode;

use config::Config;
use step::{Outcome, Step};

/// The exit status of a run that was asked for something it cannot do.
const USAGE_ERROR: u8 = 2;

const STEPS: [Step; 13] = [
    Step { name: "build", quick: false, blocking: true, run: build::run },
    Step { name: "lint", quick: true, blocking: false, run: lint::run },
    Step { name: "known-lists", quick: true, blocking: false, run: known_lists::run },
    Step { name: "census", quick: true, blocking: false, run: census::run },
    Step { name: "library", quick: false, blocking: false, run: oracle::run },
    Step { name: "debug", quick: false, blocking: false, run: debug::run },
    Step { name: "c-parity", quick: true, blocking: false, run: cparity::run },
    Step { name: "no-jit-ir", quick: false, blocking: false, run: no_jit_ir::run },
    Step { name: "ir-budget", quick: true, blocking: false, run: ir_budget::run },
    Step { name: "js3-budgets", quick: false, blocking: false, run: js3_budgets::run },
    Step { name: "conformance", quick: false, blocking: false, run: conformance::run },
    Step { name: "aot", quick: false, blocking: false, run: aot::run },
    Step { name: "startup", quick: false, blocking: false, run: startup::run },
];

/// Runs the selected steps in order, stopping after a blocking step fails.
fn run(config: &Config) -> Vec<Outcome> {
    let mut outcomes = Vec::new();
    let selected = |step: &&Step| match &config.only { Some(only) => step.name == only, None => step.quick || !config.quick };
    for step in STEPS.iter().filter(selected) {
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
    println!("\npass record: {}", recorded.unwrap_or_else(|reason| reason));
    if all_passed { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

/// Prints where the pass record is, or why there is none, and exits accordingly.
fn report(record: Result<String, String>) -> ExitCode {
    match record {
        Ok(note) => {
            println!("passed: {note}");
            ExitCode::SUCCESS
        }
        Err(reason) => {
            eprintln!("quench-gate: {reason}");
            ExitCode::FAILURE
        }
    }
}
