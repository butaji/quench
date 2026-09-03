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
    argv: Vec<String>,
}

impl Options {
    fn parse(args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut options = Self {
            dir: PathBuf::from("crates/quench-node-test/node-tests"),
            list: false,
            quiet: false,
            filter: None,
            argv: Vec::new(),
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
                value => options.argv.push(value.to_string()),
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
        if dir.is_file() {
            let mut runner = quench_node_test::NodeTestRunner::new();
            return match runner.run_file_with_args(&dir, options.argv) {
                NodeOutcome::Pass | NodeOutcome::Skip { .. } => ExitCode::SUCCESS,
                NodeOutcome::Fail { reason } => {
                    eprintln!("{reason}");
                    ExitCode::from(1)
                }
            };
        }
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
            println!("{}", f.file_name().unwrap().to_string_lossy());
        }
        return ExitCode::SUCCESS;
    }
    let mut runner = quench_node_test::NodeTestRunner::new();
    let summary = run_suite(&mut runner, &fixtures, options.quiet);
    print_summary(&summary, fixtures.len());
    if summary.failed == 0 {
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
            .filter(|f| f.file_name().unwrap().to_string_lossy().contains(&name))
            .collect(),
        None => fixtures,
    }
}

struct SuiteSummary {
    passed: usize,
    failed: usize,
    failed_names: Vec<String>,
}

fn run_suite(
    runner: &mut quench_node_test::NodeTestRunner,
    fixtures: &[PathBuf],
    quiet: bool,
) -> SuiteSummary {
    let mut summary = SuiteSummary {
        passed: 0,
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
        }
    }
    summary
}

fn print_summary(summary: &SuiteSummary, total: usize) {
    println!(
        "\ncompat: {passed} passed, {failed} failed, {total} total",
        passed = summary.passed,
        failed = summary.failed,
    );
    if !summary.failed_names.is_empty() {
        println!("failures:");
        for name in &summary.failed_names {
            println!("  - {name}");
        }
    }
}
