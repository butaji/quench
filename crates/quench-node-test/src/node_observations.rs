//! Matched raw process traces for the inventory's output-observation cases.

use crate::{
    case_process::{observe_case, observe_command, CaseObservation, DEFAULT_CASE_TIMEOUT_SECS},
    inventory::{NodeInventory, ObservationInput},
    fixture_metadata::fixture_flags,
    outcome::NodeOutcome,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    env,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

const TRACE_SCHEMA: u64 = 1;
const TRACE_PATH: &str = "tasks/evidence/task19-node-observation-traces.json";
const NODE_COMMONJS_ADAPTER: &str = r#"
const fs = require('node:fs');
const path = require('node:path');
const { Module } = require('node:module');
const filename = path.resolve(process.argv[1]);
const fixture = new Module(filename);
fixture.filename = filename;
fixture.paths = Module._nodeModulePaths(path.dirname(filename));
fixture._compile(fs.readFileSync(filename, 'utf8'), filename);
"#;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
struct ProcessRecord {
    exit_code: Option<i32>,
    signal: Option<i32>,
    timed_out: bool,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

impl From<&CaseObservation> for ProcessRecord {
    fn from(observation: &CaseObservation) -> Self {
        Self {
            exit_code: observation.exit_code,
            signal: observation.signal,
            timed_out: observation.timed_out,
            stdout: observation.stdout.clone(),
            stderr: observation.stderr.clone(),
        }
    }
}

impl ProcessRecord {
    fn completed_successfully(&self) -> bool {
        self.exit_code == Some(0) && self.signal.is_none() && !self.timed_out
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
struct ExecutableIdentity {
    path: PathBuf,
    sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
struct NodeIdentity {
    path: PathBuf,
    sha256: String,
    version: String,
    commonjs_adapter_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ObservationRecord {
    path: PathBuf,
    sha256: String,
    flags: Vec<String>,
    node: ProcessRecord,
    shared_vm: ProcessRecord,
    shared_worker: Option<NodeOutcome>,
}

impl ObservationRecord {
    fn matches(&self) -> bool {
        self.node.completed_successfully()
            && self.shared_vm.completed_successfully()
            && self.node == self.shared_vm
            && matches!(self.shared_worker.as_ref(), Some(NodeOutcome::Pass))
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ObservationEvidence {
    schema: u64,
    inventory_sha256: String,
    node: NodeIdentity,
    shared_runner: ExecutableIdentity,
    timeout_seconds: u64,
    records: Vec<ObservationRecord>,
}

/// Run only included observation fixtures and save their exact local-Node and shared-VM records.
pub(crate) fn trace_observations(
    inventory: &NodeInventory,
    repository: &Path,
) -> Result<bool, String> {
    let observations = inventory.included_observations();
    if observations.is_empty() {
        return Err("Node inventory has no included observation cases".into());
    }

    let node = node_identity()?;
    let shared_runner = executable_identity(
        &env::current_exe().map_err(|error| format!("resolve shared runner: {error}"))?,
    )?;
    let timeout = Duration::from_secs(DEFAULT_CASE_TIMEOUT_SECS);
    let records = observations
        .iter()
        .map(|input| capture_observation(input, repository, &node, &shared_runner, timeout))
        .collect::<Result<Vec<_>, _>>()?;
    let evidence = ObservationEvidence {
        schema: TRACE_SCHEMA,
        inventory_sha256: inventory.sha256().to_owned(),
        node,
        shared_runner,
        timeout_seconds: DEFAULT_CASE_TIMEOUT_SECS,
        records,
    };
    let path = repository.join(TRACE_PATH);
    fs::write(
        &path,
        serde_json::to_vec_pretty(&evidence).map_err(|error| error.to_string())?,
    )
    .map_err(|error| format!("write {}: {error}", path.display()))?;

    let matched = evidence
        .records
        .iter()
        .filter(|record| record.matches())
        .count();
    println!(
        "observation traces: {matched}/{} matched; evidence {}",
        evidence.records.len(),
        path.display()
    );
    Ok(matched == evidence.records.len())
}

fn capture_observation(
    input: &ObservationInput,
    repository: &Path,
    node: &NodeIdentity,
    shared_runner: &ExecutableIdentity,
    timeout: Duration,
) -> Result<ObservationRecord, String> {
    let fixture = repository.join(&input.path);
    let source = fs::read_to_string(&fixture)
        .map_err(|error| format!("read {}: {error}", fixture.display()))?;
    let flags = fixture_flags(&source);
    let mut node_command = Command::new(&node.path);
    node_command
        .args(&flags)
        .arg("--input-type=commonjs")
        .arg("--eval")
        .arg(NODE_COMMONJS_ADAPTER)
        .arg(&fixture)
        .env_remove("NODE_OPTIONS")
        .env_remove("NODE_PATH")
        .current_dir(repository);
    let node_observation = observe_command(node_command, timeout)
        .map_err(|error| format!("local Node {}: {error}", input.path.display()))?;
    let shared_observation = observe_case(&shared_runner.path, &fixture, timeout)
        .map_err(|error| format!("shared VM {}: {error}", input.path.display()))?;
    let record = ObservationRecord {
        path: input.path.clone(),
        sha256: input.sha256.clone(),
        flags,
        node: ProcessRecord::from(&node_observation),
        shared_vm: ProcessRecord::from(&shared_observation),
        shared_worker: shared_observation.worker.clone(),
    };
    println!(
        "{}  {} ({})",
        if record.matches() {
            "MATCH"
        } else {
            "MISMATCH"
        },
        input.path.display(),
        match &record.shared_worker {
            Some(NodeOutcome::Pass) => "shared worker: pass".to_owned(),
            Some(NodeOutcome::Fail { reason }) => format!("shared worker: fail: {reason}"),
            Some(NodeOutcome::Skip { reason }) => format!("shared worker: skip: {reason}"),
            Some(NodeOutcome::GuestExit { code }) => format!("shared worker: guest exit {code}"),
            None => "shared worker: no result".to_owned(),
        }
    );
    Ok(record)
}

pub(crate) fn validate_evidence(
    inventory: &NodeInventory,
    repository: &Path,
    selected: &[PathBuf],
) -> Result<(), String> {
    let expected = inventory.included_observations();
    if !expected
        .iter()
        .any(|input| selected.contains(&repository.join(&input.path)))
    {
        return Ok(());
    }

    let path = repository.join(TRACE_PATH);
    let bytes = fs::read(&path).map_err(|_| {
        format!(
            "included observation cases require matched traces in {}",
            path.display()
        )
    })?;
    let evidence: ObservationEvidence = serde_json::from_slice(&bytes)
        .map_err(|error| format!("parse observation evidence {}: {error}", path.display()))?;
    if evidence.schema != TRACE_SCHEMA {
        return Err("unsupported Node observation evidence schema".into());
    }
    if evidence.inventory_sha256 != inventory.sha256() {
        return Err("Node observation evidence is stale for the current inventory".into());
    }
    if evidence.timeout_seconds != DEFAULT_CASE_TIMEOUT_SECS
        || evidence.records.len() != expected.len()
    {
        return Err("Node observation evidence is incomplete or uses a different deadline".into());
    }

    let current_node = node_identity()?;
    if evidence.node != current_node {
        return Err("Node observation evidence uses a different local Node binary".into());
    }
    let current_runner = executable_identity(
        &env::current_exe().map_err(|error| format!("resolve shared runner: {error}"))?,
    )?;
    if evidence.shared_runner != current_runner {
        return Err("Node observation evidence uses a different shared runner binary".into());
    }

    for (record, input) in evidence.records.iter().zip(expected.iter()) {
        if record.path != input.path || record.sha256 != input.sha256 {
            return Err(format!(
                "Node observation evidence has a stale or reordered input at {}",
                input.path.display()
            ));
        }
        let fixture = repository.join(&input.path);
        let source = fs::read_to_string(&fixture)
            .map_err(|error| format!("read {}: {error}", fixture.display()))?;
        if record.flags != fixture_flags(&source) {
            return Err(format!(
                "Node observation evidence has stale flags for {}",
                input.path.display()
            ));
        }
        if !record.matches() {
            return Err(format!(
                "Node observation does not match local Node: {}",
                input.path.display()
            ));
        }
    }
    Ok(())
}

fn node_identity() -> Result<NodeIdentity, String> {
    let requested = env::var_os("NODE_BIN").unwrap_or_else(|| "node".into());
    let path = resolve_executable(&requested)?;
    let output = Command::new(&path)
        .arg("--version")
        .output()
        .map_err(|error| format!("run {} --version: {error}", path.display()))?;
    if !output.status.success() {
        return Err(format!("{} --version failed", path.display()));
    }
    let version = String::from_utf8(output.stdout)
        .map_err(|error| format!("decode {} --version: {error}", path.display()))?
        .trim()
        .to_owned();
    Ok(NodeIdentity {
        sha256: hash_file(&path)?,
        path,
        version,
        commonjs_adapter_sha256: format!("{:x}", Sha256::digest(NODE_COMMONJS_ADAPTER)),
    })
}

fn executable_identity(path: &Path) -> Result<ExecutableIdentity, String> {
    let path = path
        .canonicalize()
        .map_err(|error| format!("resolve executable {}: {error}", path.display()))?;
    Ok(ExecutableIdentity {
        sha256: hash_file(&path)?,
        path,
    })
}

fn resolve_executable(requested: &OsStr) -> Result<PathBuf, String> {
    let requested = PathBuf::from(requested);
    if requested.is_absolute() || requested.components().count() > 1 {
        return requested
            .canonicalize()
            .map_err(|error| format!("resolve Node binary {}: {error}", requested.display()));
    }
    let path = env::var_os("PATH").ok_or("PATH is unset while resolving Node binary")?;
    for directory in env::split_paths(&path) {
        let candidate = directory.join(&requested);
        if candidate.is_file() {
            return candidate
                .canonicalize()
                .map_err(|error| format!("resolve Node binary {}: {error}", candidate.display()));
        }
    }
    Err(format!("Node binary {:?} was not found in PATH", requested))
}

fn hash_file(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|error| format!("read {}: {error}", path.display()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}
