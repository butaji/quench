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

const RESULT_MARKER: &str = "__QUENCH_RESULT__";

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
        .position(|arg| arg == "--one")
        .and_then(|index| args.get(index + 1))
    {
        let timeout = args
            .iter()
            .position(|arg| arg == "--timeout-secs")
            .and_then(|index| args.get(index + 1))
            .and_then(|value| value.parse().ok())
            .unwrap_or(30);
        return run_one(PathBuf::from(path), timeout);
    }
    if let Some(path) = args
        .iter()
        .position(|a| a == "--triage-one")
        .and_then(|i| args.get(i + 1))
    {
        // Child mode: run one fixture so a fatal abort (e.g. stack
        // overflow) cannot take down the parent triage sweep.
        let outcome = quench_node_test::NodeTestRunner::new().run_file(&PathBuf::from(path));
        let (status, code) = match outcome {
            quench_node_test::NodeOutcome::Pass => ("pass", ExitCode::SUCCESS),
            quench_node_test::NodeOutcome::Skip { .. } => ("skip", ExitCode::SUCCESS),
            quench_node_test::NodeOutcome::Fail { .. } => ("fail", ExitCode::from(1)),
        };
        println!("{RESULT_MARKER} {status}");
        return code;
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
            .unwrap_or(30);
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
    println!("  run-parallel --one PATH               run one fixture");
    println!("  run-parallel --all [options]         run the recursive fixture inventory");
    println!("  run-parallel --triage [options]        print passing triage fixtures");
    println!();
    println!("options for --all:");
    println!("  --filter NAME       restrict fixtures by filename");
    println!("  --timeout-secs N    isolate each fixture with an N-second timeout (default 30)");
    println!("  --results PATH      write machine-readable results and inventory hash");
}

