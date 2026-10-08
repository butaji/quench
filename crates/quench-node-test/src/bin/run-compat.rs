//! Canonical Node compatibility runner entry point.

use quench_node_test::{case_process::worker_entry, compat_cli};
use std::process::ExitCode;

fn main() -> ExitCode {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    if let Some(code) = worker_entry(&arguments) {
        return code;
    }
    compat_cli::run(arguments)
}
