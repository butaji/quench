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
    let options = match Options::parse(std::env::args().skip(1)) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("error: {message}");
            return ExitCode::from(2);
        }
    };
    run_with_options(options)
}

struct Options {
    dir: PathBuf,
    list: bool,
    quiet: bool,
    filter: Option<String>,
}

impl Options {
    fn parse(args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut options = Self {
            dir: PathBuf::from("crates/quench-node-test/node-tests"),
            list: false,
            quiet: false,
            filter: None,
        };
        let mut positional = None;
        let mut args = args;
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--list" => options.list = true,
                "--quiet" => options.quiet = true,
                "--filter" => {
                    options.filter = Some(args.next().ok_or("--filter requires a name")?);
                }
                value if value.starts_with('-') => return Err(format!("unknown option {value}")),
                value if positional.is_none() => positional = Some(PathBuf::from(value)),
                _ => return Err("only one directory may be specified".into()),
            }
        }
        if let Some(dir) = positional {
            options.dir = dir;
        }
        Ok(options)
    }
}

fn run_with_options(options: Options) -> ExitCode {
    let dir = options.dir;
    if !dir.is_dir() {
        eprintln!("error: {} is not a directory", dir.display());
        return ExitCode::from(2);
    }
    let fixtures = discover_fixtures(&dir);
    if fixtures.is_empty() {
        eprintln!("error: no `*.js` fixtures under {}", dir.display());
        return ExitCode::from(2);
    }
    let fixtures = filter_fixtures(fixtures, options.filter.as_deref());
    if fixtures.is_empty() {
        if let Some(filter) = options.filter.as_deref() {
            eprintln!("error: no fixtures match --filter {filter:?}");
        } else {
            eprintln!("error: no fixtures remain after discovery");
        }
        return ExitCode::from(2);
    }
    if options.list {
        for f in &fixtures {
            let display = f.strip_prefix(&dir).unwrap_or(f);
            println!("{}", display.display());
        }
        return ExitCode::SUCCESS;
    }
    let mut runner = quench_node_test::NodeTestRunner::new();
    let summary = run_suite(&mut runner, &fixtures, options.quiet);
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

fn filter_fixtures(
    fixtures: Vec<std::path::PathBuf>,
    filter: Option<&str>,
) -> Vec<std::path::PathBuf> {
    match filter {
        Some(name) => fixtures
            .into_iter()
            .filter(|f| fixture_matches(f, &name))
            .collect(),
        None => fixtures,
    }
}

fn fixture_matches(path: &std::path::Path, filter: &str) -> bool {
    path.to_string_lossy().contains(filter)
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
            }
            NodeOutcome::Fail { reason } => {
                if !quiet {
                    println!("FAIL  {}: {reason}", fixture.display());
                }
                summary.failed += 1;
                summary.failed_names.push(fixture.display().to_string());
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
    use super::{fixture_matches, SuiteSummary};
    use std::path::PathBuf;

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

    #[test]
    fn filter_matches_nested_fixture_paths() {
        let fixture = PathBuf::from("node-tests/node_modules/ms/index.js");
        assert!(fixture_matches(&fixture, "node_modules/ms/index.js"));
    }
}
