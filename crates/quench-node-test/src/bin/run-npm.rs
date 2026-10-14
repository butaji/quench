//! Differential observations of pinned, published npm packages.
use quench_node_test::case_process::{observe_command, CaseObservation};
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf, process::Command, time::Duration};

#[derive(Deserialize)]
struct Inventory {
    packages: Vec<Package>,
}

#[derive(Deserialize, Serialize)]
struct Package {
    name: String,
    version: String,
    types_only: bool,
}

#[derive(Serialize)]
struct Record {
    package: Package,
    verdict: &'static str,
    node: CaseObservation,
    quench: CaseObservation,
}

#[derive(Serialize)]
struct Report {
    source_commit: String,
    source_dirty: bool,
    host: String,
    node_executable: PathBuf,
    node_version: String,
    quench_executable: PathBuf,
    quench_sha256: String,
    node_sha256: String,
    timeout_seconds: u64,
    inventory_sha256: String,
    lock_sha256: String,
    scenario_sha256: String,
    records: Vec<Record>,
}

fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

fn complete_success(observation: &CaseObservation, marker: &[u8]) -> bool {
    observation.exit_code == Some(0)
        && observation.signal.is_none()
        && !observation.timed_out
        && observation.stdout == marker
        && observation.stderr.is_empty()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let [node, quench, output, timeout, rest @ ..] = args.as_slice() else {
        return Err("usage: run-npm NODE QUENCH REPORT TIMEOUT_SECONDS [PACKAGE]".into());
    };
    if rest.len() > 1 {
        return Err("at most one package filter is allowed".into());
    }
    let timeout_seconds: u64 = timeout.parse()?;
    if timeout_seconds == 0 {
        return Err("a positive per-package deadline is required".into());
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let suite = fs::canonicalize(root.join("tests/npm-top100"))?;
    let inventory_bytes = fs::read(suite.join("inventory.json"))?;
    let inventory: Inventory = serde_json::from_slice(&inventory_bytes)?;
    if inventory.packages.len() != 100 {
        return Err("qualification inventory must contain exactly 100 packages".into());
    }
    let mut unique = std::collections::HashSet::new();
    if inventory.packages.iter().any(|p| !unique.insert(&p.name)) {
        return Err("duplicate inventory package".into());
    }
    let node_executable = fs::canonicalize(node)?;
    let quench_executable = fs::canonicalize(quench)?;
    let node_output = Command::new(&node_executable).arg("--version").output()?;
    if !node_output.status.success() {
        return Err("Node oracle version query failed".into());
    }
    let mut report = Report {
        source_commit: String::from_utf8(
            Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(&root)
                .output()?
                .stdout,
        )?
        .trim()
        .into(),
        source_dirty: !Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(&root)
            .output()?
            .stdout
            .is_empty(),
        host: format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS),
        quench_sha256: digest(&fs::read(&quench_executable)?),
        node_sha256: digest(&fs::read(&node_executable)?),
        node_executable,
        node_version: String::from_utf8(node_output.stdout)?.trim().into(),
        quench_executable,
        timeout_seconds,
        inventory_sha256: digest(&inventory_bytes),
        lock_sha256: digest(&fs::read(suite.join("package-lock.json"))?),
        scenario_sha256: digest(&fs::read(suite.join("scenario.mjs"))?),
        records: Vec::new(),
    };
    for package in inventory.packages {
        if rest.first().is_some_and(|filter| filter != &package.name) {
            continue;
        }
        let installed: serde_json::Value = serde_json::from_slice(&fs::read(
            suite
                .join("node_modules")
                .join(&package.name)
                .join("package.json"),
        )?)?;
        if installed["version"].as_str() != Some(&package.version) {
            return Err(format!("installed version mismatch: {}", package.name).into());
        }
        let record = {
            let observe = |executable: &PathBuf| -> Result<CaseObservation, String> {
                let working = tempfile::tempdir().map_err(|e| e.to_string())?;
                let mut command = Command::new(executable);
                command
                    .arg(suite.join("scenario.mjs"))
                    .arg(&package.name)
                    .current_dir(working.path())
                    .env("NO_COLOR", "1")
                    .env_remove("FORCE_COLOR")
                    .env_remove("DEBUG");
                observe_command(command, Duration::from_secs(timeout_seconds))
            };
            let node = observe(&report.node_executable)?;
            let quench = observe(&report.quench_executable)?;
            let marker = format!("npm-behavior-ok:{}\n", package.name);
            let verdict = if !complete_success(&node, marker.as_bytes()) {
                "oracle-failed"
            } else if !complete_success(&quench, marker.as_bytes()) {
                "quench-failed"
            } else if package.types_only {
                "declarations-pass"
            } else {
                "behavior-pass"
            };
            Record {
                package,
                verdict,
                node,
                quench,
            }
        };
        println!(
            "{}@{}: {}",
            record.package.name, record.package.version, record.verdict
        );
        report.records.push(record);
        fs::write(output, serde_json::to_vec_pretty(&report)?)?;
    }
    if report.records.is_empty() {
        return Err("filter selected no packages".into());
    }
    if report
        .records
        .iter()
        .any(|r| !matches!(r.verdict, "behavior-pass" | "declarations-pass"))
    {
        return Err("npm qualification incomplete; see per-package report".into());
    }
    Ok(())
}
