//! Node compatibility runner entry point.

use quench_node_test::{case_process::worker_entry_with, compat_cli, shared_runner};
use std::process::ExitCode;

fn main() -> ExitCode {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    if let Some(code) = worker_entry_with(&arguments, shared_runner::run_file) {
        return code;
    }
    compat_cli::run(arguments)
}
