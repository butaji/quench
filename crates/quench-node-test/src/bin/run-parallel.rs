//! Run real Node.js `test/parallel` fixtures from the `tests/node`
//! submodule through the host.
//!
//! Default mode runs the manifest (one test file name per line, `#`
//! comments allowed) and fails if any listed test regresses. An inline
//! `profile=NAME` comment selects a reviewed subset with `--profile NAME`.
//! `--triage` sweeps the whole `parallel/` directory and prints the
//! tests that pass — diagnostic output for growing the manifest,
//! never a conformance gate.
//!
//! Usage:
//!   cargo run -p quench-node-test --bin run-parallel
//!   cargo run -p quench-node-test --bin run-parallel -- --profile framework-core
//!   cargo run -p quench-node-test --bin run-parallel -- --triage [--filter NAME]

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use quench_node_test::case_process::{
    observe_parallel_case, worker_entry, RunResult, DEFAULT_CASE_TIMEOUT_SECS,
};

const PARALLEL_DIR: &str = "tests/node/test/parallel";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(code) = worker_entry(&args) {
        return code;
    }
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_help();
        return ExitCode::SUCCESS;
    }
    if let Some(path) = args
        .iter()
        .position(|arg| arg == "--one")
        .and_then(|index| args.get(index + 1))
    {
        return run_one(PathBuf::from(path));
    }
    if args.iter().any(|arg| arg == "--triage-one") {
        eprintln!("error: obsolete private worker invocation");
        return ExitCode::from(2);
    }
    if args.iter().any(|a| a == "--triage") {
        let filter = args
            .iter()
            .position(|a| a == "--filter")
            .and_then(|i| args.get(i + 1));
        let timeout = args
            .iter()
            .position(|a| a == "--timeout-secs")
            .and_then(|i| args.get(i + 1))
            .and_then(|value| value.parse().ok())
            .unwrap_or(DEFAULT_CASE_TIMEOUT_SECS);
        return triage(filter, timeout);
    }
    if args.iter().any(|a| a == "--all") {
        let filter = args
            .iter()
            .position(|a| a == "--filter")
            .and_then(|i| args.get(i + 1));
        let timeout = args
            .iter()
            .position(|a| a == "--timeout-secs")
            .and_then(|i| args.get(i + 1))
            .and_then(|value| value.parse().ok())
            .unwrap_or(DEFAULT_CASE_TIMEOUT_SECS);
        let jobs = args
            .iter()
            .position(|a| a == "--jobs")
            .and_then(|i| args.get(i + 1))
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|jobs| *jobs > 0)
            .unwrap_or_else(|| {
                std::thread::available_parallelism()
                    .map(usize::from)
                    .unwrap_or(1)
            });
        let results = args
            .iter()
            .position(|a| a == "--results")
            .and_then(|i| args.get(i + 1));
        return run_all(filter, timeout, jobs, results);
    }
    let profile = args
        .iter()
        .position(|arg| arg == "--profile")
        .and_then(|index| args.get(index + 1));
    if args.iter().any(|arg| arg == "--profile") && profile.is_none() {
        eprintln!("error: --profile requires a profile name");
        return ExitCode::from(2);
    }
    run_manifest(profile.map(String::as_str))
}

fn print_help() {
    println!("run-parallel: execute Node parallel fixtures through quench-node");
    println!();
    println!("usage:");
    println!("  run-parallel                         run the checked-in stage manifest");
    println!("  run-parallel --profile NAME          run a tagged manifest subset");
    println!("  run-parallel --one PATH               run one fixture");
    println!("  run-parallel --all [options]         run the recursive fixture inventory");
    println!("  run-parallel --triage [options]        print passing triage fixtures");
    println!();
    println!("options for --all:");
    println!("  --filter NAME       restrict fixtures by filename");
    println!("  --timeout-secs N    isolate each fixture with an N-second timeout (default 30)");
    println!("  --jobs N            run up to N isolated fixtures concurrently (default: available CPUs)");
    println!("  --results PATH      write machine-readable results and inventory hash");
}

fn run_one(path: PathBuf) -> ExitCode {
    let executable = match std::env::current_exe() {
        Ok(executable) => executable,
        Err(error) => {
            eprintln!("worker executable: {error}");
            return ExitCode::from(2);
        }
    };
    match observe_parallel_case(
        &executable,
        &path,
        std::time::Duration::from_secs(DEFAULT_CASE_TIMEOUT_SECS),
    ) {
        Ok(observation) => {
            use std::io::Write;
            if std::io::stdout().write_all(&observation.stdout).is_err()
                || std::io::stderr().write_all(&observation.stderr).is_err()
            {
                return ExitCode::from(2);
            }
            let result = observation.outcome();
            println!("{} {}", result.label().to_uppercase(), path.display());
            if result == RunResult::Pass {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Err(error) => {
            eprintln!("worker {}: {error}", path.display());
            ExitCode::from(2)
        }
    }
}

fn run_manifest(profile: Option<&str>) -> ExitCode {
    quench_node_test::parallel_profile::run(profile, None, DEFAULT_CASE_TIMEOUT_SECS)
}

fn triage(filter: Option<&String>, timeout_secs: u64) -> ExitCode {
    if !std::path::Path::new(PARALLEL_DIR).is_dir() {
        eprintln!(
            "error: upstream Node fixture directory is missing: {PARALLEL_DIR}\n\
             initialize the tests/node submodule before running triage"
        );
        return ExitCode::from(2);
    }
    let entries = match quench_node_test::stages::discover_fixtures(&PathBuf::from(PARALLEL_DIR)) {
        Ok(entries) => entries,
        Err(error) => {
            eprintln!("error: fixture discovery: {error}");
            return ExitCode::from(2);
        }
    };
    let mut entries: Vec<PathBuf> = entries
        .into_iter()
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.starts_with("test-")
                        && filter.as_ref().is_none_or(|f| name.contains(f.as_str()))
                })
        })
        .collect();
    entries.sort();
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("run-parallel"));
    let mut passed = 0;
    for path in &entries {
        if matches!(triage_one(&exe, path, timeout_secs), RunResult::Pass) {
            println!("{}", path.file_name().unwrap().to_string_lossy());
            passed += 1;
        }
    }
    eprintln!("triage: {passed} passed of {}", entries.len());
    ExitCode::SUCCESS
}

