//! Step 4: every `quench-runtime` library test, the differential oracle's among them
//! (`vm/test_oracle.rs`: the interpreter and a tier must end alike, and the tier's events must
//! show it engaged), held to the known-failure ratchet (`ratchet`). A test that is not run
//! proves nothing, so the step selects no subset.

use crate::{config::{Config, PROFILE}, ratchet, runtime_tests, step::Verdict};

/// The library tests expected to fail until their cause is fixed, one name per line.
const KNOWN_FAILURES: &str = "crates/quench-gate/known_failures.txt";

pub fn run(config: &Config) -> Verdict {
    let tally = runtime_tests::tally(config, PROFILE, &[])?;
    ratchet::hold(&tally, &config.root.join(KNOWN_FAILURES), "library")
}
