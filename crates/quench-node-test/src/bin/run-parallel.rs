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

use std::collections::HashMap;
use std::io::{BufWriter, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use quench_node_test::case_process::{
    cleanup_adopted_children, observe_parallel_case, worker_entry, RunResult,
    DEFAULT_CASE_TIMEOUT_SECS,
};

const PARALLEL_DIR: &str = "tests/node/test/parallel";

fn main() -> ExitCode {
    #[cfg(target_os = "linux")]
    if let Err(error) = quench_node::shared_run::initialize_cli_process_signals() {
        eprintln!("initialize CLI process signals: {error}");
        return std::process::ExitCode::FAILURE;
    }

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
        let checkpoint = args
            .iter()
            .position(|a| a == "--checkpoint")
            .and_then(|i| args.get(i + 1));
        let resume = args.iter().any(|a| a == "--resume");
        let root = match args.iter().position(|arg| arg == "--parallel-dir") {
            Some(index) => match args.get(index + 1) {
                Some(path) => PathBuf::from(path),
                None => {
                    eprintln!("error: --parallel-dir requires a directory");
                    return ExitCode::from(2);
                }
            },
            None => PathBuf::from(PARALLEL_DIR),
        };
        return run_all(filter, timeout, jobs, results, checkpoint, resume, root);
    }
    let profile = args
        .iter()
        .position(|arg| arg == "--profile")
        .and_then(|index| args.get(index + 1));
    if args.iter().any(|arg| arg == "--profile") && profile.is_none() {
        eprintln!("error: --profile requires a profile name");
        return ExitCode::from(2);
    }
    let timeout = args
        .iter()
        .position(|arg| arg == "--timeout-secs")
        .and_then(|index| args.get(index + 1))
        .and_then(|value| value.parse().ok())
        .unwrap_or(DEFAULT_CASE_TIMEOUT_SECS);
    run_manifest(profile.map(String::as_str), timeout)
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
    println!("  --timeout-secs N                     isolate fixtures for N seconds (default 30)");
    println!();
    println!("options for --all:");
    println!("  --parallel-dir PATH use another unchanged Node parallel checkout");
    println!("  --filter NAME       restrict fixtures by filename");
    println!("  --timeout-secs N    isolate each fixture with an N-second timeout (default 30)");
    println!("  --jobs N            run up to N isolated fixtures concurrently (default: available CPUs)");
    println!("  --results PATH      write machine-readable results and inventory hash");
    println!("  --checkpoint PATH   persist each completed fixture as JSONL");
    println!("  --resume            continue completed fixtures from --checkpoint");
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

fn run_manifest(profile: Option<&str>, timeout_secs: u64) -> ExitCode {
    quench_node_test::parallel_profile::run(profile, None, timeout_secs)
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
    checkpoint_path: Option<&String>,
    resume: bool,
    root: PathBuf,
) -> ExitCode {
    let runtime_commit = command_output("git", &["rev-parse", "HEAD"]);
    let tests_node_commit =
        command_output("git", &["-C", &root.to_string_lossy(), "rev-parse", "HEAD"]);
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
    let inventory_hash = inventory_hash(&entries);
    if resume && checkpoint_path.is_none() {
        eprintln!("error: --resume requires --checkpoint PATH");
        return ExitCode::from(2);
    }
    let mut observations = (0..entries.len()).map(|_| None).collect::<Vec<_>>();
    if resume {
        let path = checkpoint_path.expect("resume requires checkpoint path");
        match load_checkpoint(path, inventory_hash, &entries) {
            Ok(restored) => {
                for (index, observation) in restored {
                    observations[index] = Some(observation);
                }
            }
            Err(error) => {
                eprintln!("error: cannot resume {path}: {error}");
                return ExitCode::from(2);
            }
        }
    }
    let pending_indices = observations
        .iter()
        .enumerate()
        .filter_map(|(index, observation)| observation.is_none().then_some(index))
        .collect::<Vec<_>>();
    let mut checkpoint_writer = match checkpoint_path {
        Some(path) => match std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            Ok(file) => Some(BufWriter::new(file)),
            Err(error) => {
                eprintln!("error: cannot open checkpoint {path}: {error}");
                return ExitCode::from(2);
            }
        },
        None => None,
    };
    let mut checkpoint_error = None;
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("run-parallel"));
    let next_entry = AtomicUsize::new(0);
    let (sender, receiver) = mpsc::channel();
    std::thread::scope(|scope| {
        for _ in 0..jobs.min(pending_indices.len()).max(1) {
            let sender = sender.clone();
            let entries = &entries;
            let pending_indices = &pending_indices;
            let exe = &exe;
            let next_entry = &next_entry;
            scope.spawn(move || loop {
                let pending_index = next_entry.fetch_add(1, Ordering::Relaxed);
                let Some(&index) = pending_indices.get(pending_index) else {
                    break;
                };
                let path = &entries[index];
                let observation =
                    observe_parallel_case(exe, path, Duration::from_secs(timeout_secs));
                if sender.send((index, observation)).is_err() {
                    break;
                }
            });
        }
        drop(sender);
        for (index, observation) in receiver {
            let result = observation
                .as_ref()
                .map(|record| record.outcome())
                .unwrap_or(RunResult::Unclassified);
            println!("{:?} {}", result, entries[index].display());
            if let Some(writer) = checkpoint_writer.as_mut() {
                if let Err(error) =
                    write_checkpoint_record(writer, inventory_hash, &entries[index], &observation)
                {
                    checkpoint_error.get_or_insert(error.to_string());
                }
            }
            observations[index] = Some(observation);
        }
    });
    cleanup_adopted_children();
    if let Some(error) = checkpoint_error {
        eprintln!("error: cannot write checkpoint: {error}");
        return ExitCode::from(2);
    }
    let mut counts = [0usize; RunResult::COUNT];
    let mut results = Vec::with_capacity(entries.len());
    for (path, observation) in entries.iter().zip(observations) {
        let observation =
            observation.unwrap_or_else(|| Err("fixture worker did not return a result".into()));
        let result = observation
            .as_ref()
            .map(|record| record.outcome())
            .unwrap_or(RunResult::Unclassified);
        counts[result as usize] += 1;
        results.push((path, observation));
    }
    let [passed, skipped, failed, timeout, crash, unclassified] = counts;
    println!(
        "all: pass={passed} skip={skipped} fail={failed} timeout={timeout} crash={crash} unclassified={unclassified} total={} inventory_hash={inventory_hash:016x}",
        entries.len()
    );
    if let Some(path) = results_path {
        if let Err(error) = write_results(
            path,
            &results,
            inventory_hash,
            timeout_secs,
            jobs,
            &runtime_commit,
            &tests_node_commit,
        ) {
            eprintln!("error: cannot write {path}: {error}");
            return ExitCode::from(2);
        }
    }
    gate_exit(passed + skipped, entries.len())
}

