//! The execution modes the gate runs programs in, as the command-line flags of the runners.

use std::iter;

use quench_tier_corpus::residuals::Tier;

/// The flag that runs the interpreter alone.
pub const INTERPRETER: &str = "--no-jit";
const FLAG_PREFIX: &str = "--";

/// The flag that runs `tier` alone.
pub fn flag(tier: Tier) -> String {
    format!("{FLAG_PREFIX}{}", tier.name())
}

/// Every mode: the interpreter, then each JIT tier alone.
pub fn modes() -> impl Iterator<Item = String> {
    iter::once(INTERPRETER.to_owned()).chain(Tier::ALL.into_iter().map(flag))
}
