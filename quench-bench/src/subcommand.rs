//! Modes that replace the suite run: `--analyze` and `--tier-invariant`.

use super::{analysis, fail, tier_invariant};

/// Runs the subcommand the arguments select and returns its exit status.
pub(crate) fn run(args: &[String]) -> Option<i32> {
    if args.first().is_some_and(|arg| arg == "--analyze") {
        analysis::run(&args[1..]).unwrap_or_else(|error| fail(&error));
        return Some(0);
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
