//! Canonical Wasm testsuite runner on the shared VM, with exact per-directive reports.

use std::{
    collections::HashSet,
    env, fs,
    path::PathBuf,
    process::{Command, ExitCode, Stdio},
    thread,
    time::{Duration, Instant},
};

use quench_wasm_test::TestSuite;
use serde_json::json;
use sha2::{Digest, Sha256};

fn main() -> ExitCode {
    match run() {
        Ok(passed) => {
            if passed {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(2)
        }
    }
}

const WORKER_POLL_INTERVAL: Duration = Duration::from_millis(10);

fn run() -> Result<bool, String> {
    if env::args_os().nth(1).is_some_and(|arg| arg == "--worker") {
        return run_worker();
    }
    let timeout_ms = env::var("WASM_FILE_TIMEOUT_MS")
        .map_err(|_| "WASM_FILE_TIMEOUT_MS must be a positive file deadline")?
        .parse::<u64>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or("WASM_FILE_TIMEOUT_MS must be a positive file deadline")?;
    let binary = env::current_exe().map_err(|error| error.to_string())?;
    let binary_sha256 = format!(
        "{:x}",
        Sha256::digest(fs::read(&binary).map_err(|error| error.to_string())?)
    );
    let mut args = env::args_os().skip(1);
    let mut report_path = PathBuf::from("target/iteration/wasm-shared-report.json");
    let mut paths = Vec::new();
    while let Some(arg) = args.next() {
        if arg == "--report" {
            report_path = args
                .next()
                .map(PathBuf::from)
                .ok_or("--report needs a path")?;
        } else {
            paths.push(PathBuf::from(arg));
        }
    }
    if paths.is_empty() {
        return Err("usage: run [--report FILE] WAST_FILE_OR_DIRECTORY ...".into());
    }
    let mut files = Vec::new();
    for path in paths {
        if path.is_dir() {
            for entry in walkdir::WalkDir::new(&path) {
                let entry = entry.map_err(|error| error.to_string())?;
                if entry.file_type().is_file()
                    && entry.path().extension().is_some_and(|e| e == "wast")
                {
                    files.push(entry.into_path());
                }
            }
        } else if path.is_file() && path.extension().is_some_and(|e| e == "wast") {
            files.push(path);
        } else {
            return Err(format!(
                "missing Wasm input or wrong extension: {}",
                path.display()
            ));
        }
    }
    files.sort();
    if files.is_empty() {
        return Err("no Wasm directives discovered: empty file scope".into());
    }
    let mut identities = HashSet::new();
    for file in &files {
        let canonical = file.canonicalize().map_err(|error| error.to_string())?;
        if !identities.insert(canonical) {
            return Err(format!("duplicate Wasm input: {}", file.display()));
        }
    }
    let mut inputs = Vec::new();
    let mut outcomes = Vec::new();
    let mut file_errors = Vec::new();
    for file in files {
        let bytes = fs::read(&file).map_err(|error| error.to_string())?;
        inputs.push(json!({"path": file, "sha256": format!("{:x}", Sha256::digest(&bytes))}));
        let worker_index = inputs.len() - 1;
        let worker_base =
            report_path.with_extension(format!("{}.{worker_index}.worker", std::process::id()));
        let worker_report = worker_base.with_extension("json");
        let stdout_path = worker_base.with_extension("stdout");
        let stderr_path = worker_base.with_extension("stderr");
        if let Some(parent) = worker_report.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let stdout = fs::File::create(&stdout_path).map_err(|error| error.to_string())?;
        let stderr = fs::File::create(&stderr_path).map_err(|error| error.to_string())?;
        let started = Instant::now();
        let mut child = Command::new(env::current_exe().map_err(|error| error.to_string())?)
            .arg("--worker")
            .arg(&file)
            .arg(&worker_report)
            .stdin(Stdio::null())
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr))
            .spawn()
            .map_err(|error| error.to_string())?;
        let mut timed_out = false;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => {}
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(error.to_string());
                }
            }
            if started.elapsed() >= Duration::from_millis(timeout_ms) {
                timed_out = true;
                let _ = child.kill();
                break child.wait().map_err(|error| error.to_string())?;
            }
            thread::sleep(WORKER_POLL_INTERVAL);
        };
        let raw_stdout = fs::read_to_string(&stdout_path).map_err(|error| error.to_string())?;
        let raw_stderr = fs::read_to_string(&stderr_path).map_err(|error| error.to_string())?;
        inputs[worker_index]["execution"] = json!({"status": status.code(), "status_text": status.to_string(), "timed_out": timed_out, "stdout": raw_stdout, "stderr": raw_stderr});
        if timed_out || !status.success() {
            file_errors.push(json!({"path": file, "error": if timed_out { "timed_out" } else { "worker failed" }, "status": status.code()}));
        } else {
            let worker: serde_json::Value = serde_json::from_slice(
                &fs::read(&worker_report).map_err(|error| error.to_string())?,
            )
            .map_err(|error| error.to_string())?;
            let results = worker
                .as_array()
                .ok_or("worker report is not an outcome array")?;
            if results.is_empty() {
                file_errors.push(json!({"path": file, "error": "empty directive discovery"}));
            }
            for result in results {
                if result["kind"] == "wast" {
                    file_errors.push(json!({"path": file, "error": result["got"]}));
                }
                outcomes.push(result.clone());
            }
        }
        for artifact in [&worker_report, &stdout_path, &stderr_path] {
            if artifact.exists() {
                fs::remove_file(artifact).map_err(|error| error.to_string())?;
            }
        }
    }
    let total = outcomes.len();
    let passed = outcomes.iter().filter(|r| r["outcome"] == "pass").count();
    let failed = total - passed;
    let final_binary_sha256 = format!(
        "{:x}",
        Sha256::digest(fs::read(&binary).map_err(|error| error.to_string())?)
    );
    if final_binary_sha256 != binary_sha256 {
        file_errors
            .push(json!({"path": binary, "error": "runner binary changed during execution"}));
    }
    let inventory_complete = file_errors.is_empty();
    let report = json!({
        "schema": 1, "engine": "quench", "suite": "wasm", "file_timeout_ms": timeout_ms, "total": total, "passed": passed, "failed": failed,
        "inventory_complete": inventory_complete, "file_errors": file_errors,
        "inputs": inputs, "outcomes": outcomes,
        "provenance": { "binary": binary, "binary_sha256": binary_sha256, "command": env::args_os().collect::<Vec<_>>() },
    });
    if let Some(parent) = report_path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let encoded = serde_json::to_vec_pretty(&report).map_err(|error| error.to_string())?;
    fs::write(&report_path, encoded).map_err(|error| error.to_string())?;
    println!(
        "shared Wasm: {total} total, {passed} passed, {failed} failed; report {}",
        report_path.display()
    );
    Ok(total != 0 && failed == 0 && inventory_complete)
}

