//! Step 5: the `quench-runtime` library tests built with debug assertions, which the release
//! profiles compile out: an assertion that fails here is a fact a release build relies on unchecked.

use crate::{config::Config, ratchet, runtime_tests, step::Verdict};

/// The cargo profile with debug assertions on.
const DEBUG_PROFILE: &str = "dev";
/// The tests expected to fail under debug assertions until their cause is fixed.
const KNOWN_FAILURES: &str = "crates/quench-gate/known_debug_failures.txt";

pub fn run(config: &Config) -> Verdict {
    let tally = runtime_tests::tally(config, DEBUG_PROFILE, &[])?;
    ratchet::hold(&tally, &config.root.join(KNOWN_FAILURES), "debug-profile")
}
