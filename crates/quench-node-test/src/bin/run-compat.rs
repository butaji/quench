//! `quench-node-test`'s `run-compat` entry point. Walks the
//! `node-tests/` directory (the Node compat API test set checked
//! in as a plain directory under `quench-node-test`), runs each
//! script through the host, and prints a per-test pass/fail
//! summary plus an aggregate tally.
//!
//! Usage: `cargo run -p quench-node-test --bin run-compat --
//!        [node-tests-dir]`

use std::path::PathBuf;
use std::process::ExitCode;

use quench_node_test::reader::NodeOutcome;
use quench_node_test::stages::discover_fixtures;

fn main() -> ExitCode {
    if std::env::args().any(|a| a == "--help" || a == "-h") {
        println!("run-compat: walk a Node compat test directory through the host");
        println!();
        println!("usage: run-compat [--list] [--filter NAME] [--quiet] [DIR]");
        println!("  --list           enumerate the suite instead of running it");
        println!("  --filter NAME    only run scripts whose name contains NAME");
        println!("  --quiet          skip per-test output, only the summary");
        println!(
            "  DIR              compat suite root (default: crates/quench-node-test/node-tests)"
        );
        return ExitCode::SUCCESS;
    }
    let mut args = std::env::args().skip(1);
    let mut quiet = false;
    while let Some(a) = args.next() {
        if a == "--list" {
            // Handled in discover_fixtures; skip.
        } else if a == "--filter" {
            // Skip the value too.
            args.next();
        } else if a == "--quiet" {
            quiet = true;
        } else {
            // Positional arg = dir.
            let dir = std::path::PathBuf::from(a);
            return run_with_dir(dir, quiet);
        }
    }
    run_with_dir(
        std::path::PathBuf::from("crates/quench-node-test/node-tests"),
        quiet,
    )
}

fn run_with_dir(dir: std::path::PathBuf, quiet: bool) -> ExitCode {
    if !dir.is_dir() {
        eprintln!("error: {} is not a directory", dir.display());
        return ExitCode::from(2);
    }
    let fixtures = discover_fixtures(&dir);
    if fixtures.is_empty() {
        eprintln!("error: no `*.js` fixtures under {}", dir.display());
        return ExitCode::from(2);
    }
    let fixtures = filter_fixtures(fixtures);
    if std::env::args().any(|a| a == "--list") {
        for f in &fixtures {
            println!("{}", f.file_name().unwrap().to_string_lossy());
        }
        return ExitCode::SUCCESS;
    }
    let mut runner = quench_node_test::NodeTestRunner::new();
    let summary = run_suite(&mut runner, &fixtures, quiet);
    print_summary(&summary, fixtures.len());
    if summary.failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn filter_fixtures(fixtures: Vec<std::path::PathBuf>) -> Vec<std::path::PathBuf> {
    let args: Vec<String> = std::env::args().collect();
    let idx = args.iter().position(|a| a == "--filter");
    let filter = idx.and_then(|i| args.get(i + 1).cloned());
    match filter {
        Some(name) => fixtures
            .into_iter()
            .filter(|f| f.file_name().unwrap().to_string_lossy().contains(&name))
            .collect(),
        None => fixtures,
    }
}

struct SuiteSummary {
    passed: usize,
    skipped: usize,
    failed: usize,
    failed_names: Vec<String>,
}

impl SuiteSummary {
    fn total(&self) -> usize {
        self.passed + self.skipped + self.failed
    }
}

fn run_suite(
    runner: &mut quench_node_test::NodeTestRunner,
    fixtures: &[PathBuf],
    quiet: bool,
) -> SuiteSummary {
    let mut summary = SuiteSummary {
        passed: 0,
        skipped: 0,
        failed: 0,
        failed_names: Vec::new(),
    };
    for fixture in fixtures {
        let outcome = runner.run_file(fixture);
        match outcome {
            NodeOutcome::Pass => {
                if !quiet {
                    println!("PASS  {}", fixture.display());
                }
                summary.passed += 1;
            }
            NodeOutcome::Skip { reason } => {
                if !quiet {
                    println!("SKIP  {}: {reason}", fixture.display());
                }
                summary.skipped += 1;
            }
            NodeOutcome::Fail { reason } => {
                println!("FAIL  {}: {reason}", fixture.display());
                summary.failed += 1;
                summary.failed_names.push(fixture.display().to_string());
            }
        }
    }
    summary
}

fn print_summary(summary: &SuiteSummary, total: usize) {
    debug_assert_eq!(summary.total(), total);
    println!(
        "\ncompat: {passed} passed, {skipped} skipped, {failed} failed, {total} total",
        passed = summary.passed,
        skipped = summary.skipped,
        failed = summary.failed,
    );
    if !summary.failed_names.is_empty() {
        println!("failures:");
        for name in &summary.failed_names {
            println!("  - {name}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SuiteSummary;

    #[test]
    fn summary_total_includes_skips_and_failures() {
        let summary = SuiteSummary {
            passed: 2,
            skipped: 3,
            failed: 1,
            failed_names: vec!["broken.js".into()],
        };
        assert_eq!(summary.total(), 6);
    }
}
