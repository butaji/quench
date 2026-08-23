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
    let args: Vec<String> = std::env::args().collect();
    if let Some(path) = args
        .iter()
        .position(|arg| arg == "--fixture-one")
        .and_then(|index| args.get(index + 1))
    {
        return match quench_node_test::NodeTestRunner::new().run_file(&PathBuf::from(path)) {
            NodeOutcome::Pass => ExitCode::SUCCESS,
            NodeOutcome::Skip { .. } => ExitCode::from(2),
            NodeOutcome::Fail { reason } => {
                eprintln!("FAIL {path}: {reason}");
                ExitCode::from(1)
            }
        };
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("run-compat: walk a Node compat test directory through the host");
        println!();
        println!("usage: run-compat [--list] [--filter NAME] [--quiet] [--test-dir DIR]");
        println!("  --list           enumerate the suite instead of running it");
        println!("  --filter NAME    only run scripts whose name contains NAME");
        println!("  --quiet          skip per-test output, only the summary");
        println!(
            "  --test-dir DIR  compat suite root (default: crates/quench-node-test/node-tests)"
        );
        println!("  each fixture is bounded by a 30 second timeout");
        return ExitCode::SUCCESS;
    }
    let mut args = args.into_iter().skip(1);
    let mut quiet = false;
    let mut list = false;
    let mut dir = PathBuf::from("crates/quench-node-test/node-tests");
    while let Some(a) = args.next() {
        match a.as_str() {
            "--list" => list = true,
            "--filter" => {
                args.next();
            }
            "--quiet" => quiet = true,
            "--test-dir" => {
                let Some(path) = args.next() else {
                    eprintln!("error: --test-dir requires a directory");
                    return ExitCode::from(2);
                };
                dir = PathBuf::from(path);
            }
            _ if a.starts_with('-') => {
                eprintln!("error: unknown option {a}");
                return ExitCode::from(2);
            }
            _ => dir = PathBuf::from(a),
        }
    }
    run_with_dir(dir, quiet, list)
}
fn run_with_dir(dir: std::path::PathBuf, quiet: bool, list: bool) -> ExitCode {
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
    if fixtures.is_empty() {
        eprintln!("error: filter selected no `*.js` fixtures under {}", dir.display());
        return ExitCode::from(2);
    }
    if list {
        for f in &fixtures {
            println!("{}", f.file_name().unwrap().to_string_lossy());
        }
        return ExitCode::SUCCESS;
    }
    let mut runner = quench_node_test::NodeTestRunner::new();
    let summary = run_suite(&mut runner, &fixtures, quiet);
    print_summary(&summary, fixtures.len());
    // A skip is not compatibility evidence.  Keep it visible in the
    // aggregate and fail the gate until the fixture is either supported or
    // explicitly removed from the selected suite.
    if summary.failed == 0 && summary.skipped == 0 {
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
    _runner: &mut quench_node_test::NodeTestRunner,
    fixtures: &[PathBuf],
    quiet: bool,
) -> SuiteSummary {
    use std::process::{Command, Stdio};
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("run-compat"));
    let mut summary = SuiteSummary {
        passed: 0,
        skipped: 0,
        failed: 0,
        failed_names: Vec::new(),
    };
    for fixture in fixtures {
        let result = Command::new(&exe)
            .arg("--fixture-one")
            .arg(fixture)
            .stdout(if quiet {
                Stdio::null()
            } else {
                Stdio::inherit()
            })
            .stderr(Stdio::piped())
            .spawn();
        let result = match result {
            Err(error) => Err(error),
            Ok(mut child) => {
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
                loop {
                    match child.try_wait() {
                        Ok(Some(_)) => break child.wait_with_output().map(Some),
                        Ok(None) => {}
                        Err(error) => break Err(error),
                    }
                    if std::time::Instant::now() >= deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        break Ok(None);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            }
        };
        let (passed, skipped, reason) = match result {
            Ok(Some(output)) if output.status.success() => (true, false, None),
            Ok(Some(output)) if output.status.code() == Some(2) => (false, true, None),
            Ok(Some(output)) => (
                false,
                false,
                Some(String::from_utf8_lossy(&output.stderr).trim().to_string()),
            ),
            Ok(None) => (
                false,
                false,
                Some("fixture timed out after 30s".to_string()),
            ),
            Err(error) => (
                false,
                false,
                format!("spawn fixture process: {error}").into(),
            ),
        };
        if skipped {
            summary.skipped += 1;
        } else if passed {
            if !quiet {
                println!("PASS  {}", fixture.display());
            }
            summary.passed += 1;
        } else {
            let reason = reason
                .filter(|reason| !reason.is_empty())
                .unwrap_or_else(|| {
                    "fixture process terminated abnormally (isolated from suite)".into()
                });
            println!("FAIL  {}: {reason}", fixture.display());
            summary.failed += 1;
            summary.failed_names.push(fixture.display().to_string());
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
