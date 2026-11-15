//! Running a filtered selection of the `quench-runtime` library tests, which must pass and
//! must have matched at least one test: a filter that selects nothing proves nothing.

use crate::{config::Config, process};

const PACKAGE: &str = "quench-runtime";
const PASSED_PREFIX: &str = "test result: ok. ";

/// The tests the cargo test output reports as passed.
fn passed(output: &str) -> usize {
    output
        .lines()
        .filter_map(|line| line.strip_prefix(PASSED_PREFIX))
        .filter_map(|summary| summary.split_whitespace().next()?.parse::<usize>().ok())
        .sum()
}

/// Runs the library tests whose names contain any of `filters`; returns how many passed.
pub fn run(config: &Config, filters: &[String]) -> Result<usize, String> {
    let mut command = config.cargo("test");
    command.args(["-p", PACKAGE, "--lib", "--"]).args(filters);
    let finished = process::run(&mut command)?;
    if !finished.success {
        return Err(finished.excerpt());
    }
    match passed(&finished.stdout) {
        0 => Err(format!("no {PACKAGE} test matched {filters:?}")),
        count => Ok(count),
    }
}