fn triage_one(exe: &std::path::Path, path: &std::path::Path, timeout_secs: u64) -> RunResult {
    match observe_parallel_case(exe, path, std::time::Duration::from_secs(timeout_secs)) {
        Ok(observation) => observation.outcome(),
        Err(error) => {
            eprintln!("worker {}: {error}", path.display());
            RunResult::Unclassified
        }
    }
}

fn run_all(
    filter: Option<&String>,
    timeout_secs: u64,
    jobs: usize,
    results_path: Option<&String>,
) -> ExitCode {
    let root = PathBuf::from(PARALLEL_DIR);
    let mut entries = match quench_node_test::stages::discover_fixtures(&root) {
        Ok(entries) => entries,
        Err(error) => {
            eprintln!("error: fixture discovery: {error}");
            return ExitCode::from(2);
        }
    };
    entries.retain(|path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("test-"))
            && filter.is_none_or(|needle| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.contains(needle))
            })
    });
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("run-parallel"));
    let next_entry = AtomicUsize::new(0);
    let (sender, receiver) = mpsc::channel();
    let mut observations = (0..entries.len()).map(|_| None).collect::<Vec<_>>();
    std::thread::scope(|scope| {
        for _ in 0..jobs.min(entries.len()).max(1) {
            let sender = sender.clone();
            let entries = &entries;
            let exe = &exe;
            let next_entry = &next_entry;
            scope.spawn(move || loop {
                let index = next_entry.fetch_add(1, Ordering::Relaxed);
                let Some(path) = entries.get(index) else {
                    break;
                };
                let observation =
                    observe_parallel_case(exe, path, Duration::from_secs(timeout_secs));
                if sender.send((index, observation)).is_err() {
                    break;
                }
            });
        }
        drop(sender);
        for (index, observation) in receiver {
            observations[index] = Some(observation);
        }
    });
    let mut counts = [0usize; RunResult::COUNT];
    let mut results = Vec::with_capacity(entries.len());
    for (path, observation) in entries.iter().zip(observations) {
        let observation = observation
            .unwrap_or_else(|| Err("fixture worker did not return a result".into()));
        let result = observation
            .as_ref()
            .map(|record| record.outcome())
            .unwrap_or(RunResult::Unclassified);
        counts[result as usize] += 1;
        results.push((path, observation));
        println!("{:?} {}", result, path.display());
    }
    let inventory_hash = inventory_hash(&entries);
    let [passed, skipped, failed, timeout, crash, unclassified] = counts;
    println!(
        "all: pass={passed} skip={skipped} fail={failed} timeout={timeout} crash={crash} unclassified={unclassified} total={} inventory_hash={inventory_hash:016x}",
        entries.len()
    );
    if let Some(path) = results_path {
        if let Err(error) = write_results(path, &results, inventory_hash, timeout_secs, jobs) {
            eprintln!("error: cannot write {path}: {error}");
            return ExitCode::from(2);
        }
    }
    gate_exit(passed + skipped, entries.len())
}

fn gate_exit(passed: usize, total: usize) -> ExitCode {
    if total != 0 && passed == total {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn write_results(
    path: &str,
    results: &[(
        &PathBuf,
        Result<quench_node_test::case_process::CaseObservation, String>,
    )],
    inventory_hash: u64,
    timeout_secs: u64,
    jobs: usize,
) -> std::io::Result<()> {
    let records: Vec<_> = results
        .iter()
        .map(|(fixture, observation)| {
            let result = observation
                .as_ref()
                .map(|record| record.outcome())
                .unwrap_or(RunResult::Unclassified);
            serde_json::json!({"fixture":fixture,"status":result.label(),"observation":observation})
        })
        .collect();
    let report = serde_json::json!({
        "schema_version":2,"inventory_hash":format!("{inventory_hash:016x}"),"timeout_secs":timeout_secs,"jobs":jobs,
        "node_version":command_output("node", &["--version"]),
        "runtime_commit":command_output("git", &["rev-parse", "HEAD"]),
        "tests_node_commit":command_output("git", &["-C", "tests/node", "rev-parse", "HEAD"]),
        "platform":format!("{}-{}",std::env::consts::OS,std::env::consts::ARCH),
        "results":records,
    });
    let bytes = serde_json::to_vec_pretty(&report).map_err(std::io::Error::other)?;
    std::fs::write(path, bytes)
}

fn inventory_hash(entries: &[PathBuf]) -> u64 {
    entries.iter().fold(0xcbf29ce484222325u64, |hash, path| {
        let hash = path.to_string_lossy().bytes().fold(hash, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
        });
        std::fs::read(path)
            .unwrap_or_default()
            .into_iter()
            .fold(hash, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
            })
    })
}

fn command_output(program: &str, args: &[&str]) -> String {
    std::process::Command::new(program)
        .args(args)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|output| !output.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}