fn run_worker() -> Result<bool, String> {
    let mut args = env::args_os().skip(2);
    let file = args
        .next()
        .map(PathBuf::from)
        .ok_or("worker needs an input path")?;
    let output = args
        .next()
        .map(PathBuf::from)
        .ok_or("worker needs an output path")?;
    if args.next().is_some() {
        return Err("unexpected worker arguments".into());
    }
    let worker_file = file.clone();
    let report = thread::Builder::new()
        .stack_size(quench_stack::WORKER_STACK_SIZE)
        .spawn(move || {
            let suite = TestSuite::new(quench_wasm_test::testsuite_root());
            suite.run_file(&worker_file)
        })
        .map_err(|error| format!("could not reserve Wasm worker stack: {error}"))?
        .join()
        .map_err(|_| "Wasm worker panicked".to_owned())?;
    let outcomes = report
        .results
        .into_iter()
        .enumerate()
        .map(|(index, result)| {
            json!({
                "path": format!("{}#directive={index}", file.display()), "file": file, "stage": "wasm", "directive_index": index, "line": result.line,
                "kind": result.kind, "outcome": if result.passed { "pass" } else { "failed" },
                "expected": result.expected, "got": result.got,
            })
        })
        .collect::<Vec<_>>();
    fs::write(
        output,
        serde_json::to_vec(&outcomes).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    // The supervisor owns the verdict; a completed script can contain failures.
    Ok(true)
}
