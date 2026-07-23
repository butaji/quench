//! Standalone test262 runner — same path as the in-process harness (`run_single_test`).
//! Usage: cargo build -p run-test && target/debug/run-test <path-to-test.js>
//!
//! Exit codes: 0 pass · 1 fail · 2 skip

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use quench_runtime::test262::metadata::Test262Metadata;
use quench_runtime::test262::runner::default_test262_dir;
use quench_runtime::test262::runner::run_single_test;
use quench_runtime::test262::{HarnessLoader, QuenchHost, TestOutcome};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: run-test <path-to-test.js>");
        return ExitCode::from(1);
    }

    let path = PathBuf::from(&args[1]);
    let source = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading {}: {}", path.display(), e);
            return ExitCode::from(1);
        }
    };

    print_header(&path, &source);

    let test262_dir = default_test262_dir();
    let harness = HarnessLoader::new(&test262_dir);
    let mut host = QuenchHost::new();
    match run_single_test(&mut host, &harness, &path) {
        TestOutcome::Pass => {
            println!("✅ PASSED");
            ExitCode::SUCCESS
        }
        TestOutcome::Skip { reason } => {
            eprintln!("⏭ SKIP: {}", reason);
            ExitCode::from(2)
        }
        TestOutcome::Fail { reason } => {
            println!("❌ FAILED\n   Reason: {}", reason);
            ExitCode::from(1)
        }
    }
}

fn print_header(path: &Path, source: &str) {
    println!("╔══════════════════════════════════════════════════╗");
    println!(
        "║  Test: {}",
        path.file_name().unwrap_or_default().to_string_lossy()
    );
    println!("╚══════════════════════════════════════════════════╝");

    if let Some(m) = Test262Metadata::parse(source) {
        if let Some(ref d) = m.description {
            println!("  Description: {}", d);
        }
        if !m.features.is_empty() {
            println!("  Features: {}", m.features.join(", "));
        }
        if let Some(ref e) = m.esid {
            println!("  Spec: §{}", e);
        }
        if let Some(ref n) = m.negative {
            println!("  Expected: {} ({})", n.typ, n.phase);
        }
        if !m.includes.is_empty() {
            println!("  Includes: {}", m.includes.join(", "));
        }
        if !m.flags.is_empty() {
            println!("  Flags: {}", m.flags.join(", "));
        }
    }
    println!();
    println!("───────────────────── Source ─────────────────────");
    for (i, line) in source.lines().enumerate() {
        println!("{:4}: {}", i + 1, line);
    }
    println!("──────────────────────────────────────────────────\n");
}
