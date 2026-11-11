//! Option arguments whose values `parse_options_from` reads: parsed and checked here.
use crate::{required_number, usage};

/// `--timeout-ms`' positive timeout.
pub(crate) fn timeout_ms(args: &mut impl Iterator<Item = String>) -> u64 {
    let timeout_ms = required_number(args, "--timeout-ms");
    if timeout_ms == 0 {
        usage("--timeout-ms must be positive");
    }
    timeout_ms
}

/// `--runs`' positive round count.
pub(crate) fn rounds(args: &mut impl Iterator<Item = String>) -> usize {
    let rounds = usize::try_from(required_number(args, "--runs"))
        .unwrap_or_else(|_| usage("--runs is too large"));
    if rounds == 0 {
        usage("--runs must be positive");
    }
    rounds
}

/// `--jit-wasm-root`'s root id.
pub(crate) fn wasm_root(args: &mut impl Iterator<Item = String>) -> u32 {
    u32::try_from(required_number(args, "--jit-wasm-root"))
        .unwrap_or_else(|_| usage("--jit-wasm-root is too large"))
}
