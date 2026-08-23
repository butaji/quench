//! `quench-node-test`'s single CLI entry point. Runs one
//! JavaScript file through the host and prints the outcome.
//!
//! Usage: `cargo run -p quench-node-test --bin run -- <file.js>`

use std::path::PathBuf;
use std::process::ExitCode;

use quench_node_test::runner::run_file;

fn main() -> ExitCode {
    let Some(path) = std::env::args_os().nth(1).map(PathBuf::from) else {
        eprintln!("usage: cargo run -p quench-node-test --bin run -- <file.js>");
        return ExitCode::from(2);
    };
    match run_file(&path) {
        quench_node_test::NodeOutcome::Pass => {
            println!("PASS {}", path.display());
            ExitCode::SUCCESS
        }
        quench_node_test::NodeOutcome::Fail { reason } => {
            eprintln!("FAIL {}: {reason}", path.display());
            ExitCode::from(1)
        }
        quench_node_test::NodeOutcome::Skip { reason } => {
            eprintln!("SKIP {}: {reason}", path.display());
            // A skipped fixture is not compatibility evidence. Keep the
            // command contract aligned with run-compat and run-parallel.
            ExitCode::from(1)
        }
    }
}