fn run_one(path: PathBuf, timeout_secs: u64) -> ExitCode {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("run-parallel"));
    match triage_one(&exe, &path, timeout_secs) {
        RunResult::Pass => {
            println!("PASS {}", path.display());
            ExitCode::SUCCESS
        }
        RunResult::Skip => {
            println!("SKIP {}", path.display());
            ExitCode::SUCCESS
        }
        result => {
            println!("{} {}", result.label().to_uppercase(), path.display());
            ExitCode::from(1)
        }
    }
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
    let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    if let Err(error) = validate_manifest(&names, &root.join(PARALLEL_DIR)) {
        eprintln!("invalid {MANIFEST}: {error}");
        return ExitCode::from(2);
    }
    let mut counts = [0usize; 6];
    // Resolve fixture paths against the startup CWD and isolate every fixture
    // in a child process. A manifest entry must not be able to retain module
    // state, change the runner's CWD, crash the gate, or hang it indefinitely.
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("run-parallel"));
    for name in &names {
        let path = root.join(PARALLEL_DIR).join(name);
        let result = triage_one(&exe, &path, 30);
        counts[result as usize] += 1;
        println!("{}  {name}", result.label().to_uppercase());
    }
    println!(
        "parallel: pass={} skip={} fail={} timeout={} crash={} unclassified={} total={}",
        counts[0],
        counts[1],
        counts[2],
        counts[3],
        counts[4],
        counts[5],
        names.len()
    );
    if counts[2..].iter().all(|count| *count == 0) {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn validate_manifest(names: &[String], parallel_root: &std::path::Path) -> Result<(), String> {
    let mut seen = std::collections::HashSet::with_capacity(names.len());
    for name in names {
        if !seen.insert(name) {
            return Err(format!("duplicate fixture entry: {name}"));
        }
        let path = parallel_root.join(name);
        if !path.is_file() {
            return Err(format!("fixture does not exist: {name}"));
        }
        if !matches!(
            path.extension().and_then(|value| value.to_str()),
            Some("js" | "mjs" | "cjs")
        ) {
            return Err(format!("fixture has unsupported extension: {name}"));
        }
    }
    Ok(())
}

fn triage(filter: Option<&String>, timeout_secs: u64) -> ExitCode {
    if !std::path::Path::new(PARALLEL_DIR).is_dir() {
        eprintln!(
            "error: upstream Node fixture directory is missing: {PARALLEL_DIR}\n\
             initialize the tests/node submodule before running triage"
        );
        return ExitCode::from(2);
    }
    let mut entries: Vec<PathBuf> =
        quench_node_test::stages::discover_fixtures(&PathBuf::from(PARALLEL_DIR))
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
        if matches!(triage_one(&exe, path, timeout_secs), RunResult::Pass) {
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
    Skip,
    Fail,
    Timeout,
    Crash,
    Unclassified,
}

impl RunResult {
    fn label(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Skip => "skip",
            Self::Fail => "fail",
            Self::Timeout => "timeout",
            Self::Crash => "crash",
            Self::Unclassified => "unclassified",
        }
    }
}

fn triage_one(exe: &PathBuf, path: &PathBuf, timeout_secs: u64) -> RunResult {
    if !path.is_file() {
        return RunResult::Unclassified;
    }
    use std::io::Read;
    use std::process::{Command, Stdio};
    let mut command = Command::new(exe);
    command
        .arg("--triage-one")
        .arg(path)
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            command.pre_exec(|| {
                // Keep each fixture in a disposable process group and detach
                // it from the runner's controlling terminal.  Otherwise a
                // gate launched from a TTY makes optional `/dev/tty` branches
                // observable only in manifest mode (the host cannot provide
                // a real Node TTY), producing environment-dependent results.
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    let Ok(mut child) = command.spawn() else {
        return RunResult::Crash;
    };
    for _ in 0..timeout_secs.saturating_mul(20) {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut output = String::new();
                if let Some(mut stdout) = child.stdout.take() {
                    let _ = stdout.read_to_string(&mut output);
                }
                let marker = output
                    .lines()
                    .find_map(|line| line.strip_prefix(RESULT_MARKER).map(str::trim));
                return match marker {
                    Some("pass") if status.success() => RunResult::Pass,
                    Some("skip") if status.success() => RunResult::Skip,
                    Some("fail") if !status.success() => RunResult::Fail,
                    Some(_) | None if status.code().is_none() => RunResult::Crash,
                    Some(_) | None => RunResult::Unclassified,
                };
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(50)),
            Err(_) => return RunResult::Crash,
        }
    }
    #[cfg(unix)]
    unsafe {
        libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
    }
    #[cfg(not(unix))]
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
    let mut counts = [0usize; 6];
    let mut results = Vec::with_capacity(entries.len());
    for path in &entries {
        let result = triage_one(&exe, path, timeout_secs);
        counts[result as usize] += 1;
        results.push((path, result));
        println!("{:?} {}", result, path.display());
    }
    let inventory_hash = inventory_hash(&entries);
    let [passed, skipped, failed, timeout, crash, unclassified] = counts;
    println!(
        "all: pass={passed} skip={skipped} fail={failed} timeout={timeout} crash={crash} unclassified={unclassified} total={} inventory_hash={inventory_hash:016x}",
        entries.len()
    );
    if let Some(path) = results_path {
        if let Err(error) = write_results(path, &results, inventory_hash, timeout_secs) {
            eprintln!("error: cannot write {path}: {error}");
            return ExitCode::from(2);
        }
    }
    if failed == 0 && timeout == 0 && crash == 0 && unclassified == 0 {
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
    let node_version = command_output("node", &["--version"]);
    let runtime_commit = command_output("git", &["rev-parse", "HEAD"]);
    let tests_node_commit = command_output("git", &["-C", "tests/node", "rev-parse", "HEAD"]);
    let platform = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
    writeln!(
        file,
        "{{\"schema_version\":2,\"inventory_hash\":\"{inventory_hash:016x}\",\"timeout_secs\":{timeout_secs},\"node_version\":\"{}\",\"runtime_commit\":\"{}\",\"tests_node_commit\":\"{}\",\"platform\":\"{}\",\"results\":[",
        json_escape(&node_version),
        json_escape(&runtime_commit),
        json_escape(&tests_node_commit),
        json_escape(&platform),
    )?;
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

fn json_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

#[cfg(test)]
mod tests {
    use super::validate_manifest;
    use std::fs;

    #[test]
    fn manifest_validation_rejects_duplicates_and_unknown_extensions() {
        let root =
            std::env::temp_dir().join(format!("quench-node-manifest-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("test-a.js"), "").unwrap();
        fs::write(root.join("test-b.txt"), "").unwrap();
        let duplicate = vec!["test-a.js".to_string(), "test-a.js".to_string()];
        assert!(validate_manifest(&duplicate, &root).is_err());
        let unsupported = vec!["test-b.txt".to_string()];
        assert!(validate_manifest(&unsupported, &root).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn manifest_validation_accepts_all_fixture_extensions() {
        let root =
            std::env::temp_dir().join(format!("quench-node-manifest-valid-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        for name in ["test-a.js", "test-b.mjs", "test-c.cjs"] {
            fs::write(root.join(name), "").unwrap();
        }
        let names = ["test-a.js", "test-b.mjs", "test-c.cjs"]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
        assert!(validate_manifest(&names, &root).is_ok());
        fs::remove_dir_all(root).unwrap();
    }
}
