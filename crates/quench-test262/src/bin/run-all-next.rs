use std::{
    collections::{BTreeMap, HashSet},
    env, fs,
    io::Read,
    path::{Path, PathBuf},
    process::{ChildStderr, Command, ExitCode, Stdio},
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};

use quench_test262::{
    HarnessCache, ResolvedStage, RuntimeNextHost, StageReport, Test262Runner, TestOutcome,
    discover_js_files, resolve_stages,
};
use wait_timeout::ChildExt;

const DEFAULT_REPORT: &str = "target/test262-next-report.json";
const DEFAULT_RATCHET: &str = "target/test262-next-ratchet.json";
const METADATA_TEST_BASENAMES: [&str; 4] = [
    "name.js",
    "length.js",
    "prop-desc.js",
    "not-a-constructor.js",
];

struct RatchetResult {
    verdict: &'static str,
    regressions: Vec<String>,
}

fn main() -> ExitCode {
    if let Err(error) = required_timeout_ms() {
        return fail(error);
    }
    if env::args().nth(1).as_deref() == Some("--case") {
        return match thread::Builder::new()
            .name("next-test262-case".into())
            .stack_size(rqj::WORKER_STACK_SIZE)
            .spawn(run_case_entry)
        {
            Ok(worker) => worker.join().unwrap_or(ExitCode::from(1)),
            Err(error) => fail(format!("next test worker thread: {error}")),
        };
    }
    let handle = thread::Builder::new()
        .name("run-all-next-main".into())
        .stack_size(rqj::WORKER_STACK_SIZE)
        .spawn(run)
        .unwrap_or_else(|error| panic!("run-all-next thread: {error}"));
    handle.join().unwrap_or(ExitCode::from(1))
}

