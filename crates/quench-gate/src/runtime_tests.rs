//! Running a filtered selection of a package's library tests (`quench-runtime`'s unless named), which must have matched
//! at least one test: a filter that selects nothing proves nothing.

use crate::{config::Config, process};

const PACKAGE: &str = "quench-runtime";
const TEST_PREFIX: &str = "test ";
const RESULT_SEPARATOR: &str = " ... ";
const PASSED: &str = "ok";
const FAILED: &str = "FAILED";
/// The line libtest ends a finished run with; a run without it died before reporting every test.
const SUMMARY: &str = "test result: ";

/// What one run of the selected tests reported, by test name.
pub struct Tally {
    pub passed: Vec<String>,
    pub failed: Vec<String>,
    /// The end of the run's output, for a report.
    pub excerpt: String,
}

/// The tests the cargo test output reports under `outcome`.
fn named(output: &str, outcome: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| line.strip_prefix(TEST_PREFIX)?.split_once(RESULT_SEPARATOR))
        .filter(|(_, result)| *result == outcome)
        .map(|(name, _)| name.to_owned())
        .collect()
}

/// Runs the `quench-runtime` library tests whose names contain any of `filters` (`package_tally`).
pub fn tally(config: &Config, profile: &str, filters: &[String]) -> Result<Tally, String> {
    package_tally(config, PACKAGE, profile, filters)
}

/// Runs `package`'s library tests whose names contain any of `filters`. A run that matched no test, or
/// that ended without its summary (a failed build, a crashed test binary), is an error; test failures are the caller's to judge.
pub fn package_tally(config: &Config, package: &str, profile: &str, filters: &[String]) -> Result<Tally, String> {
    let mut command = config.cargo_in("test", profile);
    command.args(["-p", package, "--lib", "--"]).args(filters);
    let finished = process::run(&mut command)?;
    let (passed, failed) = (named(&finished.stdout, PASSED), named(&finished.stdout, FAILED));
    if !finished.stdout.lines().any(|line| line.starts_with(SUMMARY)) {
        return Err(format!("the {package} test run ended without its summary:\n{}", finished.excerpt()));
    }
    if passed.is_empty() && failed.is_empty() {
        return Err(format!("no {package} test matched {filters:?}"));
    }
    Ok(Tally { passed, failed, excerpt: finished.excerpt() })
}

/// Runs the selection, every test of which must pass; returns how many did.
pub fn run(config: &Config, filters: &[String]) -> Result<usize, String> {
    let tally = tally(config, crate::config::PROFILE, filters)?;
    if tally.failed.is_empty() { Ok(tally.passed.len()) } else { Err(tally.excerpt) }
}
