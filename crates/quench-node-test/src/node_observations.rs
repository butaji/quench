//! Live comparison of inventory observation cases with local Node.

use crate::{
    case_process::{
        CaseObservation, DEFAULT_CASE_TIMEOUT_SECS, observe_command, observe_inventory_case,
    },
    fixture_metadata::fixture_flags,
    outcome::NodeOutcome,
};
use std::{
    env,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

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

#[derive(Clone, Debug, PartialEq, Eq)]
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

struct ObservationRecord {
    path: PathBuf,
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

/// Run selected observation cases against local Node and the shared VM without
/// persisting process output.
pub(crate) fn validate_live(repository: &Path, observations: &[PathBuf]) -> Result<(), String> {
    if observations.is_empty() {
        return Ok(());
    }

    let node = node_executable()?;
    let shared_runner =
        env::current_exe().map_err(|error| format!("resolve shared runner: {error}"))?;
    let timeout = Duration::from_secs(DEFAULT_CASE_TIMEOUT_SECS);
    let mut mismatches = Vec::new();

    for fixture in observations {
        let record = capture_observation(fixture, repository, &node, &shared_runner, timeout)?;
        let matched = record.matches();
        println!(
            "{}  {} ({})",
            if matched { "MATCH" } else { "MISMATCH" },
            record.path.display(),
            shared_worker_description(record.shared_worker.as_ref())
        );
        if !matched {
            mismatches.push(record.path);
        }
    }

    if mismatches.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Node observation cases did not match local Node: {}",
            mismatches
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ))
    }
}

fn capture_observation(
    fixture: &Path,
    repository: &Path,
    node: &Path,
    shared_runner: &Path,
    timeout: Duration,
) -> Result<ObservationRecord, String> {
    let source = fs::read_to_string(fixture)
        .map_err(|error| format!("read {}: {error}", fixture.display()))?;
    let flags = fixture_flags(&source);
    let parallel_fixture = crate::case_process::is_node_parallel_fixture(repository, fixture);
    let mut node_command = Command::new(node);
    node_command
        .args(&flags)
        .arg("--input-type=commonjs")
        .arg("--eval")
        .arg(NODE_COMMONJS_ADAPTER)
        .arg(fixture)
        .env_remove("NODE_OPTIONS")
        .env_remove("NODE_PATH")
        .current_dir(repository);
    if parallel_fixture {
        if let Some(project) = crate::case_process::parallel_checkout_root(fixture) {
            node_command.current_dir(project);
        }
        node_command.envs(crate::fixture_metadata::fixture_metadata(&source).env);
        // The official Node test runner applies fixture flags before asking
        // common/index.js to inspect them, so the child must not re-exec.
        node_command.env("NODE_SKIP_FLAG_CHECK", "true");
    }
    let node_observation = observe_command(node_command, timeout)
        .map_err(|error| format!("local Node {}: {error}", fixture.display()))?;
    let shared_observation = observe_inventory_case(shared_runner, fixture, repository, timeout)
        .map_err(|error| format!("shared VM {}: {error}", fixture.display()))?;
    Ok(ObservationRecord {
        path: fixture.to_path_buf(),
        node: ProcessRecord::from(&node_observation),
        shared_vm: ProcessRecord::from(&shared_observation),
        shared_worker: shared_observation.worker,
    })
}

fn shared_worker_description(worker: Option<&NodeOutcome>) -> String {
    match worker {
        Some(NodeOutcome::Pass) => "shared worker: pass".to_owned(),
        Some(NodeOutcome::Fail { reason }) => format!("shared worker: fail: {reason}"),
        Some(NodeOutcome::Skip { reason }) => format!("shared worker: skip: {reason}"),
        Some(NodeOutcome::GuestExit { code }) => format!("shared worker: guest exit {code}"),
        None => "shared worker: no result".to_owned(),
    }
}

fn node_executable() -> Result<PathBuf, String> {
    let requested = env::var_os("NODE_BIN").unwrap_or_else(|| "node".into());
    resolve_executable(&requested)
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
