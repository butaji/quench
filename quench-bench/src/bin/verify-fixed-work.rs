//! Replay frozen instruction-guard inputs; a process exit alone is not completion.
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, VecDeque},
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Mutex,
    thread,
    time::{Duration, Instant},
};

const WORKERS: usize = 2;
const CASE_TIMEOUT: Duration = Duration::from_secs(60);
const CALLGRIND_TIMEOUT: Duration = Duration::from_secs(600);
const INSTRUCTION_LIMIT_PERCENT: f64 = 1.0;
const POLL_INTERVAL: Duration = Duration::from_millis(10);

#[derive(Clone)]
struct Case {
    fixture: String,
    runs: usize,
    path: PathBuf,
    sha256: String,
}
#[derive(Clone)]
struct Job {
    binary_index: usize,
    binary: PathBuf,
    case: Case,
}
fn hash(path: &Path) -> Result<String, String> {
    let output = Command::new("sha256sum")
        .stdin(Stdio::from(
            fs::File::open(path).map_err(|e| e.to_string())?,
        ))
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(format!("hash failed for {}", path.display()));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let hash = text.split_whitespace().next().unwrap_or("");
    if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("invalid SHA256 output".into());
    }
    Ok(hash.to_owned())
}
fn inventory(input: &Path) -> Result<Vec<Case>, String> {
    let mut groups: BTreeMap<String, BTreeMap<usize, PathBuf>> = BTreeMap::new();
    for entry in fs::read_dir(input).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().and_then(|s| s.to_str()) != Some("js") {
            continue;
        }
        let Some((fixture, count)) = path
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(|s| s.rsplit_once('-'))
        else {
            continue;
        };
        let Ok(count) = count.parse::<usize>() else {
            continue;
        };
        if groups
            .entry(fixture.to_owned())
            .or_default()
            .insert(count, path)
            .is_some()
        {
            return Err("duplicate guard input".into());
        }
    }
    if groups.is_empty() {
        return Err("no fixed-work guard inputs".into());
    }
    let mut cases = Vec::new();
    for (fixture, runs) in groups {
        if runs.len() != 2 || !runs.contains_key(&0) {
            return Err(format!("{fixture} needs N=0 and one positive run count"));
        }
        for (runs, path) in runs {
            cases.push(Case {
                fixture: fixture.clone(),
                runs,
                sha256: hash(&path)?,
                path,
            });
        }
    }
    Ok(cases)
}
fn run(job: &Job, out: &Path, callgrind: bool) -> Result<Value, String> {
    let prefix = format!(
        "{}-{}-{}",
        job.binary_index, job.case.fixture, job.case.runs
    );
    let stdout_path = out.join(format!("{prefix}.stdout"));
    let stderr_path = out.join(format!("{prefix}.stderr"));
    let instruction_log = out.join(format!("{prefix}.callgrind.log"));
    let mut command = if callgrind {
        let mut command = Command::new("valgrind");
        let mut log_option = std::ffi::OsString::from("--log-file=");
        log_option.push(instruction_log.as_os_str());
        command
            .args(["--tool=callgrind", "--callgrind-out-file=/dev/null"])
            .arg(log_option)
            .arg(&job.binary);
        command
    } else {
        Command::new(&job.binary)
    };
    let timeout = if callgrind {
        CALLGRIND_TIMEOUT
    } else {
        CASE_TIMEOUT
    };
    let mut child = command
        .arg(&job.case.path)
        .stdin(Stdio::null())
        .stdout(fs::File::create(&stdout_path).map_err(|e| e.to_string())?)
        .stderr(fs::File::create(&stderr_path).map_err(|e| e.to_string())?)
        .spawn()
        .map_err(|e| e.to_string())?;
    let started = Instant::now();
    let (status, timed_out) = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break (status, false);
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            break (child.wait().map_err(|e| e.to_string())?, true);
        }
        thread::sleep(POLL_INTERVAL);
    };
    let stdout = fs::read(&stdout_path).map_err(|e| e.to_string())?;
    let stderr = fs::read(&stderr_path).map_err(|e| e.to_string())?;
    let marker = format!("done {} {}", job.case.fixture, job.case.runs);
    let completed = String::from_utf8_lossy(&stdout)
        .lines()
        .any(|line| line == marker);
    let instructions = if callgrind {
        let log = fs::read_to_string(&instruction_log).map_err(|e| e.to_string())?;
        let counts = log
            .lines()
            .filter(|line| line.contains("I   refs:"))
            .filter_map(|line| {
                line.split_whitespace()
                    .last()?
                    .replace(',', "")
                    .parse::<u64>()
                    .ok()
            })
            .collect::<Vec<_>>();
        if counts.len() == 1 {
            Some(counts[0])
        } else {
            None
        }
    } else {
        None
    };
    Ok(json!({
        "binary_index":job.binary_index,"fixture":job.case.fixture,"runs":job.case.runs,
        "input":job.case.path,"input_sha256":job.case.sha256,"exit_code":status.code(),
        "timed_out":timed_out,"completion_marker":completed,"stderr_empty":stderr.is_empty(),
        "valid":status.success() && !timed_out && completed && stderr.is_empty()
            && (!callgrind || instructions.is_some_and(|count| count > 0)),
        "instructions":instructions,"instruction_log":if callgrind {Some(&instruction_log)} else {None},
        "stdout":stdout_path,"stderr":stderr_path
    }))
}
fn execute() -> Result<bool, String> {
    let mut args: Vec<_> = env::args_os().skip(1).collect();
    let callgrind = args.first().is_some_and(|arg| arg == "--callgrind");
    if callgrind {
        args.remove(0);
    }
    if callgrind && args.len() != 4 {
        return Err("Callgrind mode requires exactly a baseline and candidate".into());
    }
    if args.len() < 3 {
        return Err(
            "usage: verify-fixed-work [--callgrind] INPUT_DIR FRESH_OUT_DIR BINARY...".into(),
        );
    }
    let inputs = fs::canonicalize(&args[0]).map_err(|e| e.to_string())?;
    let cases = inventory(&inputs)?;
    fs::create_dir(&args[1]).map_err(|e| format!("output must be a fresh directory: {e}"))?;
    let out = fs::canonicalize(&args[1]).map_err(|e| e.to_string())?;
    let mut binaries = Vec::new();
    let mut pending = VecDeque::new();
    for (index, path) in args[2..].iter().enumerate() {
        let path = fs::canonicalize(path).map_err(|e| e.to_string())?;
        binaries.push(json!({"path":path,"sha256":hash(&path)?}));
        for case in &cases {
            pending.push_back(Job {
                binary_index: index,
                binary: path.clone(),
                case: case.clone(),
            });
        }
    }
    let expected = pending.len();
    let pending = Mutex::new(pending);
    let rows = Mutex::new(Vec::new());
    thread::scope(|scope| {
        for _ in 0..WORKERS {
            scope.spawn(|| loop {
                let Some(job) = pending.lock().unwrap().pop_front() else { break; };
                let row = run(&job, &out, callgrind).unwrap_or_else(|error| json!({"binary_index":job.binary_index,"fixture":job.case.fixture,"runs":job.case.runs,"valid":false,"error":error}));
                rows.lock().unwrap().push(row);
            });
        }
    });
    let mut rows = rows.into_inner().unwrap();
    rows.sort_by_key(|row| {
        (
            row["binary_index"].as_u64(),
            row["fixture"].as_str().unwrap_or("").to_owned(),
            row["runs"].as_u64(),
        )
    });
    let mut identities_unchanged = true;
    for binary in &binaries {
        identities_unchanged &= hash(Path::new(binary["path"].as_str().unwrap()))?
            == binary["sha256"].as_str().unwrap();
    }
    for case in &cases {
        identities_unchanged &= hash(&case.path)? == case.sha256;
    }
    let valid = identities_unchanged
        && rows.len() == expected
        && rows.iter().all(|row| row["valid"] == true);
    let comparisons = if callgrind && valid {
        instruction_comparisons(&cases, &rows)?
    } else {
        Vec::new()
    };
    let instruction_limit_met = if callgrind && valid {
        Some(comparisons.iter().all(|row| row["within_limit"] == true))
    } else {
        None
    };
    let host = Command::new("uname")
        .arg("-a")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned());
    let verifier_path = env::current_exe().map_err(|e| e.to_string())?;
    let verifier_sha256 = hash(&verifier_path)?;
    let revision = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned());
    let report = json!({
        "kind":if callgrind {"validated paired Callgrind instruction counts, not throughput or RSS"} else {"fixed-work correctness replay, not timing or instruction measurement"},
        "verifier":{"path":verifier_path,"sha256":verifier_sha256,"repository_revision":revision},
        "identities_unchanged":identities_unchanged,"binaries":binaries,"cases_per_binary":cases.len(),
        "expected_cases":expected,"all_valid":valid,"results":rows,"host":host,
        "cpu_limits":fs::read_to_string("/sys/fs/cgroup/cpu.max").ok(),
        "instruction_limit_percent":if callgrind {Some(INSTRUCTION_LIMIT_PERCENT)} else {None},
        "instruction_limit_met":instruction_limit_met,"comparisons":comparisons
    });
    fs::write(
        out.join("report.json"),
        serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    println!(
        "fixed-work verification: valid={valid}, instruction_limit_met={instruction_limit_met:?}, cases={expected}, report={}",
        out.join("report.json").display()
    );
    Ok(valid)
}
fn instruction_comparisons(cases: &[Case], rows: &[Value]) -> Result<Vec<Value>, String> {
    let mut comparisons = Vec::new();
    for case in cases.iter().filter(|case| case.runs > 0) {
        let count = |side: u64, runs: usize| -> Result<u64, String> {
            rows.iter()
                .find(|row| {
                    row["binary_index"] == side
                        && row["fixture"] == case.fixture
                        && row["runs"] == runs
                })
                .and_then(|row| row["instructions"].as_u64())
                .ok_or_else(|| "missing paired instruction count".into())
        };
        let base_startup = count(0, 0)?;
        let candidate_startup = count(1, 0)?;
        let base = count(0, case.runs)?
            .checked_sub(base_startup)
            .filter(|n| *n > 0)
            .ok_or("invalid baseline startup subtraction")?;
        let candidate = count(1, case.runs)?
            .checked_sub(candidate_startup)
            .filter(|n| *n > 0)
            .ok_or("invalid candidate startup subtraction")?;
        let delta = (candidate as f64 / base as f64 - 1.0) * 100.0;
        comparisons.push(json!({"fixture":case.fixture,"runs":case.runs,"base_startup":base_startup,
            "candidate_startup":candidate_startup,"base_per_run":base as f64 / case.runs as f64,
            "candidate_per_run":candidate as f64 / case.runs as f64,"delta_percent":delta,"within_limit":delta <= INSTRUCTION_LIMIT_PERCENT}));
    }
    Ok(comparisons)
}

fn main() -> std::process::ExitCode {
    match execute() {
        Ok(true) => std::process::ExitCode::SUCCESS,
        Ok(false) => std::process::ExitCode::FAILURE,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::from(2)
        }
    }
}
