//! Modes that replace the suite run: `--analyze`, `--tier-differential`, `--tier-invariant` and `--residuals`.

use super::{analysis, fail, residuals, tier_differential, tier_invariant};

/// Runs the subcommand the arguments select and returns its exit status.
pub(crate) fn run(args: &[String]) -> Option<i32> {
    if args.first().is_some_and(|arg| arg == "--analyze") {
        analysis::run(&args[1..]).unwrap_or_else(|error| fail(&error));
        return Some(0);
    }
    if args.iter().any(|arg| arg == tier_differential::FLAG) {
        return Some(tier_differential::run(args));
    }
    if args.iter().any(|arg| arg == residuals::FLAG) {
        return Some(residuals::run(args));
    }
    args.iter()
        .any(|arg| arg == tier_invariant::FLAG)
        .then(|| tier_invariant::run(args))
}

/// A scored run measures the engine, not its tracing.
pub(crate) fn reject_exec_trace() {
    if std::env::var_os("QUENCH_EXEC_TRACE").is_some() {
        fail("scored runs must not inherit QUENCH_EXEC_TRACE");
    }
}
