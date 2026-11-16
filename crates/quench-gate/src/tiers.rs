//! The execution modes the gate runs programs in, as the command-line flags of the runners.

use quench_tier_corpus::residuals::Tier;

/// The flag that runs the interpreter alone.
pub const INTERPRETER: &str = "--no-jit";
const TIER_FLAG_PREFIX: &str = "--";

/// The flag that runs `tier` alone.
pub fn flag(tier: Tier) -> String {
    format!("{TIER_FLAG_PREFIX}{}", tier.name())
}

/// The modes trunk must pass whole suites in: the interpreter, b-jit alone, and the default
/// (every tier), with their runner flag (none for the default).
pub const TRUNK_MODES: [(&str, Option<&str>); 3] = [("no-jit", Some(INTERPRETER)), ("b-jit", Some("--b-jit")), ("default", None)];
