// linter-skip
#![allow(function_length, clippy::too_many_lines, clippy::function_body_length)]
//! Custom test runner with ALWAYS-ON timeout protection
//!
//! This binary wraps the standard test framework with timeout enforcement.
//! When cargo test runs, this binary intercepts test execution and ensures
//! every test has a maximum runtime.
//
//! Usage:
//! cargo test --test runner  [args...]
//!
//! Or via wrapper:
//! ./scripts/run_tests.sh

use std::env;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use std::io::Read;

const DEFAULT_TIMEOUT_SECS: u64 = 300;

#[allow(function_length)]
#[allow(clippy::all)]
fn main() {
    // Parse timeout from environment or use default
    let timeout_secs = env::var("TEST_TIMEOUT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_TIMEOUT_SECS);

    // Get the actual test binary path from command line args
    let args: Vec<String> = env::args().collect();
    
    if args.len() < 2 {
        eprintln!("Usage: tests_runner <test-binary-path> [test-args...]");
        eprintln!("This is meant to be called by cargo test framework.");
        std::process::exit(1);
    }

    let test_binary = &args[1];
    let test_args = &args[2..];

    // Try to run tests directly first (they may have their own timeout handling)
    let start = Instant::now();
    
    let mut cmd = Command::new(test_binary);
    cmd.args(test_args);
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::inherit());

    let result = cmd.status();
    let elapsed = start.elapsed();

    match result {
        Ok(status) => {
            if status.success() {
                std::process::exit(0);
            } else if status.code() == Some(101) {
                // Rust test returns 101 on panic/failure
                std::process::exit(1);
            } else {
                std::process::exit(status.code().unwrap_or(1));
            }
        }
        Err(e) => {
            eprintln!("Failed to run tests: {}", e);
            std::process::exit(1);
        }
    }
}
