//! Run real Node.js `test/parallel` fixtures from the `tests/node`
//! submodule through the host.
//!
//! Default mode runs the manifest (one test file name per line, `#`
//! comments allowed) and fails if any listed test regresses.
//! `--triage` sweeps the whole `parallel/` directory and prints the
//! tests that pass — diagnostic output for growing the manifest,
//! never a conformance gate.
//!
//! Usage:
//!   cargo run -p quench-node-test --bin run-parallel
//!   cargo run -p quench-node-test --bin run-parallel -- --triage [--filter NAME]

use std::path::PathBuf;
use std::process::ExitCode;

const PARALLEL_DIR: &str = "tests/node/test/parallel";
const MANIFEST: &str = "crates/quench-node-test/node-tests/parallel.txt";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_help();
        return ExitCode::SUCCESS;
    }
    if let Some(path) = args
        .iter()
        .position(|a| a == "--triage-one")
        .and_then(|i| args.get(i + 1))
    {
        // Child mode: run one fixture so a fatal abort (e.g. stack
        // overflow) cannot take down the parent triage sweep.
        return match quench_node_test::NodeTestRunner::new().run_file(&PathBuf::from(path)) {
            quench_node_test::NodeOutcome::Pass => ExitCode::SUCCESS,
            _ => ExitCode::from(1),
        };
    }
    if args.iter().any(|a| a == "--triage") {
        let filter = args
            .iter()
            .position(|a| a == "--filter")
            .and_then(|i| args.get(i + 1));
        return triage(filter);
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
            .unwrap_or(30);
        let results = args
            .iter()
            .position(|a| a == "--results")
            .and_then(|i| args.get(i + 1));
        return run_all(filter, timeout, results);
    }
    run_manifest()
}

fn print_help() {
    println!("run-parallel: execute Node parallel fixtures through quench-node");
    println!();
    println!("usage:");
    println!("  run-parallel                         run the checked-in stage manifest");
    println!("  run-parallel --all [options]         run the recursive fixture inventory");
    println!("  run-parallel --triage [--filter NAME]  print passing triage fixtures");
    println!();
    println!("options for --all:");
    println!("  --filter NAME       restrict fixtures by filename");
    println!("  --timeout-secs N    isolate each fixture with an N-second timeout (default 30)");
    println!("  --results PATH      write machine-readable results and inventory hash");
}

fn manifest_names() -> Vec<String> {
    let manifest = std::fs::read_to_string(MANIFEST).unwrap_or_default();
    manifest
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_string)
        .collect()
}

fn run_manifest() -> ExitCode {
    if !std::path::Path::new(PARALLEL_DIR).is_dir() {
        eprintln!(
            "error: upstream Node fixture directory is missing: {PARALLEL_DIR}\n\
             initialize the tests/node submodule before running run-parallel"
        );
        return ExitCode::from(2);
    }
    let names = manifest_names();
    if names.is_empty() {
        eprintln!("read {MANIFEST}: missing or empty manifest");
        return ExitCode::from(2);
    }
    let mut failed = Vec::new();
    // Resolve fixture paths against the startup CWD: a fixture may call
    // process.chdir (e.g. tmpdir cleanup), which moves the process CWD
    // shared by all fixtures in this process.
    let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    for name in &names {
        let path = root.join(PARALLEL_DIR).join(name);
        // Fresh runner per fixture: host state (module cache, exit
        // handlers, timers) must not leak across tests.
        match quench_node_test::NodeTestRunner::new().run_file(&path) {
            quench_node_test::NodeOutcome::Pass => println!("PASS  {name}"),
            other => {
                println!("FAIL  {name}");
                failed.push(format!("{name}: {other:?}"));
            }
        }
    }
    println!(
        "parallel: {} passed, {} failed, {} total",
        names.len() - failed.len(),
        failed.len(),
        names.len()
    );
    for failure in &failed {
        println!("  - {failure}");
    }
    if failed.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn triage(filter: Option<&String>) -> ExitCode {
    if !std::path::Path::new(PARALLEL_DIR).is_dir() {
        eprintln!(
            "error: upstream Node fixture directory is missing: {PARALLEL_DIR}\n\
             initialize the tests/node submodule before running triage"
        );
        return ExitCode::from(2);
    }
    let mut entries: Vec<PathBuf> = quench_node_test::stages::discover_fixtures(
        &PathBuf::from(PARALLEL_DIR),
    )
    .into_iter()
    .filter(|path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                name.starts_with("test-")
                    && filter.as_ref().map_or(true, |f| name.contains(f.as_str()))
            })
    })
    .collect();
    entries.sort();
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("run-parallel"));
    let mut passed = 0;
    for path in &entries {
        if matches!(triage_one(&exe, path, 30), RunResult::Pass) {
            println!("{}", path.file_name().unwrap().to_string_lossy());
            passed += 1;
        }
    }
    eprintln!("triage: {passed} passed of {}", entries.len());
    ExitCode::SUCCESS
}

