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
    #[serde(default)]
    installation: Option<String>,
}

const PACKAGE_COUNT: usize = 300;

#[derive(Serialize)]
struct Record {
    package: Package,
    verdict: &'static str,
    #[serde(serialize_with = "serialize_observation")]
    node: CaseObservation,
    #[serde(serialize_with = "serialize_observation")]
    quench: CaseObservation,
    /// Bun's run of the same scenario, observed for resident memory (`--rss`).
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_optional_observation"
    )]
    bun: Option<CaseObservation>,
    node_function_checks: Option<serde_json::Value>,
    quench_function_checks: Option<serde_json::Value>,
}

fn serialize_observation<S: serde::Serializer>(
    observation: &CaseObservation,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    #[derive(Serialize)]
    #[serde(untagged)]
    enum Output<'a> {
        Text(&'a str),
        Bytes(&'a [u8]),
    }
    let output = |bytes| match std::str::from_utf8(bytes) {
        Ok(text) => Output::Text(text),
        Err(_) => Output::Bytes(bytes),
    };
    use serde::ser::SerializeStruct;
    let mut record = serializer.serialize_struct("Observation", 9)?;
    record.serialize_field("exit_code", &observation.exit_code)?;
    record.serialize_field("signal", &observation.signal)?;
    record.serialize_field("timed_out", &observation.timed_out)?;
    record.serialize_field("worker", &observation.worker)?;
    record.serialize_field("stdout", &output(&observation.stdout))?;
    record.serialize_field("stderr", &output(&observation.stderr))?;
    let usage = observation.usage;
    record.serialize_field("peak_rss_bytes", &usage.map(|usage| usage.peak_rss_bytes))?;
    record.serialize_field("cpu_nanos", &usage.map(|usage| usage.cpu.as_nanos()))?;
    record.serialize_field("wall_nanos", &observation.wall.as_nanos())?;
    record.end()
}

fn serialize_optional_observation<S: serde::Serializer>(
    observation: &Option<CaseObservation>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match observation {
        Some(observation) => serialize_observation(observation, serializer),
        None => serializer.serialize_none(),
    }
}

#[derive(Serialize)]
struct Report {
    mode: String,
    source_commit: String,
    source_dirty: bool,
    host: String,
    node_executable: PathBuf,
    node_version: String,
    quench_executable: PathBuf,
    quench_sha256: String,
    node_sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    bun_executable: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bun_version: Option<String>,
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
        return Err(
            "usage: run-npm NODE QUENCH REPORT TIMEOUT_SECONDS [--surface | --rss BUN] [PACKAGE]"
                .into(),
        );
    };
    let surface = rest.first().is_some_and(|arg| arg == "--surface");
    let bun = match rest {
        [flag, bun, ..] if flag == "--rss" => Some(fs::canonicalize(bun)?),
        _ => None,
    };
    let filters = match (surface, &bun) {
        (true, _) => &rest[1..],
        (false, Some(_)) => &rest[2..],
        (false, None) => rest,
    };
    if filters.len() > 1 {
        return Err("at most one package filter is allowed".into());
    }
    let timeout_seconds: u64 = timeout.parse()?;
    if timeout_seconds == 0 {
        return Err("a positive per-package deadline is required".into());
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let suite = fs::canonicalize(root.join("tests/npm-top300"))?;
    let inventory_bytes = fs::read(suite.join("inventory.json"))?;
    let inventory: Inventory = serde_json::from_slice(&inventory_bytes)?;
    if inventory.packages.len() != PACKAGE_COUNT {
        return Err(format!(
            "qualification inventory must contain exactly {PACKAGE_COUNT} packages"
        )
        .into());
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
        mode: if surface { "surface" } else { "scenario" }.into(),
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
        bun_version: match &bun {
            Some(bun) => Some(
                String::from_utf8(Command::new(bun).arg("--version").output()?.stdout)?
                    .trim()
                    .into(),
            ),
            None => None,
        },
        bun_executable: bun.clone(),
        node_executable,
        node_version: String::from_utf8(node_output.stdout)?.trim().into(),
        quench_executable,
        timeout_seconds,
        inventory_sha256: digest(&inventory_bytes),
        lock_sha256: digest(&fs::read(suite.join("package-lock.json"))?),
        scenario_sha256: digest(&fs::read(suite.join(if surface {
            "surface.mjs"
        } else {
            "scenario.mjs"
        }))?),
        records: Vec::new(),
    };
    for package in inventory.packages {
        if filters
            .first()
            .is_some_and(|filter| filter != &package.name)
        {
            continue;
        }
        let installed: serde_json::Value = serde_json::from_slice(&fs::read(
            suite
                .join("node_modules")
                .join(package.installation.as_ref().unwrap_or(&package.name))
                .join("package.json"),
        )?)?;
        if installed["version"].as_str() != Some(&package.version) {
            return Err(format!("installed version mismatch: {}", package.name).into());
        }
        let record = {
            let observe = |executable: &PathBuf| -> Result<(CaseObservation, Option<serde_json::Value>), String> {
                let working = tempfile::tempdir().map_err(|e| e.to_string())?;
                let mut command = Command::new(executable);
                command
                    .arg(suite.join(if surface { "surface.mjs" } else { "scenario.mjs" }))
                    .arg(&package.name)
                    .current_dir(working.path())
                    .env("NO_COLOR", "1")
                    .env_remove("FORCE_COLOR")
                    .env_remove("DEBUG");
                let observation = observe_command(command, Duration::from_secs(timeout_seconds))?;
                let checks_path = working.path().join("function-checks.json");
                let checks = if checks_path.exists() {
                    Some(serde_json::from_slice(&fs::read(checks_path).map_err(|e| e.to_string())?)
                        .map_err(|e| e.to_string())?)
                } else { None };
                Ok((observation, checks))
            };
            let (node, node_function_checks) = observe(&report.node_executable)?;
            let (quench, quench_function_checks) = observe(&report.quench_executable)?;
            let bun = match &report.bun_executable {
                Some(bun) => Some(observe(bun)?.0),
                None => None,
            };
            let marker = format!("npm-behavior-ok:{}\n", package.name);
            let verdict = if surface {
                let valid = |o: &CaseObservation| {
                    o.exit_code == Some(0)
                        && o.signal.is_none()
                        && !o.timed_out
                        && o.stderr.is_empty()
                        && serde_json::from_slice::<serde_json::Value>(&o.stdout)
                            .is_ok_and(|v| v["package"].as_str() == Some(package.name.as_str()))
                };
                if !valid(&node) {
                    "oracle-failed"
                } else if !valid(&quench) || node.stdout != quench.stdout {
                    "surface-diff"
                } else {
                    "surface-match-unqualified"
                }
            } else if String::from_utf8_lossy(&node.stderr).contains("No behavioral scenario for ")
            {
                "uncovered-scenario"
            } else if !complete_success(&node, marker.as_bytes()) {
                "oracle-failed"
            } else if !complete_success(&quench, marker.as_bytes()) {
                "quench-failed"
            } else if node_function_checks != quench_function_checks {
                "function-check-diff"
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
                bun,
                node_function_checks,
                quench_function_checks,
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
