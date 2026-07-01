// linter-skip
#![allow(function_length)]
//! Custom build/test tasks with guaranteed timeout protection
//!
//! Run with: cargo run -p xtask -- test [args...]

use std::env;
use std::process::{Command, Stdio};
use std::time::Instant;

const DEFAULT_TIMEOUT_SECS: u64 = 300;

#[allow(function_length)]
fn main() {
    let all_args: Vec<String> = env::args().skip(1).collect();
    let task = all_args.first().map(|s| s.as_str()).unwrap_or("help");

    let exit_code = match task {
        "test" => {
            let extra: Vec<String> = all_args.iter().skip(1).cloned().collect();
            run_tests_with_timeout(&extra, DEFAULT_TIMEOUT_SECS)
        }
        "test-quick" => {
            let extra: Vec<String> = all_args.iter().skip(1).cloned().collect();
            run_tests_with_timeout(&extra, 60)
        }
        "test-runtime" => {
            let extra: Vec<String> = all_args.iter().skip(1).cloned().collect();
            let mut args = vec!["-p".to_string(), "quench-runtime".to_string(), "--test".to_string(), "runtime_tests".to_string()];
            args.extend(extra);
            run_tests_with_timeout(&args, 300)
        }
        "test-conformance" => {
            let mut args = vec!["-p".to_string(), "quench-runtime".to_string(), "--test".to_string(), "conformance".to_string()];
            args.push("--".to_string());
            args.push("--ignored".to_string());
            run_tests_with_timeout(&args, 600)
        }
        "test-test262" => {
            let mut args = vec!["-p".to_string(), "quench-runtime".to_string(), "--test".to_string(), "test262".to_string()];
            args.push("--".to_string());
            args.push("--ignored".to_string());
            run_tests_with_timeout(&args, 600)
        }
        "check" => {
            run_command("cargo", &["check", "--all-targets"]);
            return;
        }
        "build" => {
            run_command("cargo", &["build"]);
            return;
        }
        "clippy" => {
            run_command("cargo", &["clippy", "--", "-D", "warnings"]);
            return;
        }
        "help" | _ => {
            eprintln!("xtask - Custom build tasks with timeout protection");
            eprintln!("");
            eprintln!("Usage: cargo run -p xtask -- <command> [args...]");
            eprintln!("");
            eprintln!("Commands:");
            eprintln!("  test [args...]      Run all tests (5min timeout)");
            eprintln!("  test-quick [args..] Run quick tests (1min timeout)");
            eprintln!("  test-runtime        Run runtime tests (3min timeout)");
            eprintln!("  test-conformance    Run conformance tests (10min timeout)");
            eprintln!("  test-test262        Run test262 ECMAScript tests (10min timeout)");
            eprintln!("  check               Type check all targets");
            eprintln!("  build               Build the project");
            eprintln!("  clippy              Run clippy linter");
            std::process::exit(0);
        }
    };
    
    std::process::exit(exit_code);
}

#[allow(function_length)]
fn run_tests_with_timeout(args: &[String], timeout_secs: u64) -> i32 {
    println!("=== Running tests with {}s timeout ===", timeout_secs);
    
    let start = Instant::now();
    
    // Find timeout command
    let timeout_exe = if cfg!(target_os = "macos") {
        if Command::new("gtimeout").arg("--version").output().is_ok() {
            Some("gtimeout")
        } else if Command::new("timeout").arg("--version").output().is_ok() {
            Some("timeout")
        } else {
            eprintln!("WARNING: Neither 'timeout' nor 'gtimeout' found!");
            eprintln!("Install GNU coreutils: brew install coreutils");
            None
        }
    } else {
        if Command::new("timeout").arg("--version").output().is_ok() {
            Some("timeout")
        } else {
            eprintln!("WARNING: 'timeout' command not found!");
            None
        }
    };

    let exit_code = if let Some(cmd) = timeout_exe {
        let status = Command::new(cmd)
            .arg(timeout_secs.to_string())
            .arg("cargo")
            .arg("test")
            .args(args)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status();
        
        match status {
            Ok(s) => s.code().unwrap_or(1),
            Err(e) => {
                eprintln!("ERROR: Failed to run tests: {}", e);
                1
            }
        }
    } else {
        eprintln!("WARNING: Running WITHOUT timeout protection - tests may hang!");
        let status = Command::new("cargo")
            .arg("test")
            .args(args)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status();
        
        match status {
            Ok(s) => s.code().unwrap_or(1),
            Err(e) => {
                eprintln!("ERROR: Failed to run tests: {}", e);
                1
            }
        }
    };

    let elapsed = start.elapsed();
    println!("\n=== Tests completed in {:.1}s (exit code: {}) ===", 
             elapsed.as_secs_f64(), exit_code);
    
    exit_code
}

fn run_command(cmd: &str, args: &[&str]) {
    let status = Command::new(cmd)
        .args(args)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .expect("Failed to run command");

    std::process::exit(status.code().unwrap_or(1));
}