/// Run one fixture in a child process (crash isolation) with a
/// 30-second timeout; report pass only on a clean zero exit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RunResult {
    Pass,
    Fail,
    Timeout,
    Crash,
}

impl RunResult {
    fn label(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
            Self::Timeout => "timeout",
            Self::Crash => "crash",
        }
    }
}

fn triage_one(exe: &PathBuf, path: &PathBuf, timeout_secs: u64) -> RunResult {
    use std::process::{Command, Stdio};
    let Ok(mut child) = Command::new(exe)
        .arg("--triage-one")
        .arg(path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return RunResult::Crash;
    };
    for _ in 0..timeout_secs.saturating_mul(20) {
        match child.try_wait() {
            Ok(Some(status)) => {
                return if status.success() {
                    RunResult::Pass
                } else if status.code().is_none() {
                    RunResult::Crash
                } else {
                    RunResult::Fail
                };
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(50)),
            Err(_) => return RunResult::Crash,
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    RunResult::Timeout
}

fn run_all(filter: Option<&String>, timeout_secs: u64, results_path: Option<&String>) -> ExitCode {
    let root = PathBuf::from(PARALLEL_DIR);
    let mut entries = quench_node_test::stages::discover_fixtures(&root);
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
    let mut counts = [0usize; 4];
    let mut results = Vec::with_capacity(entries.len());
    for path in &entries {
        let result = triage_one(&exe, path, timeout_secs);
        counts[result as usize] += 1;
        results.push((path, result));
        println!("{:?} {}", result, path.display());
    }
    let inventory_hash = entries.iter().fold(0xcbf29ce484222325u64, |hash, path| {
        path.to_string_lossy().bytes().fold(hash, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
        })
    });
    let [passed, failed, timeout, crash] = counts;
    println!(
        "all: pass={passed} fail={failed} timeout={timeout} crash={crash} total={} inventory_hash={inventory_hash:016x}",
        entries.len()
    );
    if let Some(path) = results_path {
        if let Err(error) = write_results(path, &results, inventory_hash, timeout_secs) {
            eprintln!("error: cannot write {path}: {error}");
            return ExitCode::from(2);
        }
    }
    if failed == 0 && timeout == 0 && crash == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn write_results(
    path: &str,
    results: &[(&PathBuf, RunResult)],
    inventory_hash: u64,
    timeout_secs: u64,
) -> std::io::Result<()> {
    use std::io::Write;
    let mut file = std::fs::File::create(path)?;
    writeln!(file, "{{\"inventory_hash\":\"{inventory_hash:016x}\",\"timeout_secs\":{timeout_secs},\"results\":[")?;
    for (index, (fixture, result)) in results.iter().enumerate() {
        let comma = if index + 1 == results.len() { "" } else { "," };
        writeln!(
            file,
            "  {{\"fixture\":\"{}\",\"status\":\"{}\"}}{comma}",
            json_escape(&fixture.to_string_lossy()),
            result.label()
        )?;
    }
    writeln!(file, "]}}")
}

fn json_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}
