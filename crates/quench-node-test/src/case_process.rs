//! One deadline and result protocol for isolated Node fixture processes.
use std::{
    fs,
    path::Path,
    process::{Command, ExitCode, Stdio},
    time::Duration,
};
use wait_timeout::ChildExt;

use crate::NodeOutcome;

pub const DEFAULT_CASE_TIMEOUT_SECS: u64 = 30;
const WORKER_OPTION: &str = "--case-worker";

macro_rules! case_results {
    ($($variant:ident => $label:literal),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum RunResult { $($variant),+ }
        impl RunResult {
            const ALL: &'static [Self] = &[$(Self::$variant),+];
            pub const COUNT: usize = Self::ALL.len();
            pub fn label(self) -> &'static str {
                match self { $(Self::$variant => $label),+ }
            }
        }
    };
}
case_results! {
    Pass => "pass", Skip => "skip", Fail => "fail",
    Timeout => "timeout", Crash => "crash", Unclassified => "unclassified",
}

#[derive(serde::Serialize)]
pub struct CaseObservation {
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    pub timed_out: bool,
    pub worker: Option<NodeOutcome>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl CaseObservation {
    pub fn outcome(&self) -> RunResult {
        if self.timed_out {
            return RunResult::Timeout;
        }
        if self.exit_code.is_none() {
            return RunResult::Crash;
        }
        match (&self.worker, self.exit_code == Some(0)) {
            (Some(NodeOutcome::Pass), true) => RunResult::Pass,
            (Some(NodeOutcome::Skip { .. }), true) => RunResult::Skip,
            (Some(NodeOutcome::Fail { .. }), false) => RunResult::Fail,
            _ => RunResult::Unclassified,
        }
    }
}

/// Compiled worker entry for a runner binary with a statically selected engine.
pub fn worker_entry_with(
    arguments: &[String],
    run_file: fn(&Path) -> NodeOutcome,
) -> Option<ExitCode> {
    if arguments.first().map(String::as_str) != Some(WORKER_OPTION) {
        return None;
    }
    let [_, fixture, result] = arguments else {
        eprintln!("invalid Node case-worker invocation");
        return Some(ExitCode::from(2));
    };
    let outcome = run_file(Path::new(fixture));
    let code = match &outcome {
        NodeOutcome::Pass | NodeOutcome::Skip { .. } | NodeOutcome::GuestExit { .. } => {
            ExitCode::SUCCESS
        }
        NodeOutcome::Fail { .. } => ExitCode::from(1),
    };
    let write = serde_json::to_vec(&outcome)
        .map_err(|error| error.to_string())
        .and_then(|bytes| fs::write(result, bytes).map_err(|error| error.to_string()));
    Some(match write {
        Ok(()) => code,
        Err(error) => {
            eprintln!("write Node worker result: {error}");
            ExitCode::from(2)
        }
    })
}

/// Output is file-backed, so a noisy worker cannot block on a full pipe.
/// The worker result has its own channel; guest stdout never classifies a case.
pub fn observe_case(
    executable: &Path,
    fixture: &Path,
    timeout: Duration,
) -> Result<CaseObservation, String> {
    observe_case_with_environment(executable, fixture, timeout, &[], false)
}

/// Run an upstream `test/parallel` fixture with the same metadata boundary as
/// Node's Python harness: apply `// Env:` before the worker and prevent the
/// shared helper from parsing flags a second time.
pub fn observe_parallel_case(
    executable: &Path,
    fixture: &Path,
    timeout: Duration,
) -> Result<CaseObservation, String> {
    let source = fs::read_to_string(fixture)
        .map_err(|error| format!("read {}: {error}", fixture.display()))?;
    let metadata = crate::fixture_metadata::fixture_metadata(&source);
    observe_case_with_environment(executable, fixture, timeout, &metadata.env, true)
}

fn observe_case_with_environment(
    executable: &Path,
    fixture: &Path,
    timeout: Duration,
    env: &[(String, String)],
    skip_flag_check: bool,
) -> Result<CaseObservation, String> {
    if timeout.is_zero() || !fixture.is_file() {
        return Err("Node cases require an existing fixture and a positive deadline".into());
    }
    let directory = tempfile::tempdir().map_err(|error| error.to_string())?;
    let result = directory.path().join("result.json");
    let mut command = Command::new(executable);
    command
        .arg(WORKER_OPTION)
        .arg(fixture)
        .arg(&result)
        .env(quench_node::modules::process::CHILD_RUNNER_ENV, "1");
    command.envs(env.iter().cloned());
    if skip_flag_check {
        // The official Node test runner sets this after applying fixture Env.
        command.env("NODE_SKIP_FLAG_CHECK", "true");
    }
    observe_process_in(command, timeout, &directory, Some(&result))
}

/// Capture a raw local process using the same deadline and output path as workers.
pub fn observe_command(command: Command, timeout: Duration) -> Result<CaseObservation, String> {
    if timeout.is_zero() {
        return Err("Node cases require a positive deadline".into());
    }
    let directory = tempfile::tempdir().map_err(|error| error.to_string())?;
    observe_process_in(command, timeout, &directory, None)
}

fn observe_process_in(
    mut command: Command,
    timeout: Duration,
    directory: &tempfile::TempDir,
    result_path: Option<&Path>,
) -> Result<CaseObservation, String> {
    let stdout = directory.path().join("stdout");
    let stderr = directory.path().join("stderr");
    command
        .stdout(Stdio::from(
            fs::File::create(&stdout).map_err(|error| error.to_string())?,
        ))
        .stderr(Stdio::from(
            fs::File::create(&stderr).map_err(|error| error.to_string())?,
        ));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            command.pre_exec(|| {
                if libc::setpgid(0, 0) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("spawn observed process: {error}"))?;
    let waited = child.wait_timeout(timeout);
    let timed_out = matches!(waited, Ok(None));
    let status = match waited {
        Ok(Some(status)) => status,
        pending => {
            #[cfg(unix)]
            unsafe {
                libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
            }
            #[cfg(not(unix))]
            let _ = child.kill();
            let status = child.wait().map_err(|error| error.to_string())?;
            pending.map_err(|error| format!("wait for Node worker: {error}"))?;
            status
        }
    };
    #[cfg(unix)]
    let signal = {
        use std::os::unix::process::ExitStatusExt;
        status.signal()
    };
    #[cfg(not(unix))]
    let signal = None;
    Ok(CaseObservation {
        exit_code: status.code(),
        signal,
        timed_out,
        worker: result_path
            .and_then(|path| fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok()),
        stdout: fs::read(stdout).map_err(|error| format!("read worker stdout: {error}"))?,
        stderr: fs::read(stderr).map_err(|error| format!("read worker stderr: {error}"))?,
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn only_completed_consistent_worker_results_can_pass() {
        const OUTPUT_CONTROL_BYTES: usize = 131_072;
        const CONTROL_TIMEOUT: Duration = Duration::from_secs(5);
        const SHORT_DEADLINE: Duration = Duration::from_secs(1);
        let directory = tempfile::tempdir().unwrap();
        let fixture = directory.path().join("case.js");
        fs::write(&fixture, "inert input; not evaluated by control workers").unwrap();
        let executable = directory.path().join("worker");
        for (body, expected) in [
            (format!("head -c {OUTPUT_CONTROL_BYTES} /dev/zero\nhead -c {OUTPUT_CONTROL_BYTES} /dev/zero >&2\nprintf '%s' '{{\"outcome\":\"pass\"}}' >\"$3\""), RunResult::Pass),
            ("printf '__QUENCH_RESULT__ pass\\n'\nprintf '%s' '{\"outcome\":\"skip\",\"reason\":\"control skip\"}' >\"$3\"".into(), RunResult::Skip),
            ("printf '__QUENCH_RESULT__ pass\\n'\nprintf '%s' '{\"outcome\":\"fail\",\"reason\":\"control failure\"}' >\"$3\"\nexit 1".into(), RunResult::Fail),
            ("printf '__QUENCH_RESULT__ pass\\n'\nexit 0".into(), RunResult::Unclassified),
            ("printf '{' >\"$3\"\nexit 0".into(), RunResult::Unclassified),
            ("printf '%s' '{\"outcome\":\"pass\"}' >\"$3\"\nexit 1".into(), RunResult::Unclassified),
            ("kill -TERM $$".into(), RunResult::Crash),
        ] {
            fs::write(&executable, format!("#!/bin/sh\n{body}\n")).unwrap();
            fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
            let observation = observe_case(&executable, &fixture, CONTROL_TIMEOUT).unwrap();
            assert_eq!(observation.outcome(), expected);
            if expected == RunResult::Pass {
                assert_eq!(observation.stdout, vec![0; OUTPUT_CONTROL_BYTES]);
                assert_eq!(observation.stderr, vec![0; OUTPUT_CONTROL_BYTES]);
            }
        }
        // A result written before terminal exit cannot bypass the deadline.
        fs::write(
            &executable,
            "#!/bin/sh\nprintf '%s' '{\"outcome\":\"pass\"}' >\"$3\"\nsleep 5\n",
        )
        .unwrap();
        let observation = observe_case(&executable, &fixture, SHORT_DEADLINE).unwrap();
        assert_eq!(observation.outcome(), RunResult::Timeout);
        assert!(observation.worker.is_some());
        assert!(observe_case(&executable, &fixture, Duration::ZERO).is_err());
        assert!(observe_case(
            &executable,
            &directory.path().join("missing.js"),
            CONTROL_TIMEOUT
        )
        .is_err());
    }
}
