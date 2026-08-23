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
//!   cargo run -p quench-node-test --bin run-parallel -- --all [--filter NAME]

use std::path::{Path, PathBuf};
use std::process::ExitCode;

const DEFAULT_TIMEOUT_SECS: u64 = 30;

const PARALLEL_REL: &str = "tests/node/test/parallel";
const MANIFEST_REL: &str = "crates/quench-node-test/node-tests/parallel.txt";

fn repo_root() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("node-tests/parallel.txt");
    manifest
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}
fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let timeout_secs = timeout_secs(&args);
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
    if args.iter().any(|a| a == "--all") {
        let filter = args
            .iter()
            .position(|a| a == "--filter")
            .and_then(|i| args.get(i + 1));
        return triage(filter, true, timeout_secs);
    }
    if args.iter().any(|a| a == "--triage") {
        let filter = args
            .iter()
            .position(|a| a == "--filter")
            .and_then(|i| args.get(i + 1));
        return triage(filter, false, timeout_secs);
    }
    run_manifest()
}

fn manifest_names(manifest_path: &Path) -> Result<Vec<String>, String> {
    let manifest = std::fs::read_to_string(manifest_path)
        .map_err(|error| format!("read {}: {error}", manifest_path.display()))?;
    Ok(manifest
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_string)
        .collect())
}

fn report_manifest_failure(
    name: &str,
    outcome: quench_node_test::NodeOutcome,
    failed: &mut Vec<String>,
) {
    println!("FAIL  {name}");
    failed.push(format!("{name}: {outcome:?}"));
}

fn run_manifest() -> ExitCode {
    let root = repo_root();
    let manifest_path = root.join(MANIFEST_REL);
    let names = match manifest_names(&manifest_path) {
        Ok(names) if !names.is_empty() => names,
        Ok(_) => {
            eprintln!("{}: missing or empty manifest", manifest_path.display());
            return ExitCode::from(2);
        }
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };
    let mut failed = Vec::new();
    let parallel_dir = root.join(PARALLEL_REL);
    for name in &names {
        let path = parallel_dir.join(name);
        match quench_node_test::NodeTestRunner::new().run_file(&path) {
            quench_node_test::NodeOutcome::Pass => println!("PASS  {name}"),
            other => report_manifest_failure(name, other, &mut failed),
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

fn timeout_secs(args: &[String]) -> u64 {
    args.iter()
        .position(|arg| arg == "--timeout-secs")
        .and_then(|index| args.get(index + 1))
        .and_then(|value| value.parse().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_TIMEOUT_SECS)
}

fn triage(filter: Option<&String>, gate: bool, timeout_secs: u64) -> ExitCode {
    let parallel_dir = repo_root().join(PARALLEL_REL);
    let mut entries: Vec<PathBuf> = quench_node_test::stages::discover_fixtures(&parallel_dir)
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
        if triage_one(&exe, path, timeout_secs) {
            println!("{}", path.file_name().unwrap().to_string_lossy());
            passed += 1;
        }
    }
    eprintln!("parallel: {passed} passed of {}", entries.len());
    if gate && passed != entries.len() {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

/// Run one fixture in a child process (crash isolation) with a
/// 30-second timeout; report pass only on a clean zero exit.
fn triage_one(exe: &PathBuf, path: &PathBuf, timeout_secs: u64) -> bool {
    use std::process::{Command, Stdio};
    let Ok(mut child) = Command::new(exe)
        .arg("--triage-one")
        .arg(path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(50))
            }
            Err(_) => return false,
            Ok(None) => break,
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    false
}