fn write_checkpoint_record(
    writer: &mut BufWriter<std::fs::File>,
    inventory_hash: u64,
    fixture: &PathBuf,
    observation: &Result<quench_node_test::case_process::CaseObservation, String>,
) -> std::io::Result<()> {
    let record = serde_json::json!({
        "inventory_hash": format!("{inventory_hash:016x}"),
        "fixture": fixture,
        "observation": observation,
    });
    serde_json::to_writer(&mut *writer, &record).map_err(std::io::Error::other)?;
    writer.write_all(b"\n")?;
    writer.flush()
}

fn load_checkpoint(
    path: &str,
    inventory_hash: u64,
    entries: &[PathBuf],
) -> Result<
    Vec<(
        usize,
        Result<quench_node_test::case_process::CaseObservation, String>,
    )>,
    String,
> {
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.to_string()),
    };
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    let complete_len = bytes
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |index| index + 1);
    let index_by_path = entries
        .iter()
        .enumerate()
        .map(|(index, path)| (path.to_string_lossy().into_owned(), index))
        .collect::<HashMap<_, _>>();
    let expected_hash = format!("{inventory_hash:016x}");
    let mut restored = HashMap::new();
    for (line_index, line) in bytes[..complete_len]
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .enumerate()
    {
        let line = std::str::from_utf8(line)
            .map_err(|error| format!("checkpoint line {}: {error}", line_index + 1))?;
        let record: serde_json::Value = serde_json::from_str(&line)
            .map_err(|error| format!("checkpoint line {}: {error}", line_index + 1))?;
        if record
            .get("inventory_hash")
            .and_then(serde_json::Value::as_str)
            != Some(expected_hash.as_str())
        {
            return Err(format!(
                "inventory hash mismatch on line {}",
                line_index + 1
            ));
        }
        let fixture = record
            .get("fixture")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| format!("missing fixture on line {}", line_index + 1))?;
        let Some(&index) = index_by_path.get(fixture) else {
            continue;
        };
        let observation: Result<quench_node_test::case_process::CaseObservation, String> =
            serde_json::from_value(
                record
                    .get("observation")
                    .cloned()
                    .ok_or_else(|| format!("missing observation on line {}", line_index + 1))?,
            )
            .map_err(|error| format!("checkpoint line {}: {error}", line_index + 1))?;
        restored.insert(index, observation);
    }
    Ok(restored.into_iter().collect())
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
    runtime_commit: &str,
    tests_node_commit: &str,
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
        "node_version":node_version_output(),
        "runtime_commit":runtime_commit,
        "tests_node_commit":tests_node_commit,
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

fn node_version_output() -> String {
    if let Some(executable) = std::env::var_os("QUENCH_NODE_TEST_EXECUTABLE") {
        return std::process::Command::new(executable)
            .arg("--version")
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
            .filter(|output| !output.is_empty())
            .unwrap_or_else(|| "unknown".to_string());
    }
    command_output("node", &["--version"])
}