fn run() -> ExitCode {
    let root = env::var_os("TEST262_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("tests/test262"));
    let all_files = match discover_js_files(root.join("test")) {
        Ok(files) => files,
        Err(error) => return fail(error),
    };
    let discovered = all_files.len();
    let files = match select_batch(all_files) {
        Ok(files) => files,
        Err(error) => return fail(error),
    };
    let stages = match resolve_stages(&root) {
        Ok(stages) => stages,
        Err(error) => return fail(error),
    };
    let executable = match env::current_exe() {
        Ok(executable) => executable,
        Err(error) => return fail(format!("run-all-next executable lookup failed: {error}")),
    };
    let timeout = match required_timeout_ms() {
        Ok(timeout) => Duration::from_millis(timeout),
        Err(error) => return fail(error),
    };
    let outcomes = match run_parallel_cases(&executable, &root, &files, timeout) {
        Ok(outcomes) => outcomes,
        Err(error) => return fail(error),
    };
    let report = collect_report(&files, &outcomes);
    let full_inventory = env::var_os("TEST262_BATCH_SIZE").is_none();
    let ratchet = match update_ratchet(&root, &files, &outcomes, timeout, full_inventory) {
        Ok(ratchet) => ratchet,
        Err(error) => return fail(error),
    };
    if let Err(error) = write_report(
        &report, discovered, &root, &stages, &files, &outcomes, &ratchet, timeout,
    ) {
        return fail(error);
    }
    println!(
        "next passed={} failed={} total={} discovered={discovered} ratchet={}",
        report.passed, report.failed, report.total, ratchet.verdict
    );
    if !ratchet.regressions.is_empty() {
        eprintln!("next ratchet regressions={}", ratchet.regressions.len());
    }
    if report.failed == 0 && report.total == files.len() && ratchet.regressions.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn run_parallel_cases(
    executable: &Path,
    root: &Path,
    files: &[PathBuf],
    timeout: Duration,
) -> Result<Vec<Result<(), String>>, String> {
    let cursor = AtomicUsize::new(0);
    let completed = AtomicUsize::new(0);
    let outcomes = Mutex::new((0..files.len()).map(|_| None).collect::<Vec<_>>());
    let jobs = worker_jobs().min(files.len().max(1));
    let mut panicked = false;
    thread::scope(|scope| {
        let handles = (0..jobs)
            .map(|_| {
                let cursor = &cursor;
                let completed = &completed;
                let outcomes = &outcomes;
                scope.spawn(move || {
                    loop {
                        let index = cursor.fetch_add(1, Ordering::Relaxed);
                        let Some(path) = files.get(index) else {
                            break;
                        };
                        let result = run_case_process(executable, root, path, timeout);
                        outcomes.lock().expect("case results poisoned")[index] = Some(result);
                        let count = completed.fetch_add(1, Ordering::Relaxed) + 1;
                        if count % 100 == 0 || count == files.len() {
                            eprintln!("run-all-next progress={count}/{} jobs={jobs}", files.len());
                        }
                    }
                })
            })
            .collect::<Vec<_>>();
        panicked = handles.into_iter().any(|handle| handle.join().is_err());
    });
    if panicked {
        return Err("run-all-next case scheduler thread panicked".into());
    }
    outcomes
        .into_inner()
        .map_err(|_| "run-all-next case results poisoned".to_string())?
        .into_iter()
        .enumerate()
        .map(|(index, outcome)| {
            outcome.ok_or_else(|| {
                format!(
                    "case scheduler omitted result for {}",
                    files[index].display()
                )
            })
        })
        .collect()
}

fn collect_report(files: &[PathBuf], outcomes: &[Result<(), String>]) -> StageReport {
    let mut report = StageReport::default();
    for (path, outcome) in files.iter().zip(outcomes) {
        report.total += 1;
        match outcome {
            Ok(()) => report.passed += 1,
            Err(reason) => {
                report.failed += 1;
                report.failures.push((path.clone(), reason.clone()));
            }
        }
    }
    report
}

fn worker_jobs() -> usize {
    env::var("TEST262_JOBS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|jobs| *jobs > 0)
        .or_else(|| thread::available_parallelism().ok().map(usize::from))
        .unwrap_or(1)
}

fn required_timeout_ms() -> Result<u64, String> {
    env::var("TEST262_TEST_TIMEOUT_MS")
        .map_err(|_| {
            "TEST262_TEST_TIMEOUT_MS must be set to a positive timeout in milliseconds".to_string()
        })?
        .parse::<u64>()
        .ok()
        .filter(|timeout| *timeout > 0)
        .ok_or_else(|| {
            "TEST262_TEST_TIMEOUT_MS must be set to a positive timeout in milliseconds".into()
        })
}

fn run_case_entry() -> ExitCode {
    let Some(path) = env::args_os().nth(2).map(PathBuf::from) else {
        return fail("--case requires a test path".into());
    };
    let root = env::var_os("TEST262_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("tests/test262"));
    let mut runner = Test262Runner::new(RuntimeNextHost::default());
    let mut harness = HarnessCache::new(root.join("harness"));
    match runner.run_file_with_cache(path, &mut harness) {
        Ok(TestOutcome::Pass) => ExitCode::SUCCESS,
        Ok(TestOutcome::Fail { reason }) => fail(reason),
        Err(error) => fail(error),
    }
}

fn run_case_process(
    executable: &Path,
    root: &Path,
    path: &Path,
    timeout: Duration,
) -> Result<(), String> {
    let mut child = Command::new(executable)
        .arg("--case")
        .arg(path)
        .env("TEST262_DIR", root)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Test262 case process spawn failed: {error}"))?;
    let stderr = child.stderr.take().expect("piped Test262 case stderr");
    let stderr_reader = drain_stderr(stderr);
    let status = child
        .wait_timeout(timeout)
        .map_err(|error| format!("Test262 case process wait failed: {error}"))?;
    let Some(status) = status else {
        let _ = child.kill();
        let _ = child.wait();
        let _ = stderr_reader.join();
        return Err(format!("timed_out after {}ms", timeout.as_millis()));
    };
    let stderr = stderr_reader
        .join()
        .map_err(|_| "Test262 case stderr reader panicked".to_string())?
        .map_err(|error| format!("Test262 case stderr read failed: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        let reason = String::from_utf8_lossy(&stderr).trim().to_string();
        Err(if status.code().is_none() {
            format!("case process exited with {status}")
        } else if reason.is_empty() {
            format!("case process exited with {status}")
        } else {
            reason
        })
    }
}

fn drain_stderr(stderr: ChildStderr) -> thread::JoinHandle<std::io::Result<Vec<u8>>> {
    thread::spawn(move || {
        let mut output = Vec::new();
        let mut stderr = stderr;
        stderr.read_to_end(&mut output)?;
        Ok(output)
    })
}

fn select_batch(files: Vec<PathBuf>) -> Result<Vec<PathBuf>, String> {
    let Some(size) = env::var_os("TEST262_BATCH_SIZE") else {
        return Ok(files);
    };
    let size = size
        .to_string_lossy()
        .parse::<usize>()
        .map_err(|_| "TEST262_BATCH_SIZE must be a positive integer".to_string())?;
    if size == 0 {
        return Err("TEST262_BATCH_SIZE must be a positive integer".into());
    }
    let index = env::var("TEST262_BATCH_INDEX")
        .unwrap_or_else(|_| "0".into())
        .parse::<usize>()
        .map_err(|_| "TEST262_BATCH_INDEX must be a non-negative integer".to_string())?;
    let start = index
        .checked_mul(size)
        .ok_or_else(|| "TEST262 batch index overflow".to_string())?;
    if start >= files.len() {
        return Err(format!(
            "TEST262 batch {index} starts at {start}, beyond discovered file count {}",
            files.len()
        ));
    }
    Ok(files.into_iter().skip(start).take(size).collect())
}

fn write_report(
    report: &StageReport,
    discovered: usize,
    root: &Path,
    stages: &[ResolvedStage],
    files: &[PathBuf],
    outcomes: &[Result<(), String>],
    ratchet: &RatchetResult,
    timeout: Duration,
) -> Result<(), String> {
    let path = env::var_os("TEST262_REPORT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_REPORT));
    let test_root = root.join("test");
    let failures = report
        .failures
        .iter()
        .map(|(path, reason)| {
            serde_json::json!({
                "path": report_path(path, &test_root),
                "reason": reason,
            })
        })
        .collect::<Vec<_>>();
    let mut stage_counts = BTreeMap::<String, (usize, usize, BTreeMap<String, usize>)>::new();
    let outcomes = files
        .iter()
        .zip(outcomes)
        .map(|(path, outcome)| {
            let stage = stage_for(path, stages);
            let counts = stage_counts.entry(stage.clone()).or_default();
            counts.0 += 1;
            match outcome {
                Ok(()) => {
                    counts.1 += 1;
                    serde_json::json!({
                        "path": report_path(path, &test_root),
                        "stage": stage,
                        "outcome": "pass",
                    })
                }
                Err(reason) => {
                    let family = normalize_failure(reason);
                    *counts.2.entry(family).or_default() += 1;
                    serde_json::json!({
                        "path": report_path(path, &test_root),
                        "stage": stage,
                        "outcome": classify_outcome(reason),
                        "reason": reason,
                    })
                }
            }
        })
        .collect::<Vec<_>>();
    let mut families = BTreeMap::<String, usize>::new();
    let mut metadata_families = BTreeMap::<String, usize>::new();
    for (_, reason) in &report.failures {
        *families.entry(normalize_failure(reason)).or_default() += 1;
    }
    for (path, _) in &report.failures {
        if let Some(basename) = metadata_test_basename(path) {
            *metadata_families.entry(basename).or_default() += 1;
        }
    }
    let stages = stage_counts
        .into_iter()
        .map(|(stage, (total, passed, families))| {
            (
                stage,
                serde_json::json!({
                    "total": total,
                    "passed": passed,
                    "failed": total - passed,
                    "families": families,
                }),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let binary =
        env::current_exe().map_err(|error| format!("runner executable lookup failed: {error}"))?;
    let (source_revision, source_dirty) = source_provenance();
    let value = serde_json::json!({
        "schema": 1,
        "engine": "next",
        "provenance": {
            "source_revision": source_revision,
            "source_dirty": source_dirty,
            "binary": binary,
            "host": format!("{}-{}", env::consts::OS, env::consts::ARCH),
            "timeout_ms": timeout.as_millis(),
            "jobs": worker_jobs(),
        },
        "discovered": discovered,
        "total": report.total,
        "passed": report.passed,
        "failed": report.failed,
        "failures": failures,
        "families": families,
        "metadata_families": metadata_families,
        "stages": stages,
        "outcomes": outcomes,
        "ratchet": ratchet.verdict,
        "regressions": ratchet.regressions,
    });
    write_json(path, &value)
}

fn report_path(path: &Path, test_root: &Path) -> String {
    path.strip_prefix(test_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn metadata_test_basename(path: &Path) -> Option<String> {
    let basename = path.file_name()?.to_str()?;
    METADATA_TEST_BASENAMES
        .contains(&basename)
        .then(|| basename.to_owned())
}

fn stage_for(path: &Path, stages: &[ResolvedStage]) -> String {
    stages
        .iter()
        .find(|stage| stage.owns_file(path, stages))
        .map_or_else(|| "unassigned".into(), |stage| stage.id.to_string())
}

fn classify_outcome(reason: &str) -> &'static str {
    if reason.starts_with("timed_out") {
        "timed_out"
    } else if reason.contains("process exited")
        || reason.contains("signal:")
        || reason.contains("panicked at")
    {
        "crashed"
    } else {
        "failed"
    }
}

fn normalize_failure(reason: &str) -> String {
    let mut normalized = String::with_capacity(reason.len());
    let mut chars = reason.chars().peekable();
    while let Some(character) = chars.next() {
        if character.is_ascii_digit() {
            if !normalized.ends_with('#') {
                normalized.push('#');
            }
            while chars.peek().is_some_and(char::is_ascii_digit) {
                chars.next();
            }
        } else if let Some((close, replacement)) = match character {
            '\'' => Some(('\'', "'…'")),
            '"' => Some(('"', "\"…\"")),
            '«' => Some(('»', "«…»")),
            _ => None,
        } {
            normalized.push_str(replacement);
            for next in chars.by_ref() {
                if next == close {
                    break;
                }
            }
        } else {
            normalized.push(character);
        }
    }
    normalized
}

fn update_ratchet(
    root: &Path,
    files: &[PathBuf],
    outcomes: &[Result<(), String>],
    timeout: Duration,
    full_inventory: bool,
) -> Result<RatchetResult, String> {
    let path = env::var_os("TEST262_RATCHET")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_RATCHET));
    update_ratchet_at(&path, root, files, outcomes, timeout, full_inventory)
}

fn update_ratchet_at(
    path: &Path,
    root: &Path,
    files: &[PathBuf],
    outcomes: &[Result<(), String>],
    timeout: Duration,
    full_inventory: bool,
) -> Result<RatchetResult, String> {
    if !full_inventory {
        return Ok(RatchetResult {
            verdict: "skipped_partial",
            regressions: Vec::new(),
        });
    }
    let had_baseline = path.exists();
    let test_root = root.join("test");
    let current_passes = files
        .iter()
        .zip(outcomes)
        .filter_map(|(path, outcome)| outcome.is_ok().then(|| report_path(path, &test_root)))
        .collect::<HashSet<_>>();
    let mut regressions = Vec::new();
    if path.exists() {
        let contents = fs::read_to_string(&path)
            .map_err(|error| format!("read Test262 ratchet {}: {error}", path.display()))?;
        let baseline: serde_json::Value = serde_json::from_str(&contents)
            .map_err(|error| format!("parse Test262 ratchet {}: {error}", path.display()))?;
        if baseline["schema"] != 1 || baseline["engine"] != "next" {
            return Err(format!(
                "Test262 ratchet {} has an unsupported schema or engine",
                path.display()
            ));
        }
        let passes = baseline["passes"]
            .as_array()
            .ok_or_else(|| format!("Test262 ratchet {} has no pass set", path.display()))?;
        if outcomes.len() == files.len() && !files.is_empty() {
            regressions = newly_failing(passes, &current_passes)?;
        }
    }
    let clean =
        outcomes.len() == files.len() && outcomes.iter().all(Result::is_ok) && !files.is_empty();
    if regressions.is_empty() && clean {
        let mut passes = current_passes.into_iter().collect::<Vec<_>>();
        passes.sort();
        let (revision, source_dirty) = source_provenance();
        let binary = env::current_exe()
            .map_err(|error| format!("runner executable lookup failed: {error}"))?;
        let baseline = serde_json::json!({
            "schema": 1,
            "engine": "next",
            "provenance": {
                "source_revision": revision,
                "source_dirty": source_dirty,
                "binary": binary,
                "host": format!("{}-{}", env::consts::OS, env::consts::ARCH),
                "timeout_ms": timeout.as_millis(),
                "jobs": worker_jobs(),
            },
            "discovered": files.len(),
            "passes": passes,
        });
        write_json(path.to_path_buf(), &baseline)?;
    }
    let verdict = if !regressions.is_empty() {
        "regressed"
    } else if clean && had_baseline {
        "clean"
    } else if clean {
        "baseline_created"
    } else if had_baseline {
        "incomplete"
    } else {
        "awaiting_baseline"
    };
    Ok(RatchetResult {
        verdict,
        regressions,
    })
}

fn source_provenance() -> (String, bool) {
    let revision = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_else(|| "unknown".into());
    let dirty = Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=no"])
        .output()
        .ok()
        .is_none_or(|output| !output.status.success() || !output.stdout.is_empty());
    (revision, dirty)
}

fn newly_failing(
    baseline: &[serde_json::Value],
    current_passes: &HashSet<String>,
) -> Result<Vec<String>, String> {
    let mut regressions = baseline
        .iter()
        .map(|path| {
            path.as_str()
                .ok_or_else(|| "Test262 ratchet pass set contains a non-string path".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|path| !current_passes.contains(*path))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    regressions.sort();
    Ok(regressions)
}

fn write_json(path: PathBuf, value: &serde_json::Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("create {}: {error}", parent.display()))?;
    }
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, format!("{value}\n"))
        .map_err(|error| format!("write {}: {error}", temporary.display()))?;
    fs::rename(&temporary, &path).map_err(|error| {
        format!(
            "replace Test262 report {} with {}: {error}",
            path.display(),
            temporary.display()
        )
    })
}

fn fail(error: String) -> ExitCode {
    eprintln!("FAIL: {error}");
    ExitCode::from(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pass_set_ratchet_finds_only_lost_passes() {
        let baseline = ["a.js", "b.js", "c.js"]
            .into_iter()
            .map(|path| serde_json::Value::String(path.into()))
            .collect::<Vec<_>>();
        let current = HashSet::from(["a.js".to_string(), "c.js".to_string(), "new.js".to_string()]);

        assert_eq!(newly_failing(&baseline, &current).unwrap(), ["b.js"]);
    }

    #[test]
    fn failure_families_normalize_values_and_numbers() {
        assert_eq!(
            normalize_failure("Expected SameValue(«17», «false») at 'fixture-42.js'"),
            "Expected SameValue(«…», «…») at '…'"
        );
    }

    #[test]
    fn metadata_failures_cluster_by_their_specified_basename() {
        assert_eq!(
            metadata_test_basename(Path::new("test/built-ins/Array/prototype/name.js")),
            Some("name.js".into())
        );
        assert_eq!(
            metadata_test_basename(Path::new("test/built-ins/TypedArray/prototype/length.js")),
            Some("length.js".into())
        );
        assert_eq!(
            metadata_test_basename(Path::new("test/built-ins/Array/prop-desc.js")),
            Some("prop-desc.js".into())
        );
        assert_eq!(
            metadata_test_basename(Path::new("test/built-ins/Array/prototype/set.js")),
            None
        );
        assert_eq!(
            metadata_test_basename(Path::new("test/built-ins/Array/not-a-constructor.js")),
            Some("not-a-constructor.js".into())
        );
    }

    #[test]
    fn case_failures_keep_timeout_and_crash_categories() {
        assert_eq!(classify_outcome("timed_out after 15ms"), "timed_out");
        assert_eq!(
            classify_outcome("case process exited with signal: 11"),
            "crashed"
        );
        assert_eq!(classify_outcome("next runtime: TypeError"), "failed");
    }

    #[cfg(unix)]
    #[test]
    fn stderr_is_drained_while_a_case_process_is_running() {
        use std::process::Command;

        let mut child = Command::new("sh")
            .args(["-c", "head -c 131072 /dev/zero >&2"])
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stderr_reader = drain_stderr(child.stderr.take().unwrap());
        assert!(
            child
                .wait_timeout(Duration::from_secs(5))
                .unwrap()
                .is_some()
        );
        let stderr = stderr_reader.join().unwrap().unwrap();
        assert_eq!(stderr.len(), 131072);
    }

    #[test]
    fn ratchet_preserves_baseline_and_names_lost_passes() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = env::temp_dir().join(format!("quench-test262-ratchet-{unique}"));
        let baseline = directory.join("ratchet.json");
        let root = directory.join("suite");
        let files = [root.join("test/a.js"), root.join("test/b.js")];
        let timeout = Duration::from_millis(900_000);

        let first =
            update_ratchet_at(&baseline, &root, &files, &[Ok(()), Ok(())], timeout, true).unwrap();
        assert_eq!(first.verdict, "baseline_created");
        assert!(first.regressions.is_empty());
        let mut saved: serde_json::Value =
            serde_json::from_slice(&fs::read(&baseline).unwrap()).unwrap();
        saved["test_marker"] = serde_json::json!("keep on incomplete run");
        fs::write(&baseline, format!("{saved}\n")).unwrap();

        let truncated =
            update_ratchet_at(&baseline, &root, &files, &[Ok(())], timeout, true).unwrap();
        assert_eq!(truncated.verdict, "incomplete");
        assert!(truncated.regressions.is_empty());

        let added_file = root.join("test/c.js");
        let incomplete = update_ratchet_at(
            &baseline,
            &root,
            &[files[0].clone(), files[1].clone(), added_file],
            &[Ok(()), Ok(()), Err("new test failed".into())],
            timeout,
            true,
        )
        .unwrap();
        assert_eq!(incomplete.verdict, "incomplete");
        assert!(incomplete.regressions.is_empty());
        let saved: serde_json::Value =
            serde_json::from_slice(&fs::read(&baseline).unwrap()).unwrap();
        assert_eq!(saved["test_marker"], "keep on incomplete run");

        let second = update_ratchet_at(
            &baseline,
            &root,
            &files,
            &[Ok(()), Err("expected failure".into())],
            timeout,
            true,
        )
        .unwrap();
        assert_eq!(second.verdict, "regressed");
        assert_eq!(second.regressions, ["b.js"]);

        let contents = fs::read_to_string(&baseline).unwrap();
        let saved: serde_json::Value = serde_json::from_str(&contents).unwrap();
        assert_eq!(saved["passes"], serde_json::json!(["a.js", "b.js"]));
        fs::remove_dir_all(directory).unwrap();
    }
}
