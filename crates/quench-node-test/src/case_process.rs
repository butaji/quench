//! One deadline and result protocol for isolated Node fixture processes.
use std::{
    collections::hash_map::DefaultHasher,
    fs,
    hash::{Hash, Hasher},
    path::Path,
    process::{Command, ExitCode, Stdio},
    time::{Duration, Instant},
};

use crate::NodeOutcome;

pub const DEFAULT_CASE_TIMEOUT_SECS: u64 = 30;
/// How often a running case is checked for exit; it bounds the error of `CaseObservation::wall`.
const POLL_INTERVAL: Duration = Duration::from_millis(1);
pub const NODE_PARALLEL_DIR: &str = "tests/node/test/parallel";
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
    /// The observed process's resource usage (`wait4`), where the platform reports it.
    pub usage: Option<ResourceUsage>,
    /// From spawn to reaping, within `POLL_INTERVAL`.
    pub wall: Duration,
}

/// What `wait4` reports for a reaped process.
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct ResourceUsage {
    /// The process's own peak resident set size (`ru_maxrss`).
    pub peak_rss_bytes: u64,
    /// User plus system CPU time (`ru_utime + ru_stime`).
    pub cpu: Duration,
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
            (Some(NodeOutcome::GuestExit { code: 0 }), true)
                if self.stdout.starts_with(b"1..0 # Skipped:") =>
            {
                RunResult::Skip
            }
            (Some(NodeOutcome::GuestExit { code: 0 }), true) => RunResult::Pass,
            (Some(NodeOutcome::GuestExit { code }), true) if *code != 0 => RunResult::Fail,
            (Some(NodeOutcome::Skip { .. }), true) => RunResult::Skip,
            (Some(NodeOutcome::Fail { .. }), false) => RunResult::Fail,
            (Some(NodeOutcome::GuestExit { .. }), false) => RunResult::Fail,
            _ => RunResult::Unclassified,
        }
    }
}

/// Select a frozen production CLI for raw child/API differential fixtures.
/// Unix exec preserves the process ID, groups, descriptors and signal boundary.
pub const NODE_TEST_EXECUTABLE_ENV: &str = "QUENCH_NODE_TEST_EXECUTABLE";

/// Both runner binaries consume this private mode before parsing public options.
pub fn worker_entry(arguments: &[String]) -> Option<ExitCode> {
    if arguments.first().map(String::as_str) != Some(WORKER_OPTION) {
        // Fixtures can launch process.argv[0], which is this embedding
        // executable. Its children must execute the requested Node entry,
        // rather than interpreting those arguments as a suite invocation.
        if std::env::var_os(quench_node::shared_run::CHILD_RUNNER_ENV).is_some() {
            if let Some(executable) = std::env::var_os(NODE_TEST_EXECUTABLE_ENV) {
                #[cfg(unix)]
                {
                    use std::os::unix::process::CommandExt;
                    let current = std::env::current_exe().and_then(std::fs::canonicalize);
                    let selected = std::fs::canonicalize(&executable);
                    if matches!((&current, &selected), (Ok(current), Ok(selected)) if current == selected)
                    {
                        eprintln!("{NODE_TEST_EXECUTABLE_ENV} must select a distinct CLI");
                        return Some(ExitCode::from(2));
                    }
                    let error = Command::new(executable).args(arguments).exec();
                    eprintln!("external Node-compatible CLI: {error}");
                    return Some(ExitCode::from(1));
                }
                #[cfg(not(unix))]
                {
                    let _ = executable;
                    eprintln!("{NODE_TEST_EXECUTABLE_ENV} requires Unix exec for process parity");
                    return Some(ExitCode::from(2));
                }
            }
            return Some(
                match quench_node::shared_run::run_shared_cli(arguments.iter().cloned()) {
                    Ok(code) => code,
                    Err(error) => {
                        eprintln!("{error}");
                        ExitCode::from(1)
                    }
                },
            );
        }
        return None;
    }
    let [_, fixture, result] = arguments else {
        eprintln!("invalid Node case-worker invocation");
        return Some(ExitCode::from(2));
    };
    let outcome = crate::shared_runner::run_file(Path::new(fixture));
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

/// Apply the harness boundary selected by the inventory's fixture source.
pub fn observe_inventory_case(
    executable: &Path,
    fixture: &Path,
    repository: &Path,
    timeout: Duration,
) -> Result<CaseObservation, String> {
    if is_node_parallel_fixture(repository, fixture) {
        observe_parallel_case(executable, fixture, timeout)
    } else {
        observe_case(executable, fixture, timeout)
    }
}

pub fn is_node_parallel_fixture(repository: &Path, fixture: &Path) -> bool {
    fixture.starts_with(repository.join(NODE_PARALLEL_DIR))
}

/// The upstream harness launches parallel fixtures from their Node checkout.
pub(crate) fn parallel_checkout_root(fixture: &Path) -> Option<&Path> {
    fixture
        .ancestors()
        .find(|directory| directory.ends_with("test/parallel"))
        .and_then(Path::parent)
        .and_then(Path::parent)
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
    let fixture = if skip_flag_check {
        fs::canonicalize(fixture).map_err(|error| format!("resolve fixture: {error}"))?
    } else {
        fixture.to_path_buf()
    };
    let directory = tempfile::tempdir().map_err(|error| error.to_string())?;
    let result = directory.path().join("result.json");
    let mut command = Command::new(executable);
    command
        .arg(WORKER_OPTION)
        .arg(&fixture)
        .arg(&result)
        .env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
    command.envs(env.iter().cloned());
    if skip_flag_check {
        if let Some(project) = parallel_checkout_root(&fixture) {
            command.current_dir(project);
            command.env(quench_node::shared_run::NODE_LIBRARY_ROOT_ENV, project);
        }
        // The official Node test runner sets this after applying fixture Env.
        command.env("NODE_SKIP_FLAG_CHECK", "true");
        // Node's common/tmpdir.js otherwise defaults every isolated fixture to
        // the same `.tmp.0` path. Parallel workers then delete and recreate one
        // another's files, making the recursive inventory nondeterministic.
        let mut hasher = DefaultHasher::new();
        fixture.hash(&mut hasher);
        command.env(
            "TEST_SERIAL_ID",
            format!("{}-{:x}", std::process::id(), hasher.finish()),
        );
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
    #[cfg(target_os = "linux")]
    enable_child_subreaper()?;
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
    let started = Instant::now();
    let deadline = started + timeout;
    let mut usage = None;
    let waited = loop {
        match try_wait(&mut child, &mut usage) {
            Ok(Some(status)) => break Ok(Some(status)),
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(POLL_INTERVAL);
            }
            Ok(None) => break Ok(None),
            Err(error) => break Err(error),
        }
    };
    let wall = started.elapsed();
    let timed_out = matches!(waited, Ok(None));
    // A fixture may leave child processes running even when its worker exits
    // normally or fails early. The case owns the entire process group, so
    // close that boundary on every terminal outcome, before reading output.
    #[cfg(unix)]
    unsafe {
        libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
    }
    let status = match waited {
        Ok(Some(status)) => status,
        pending => {
            #[cfg(not(unix))]
            let _ = child.kill();
            let status = wait(&mut child, &mut usage).map_err(|error| error.to_string())?;
            pending.map_err(|error| format!("wait for Node worker: {error}"))?;
            status
        }
    };
    #[cfg(target_os = "linux")]
    reap_process_group(child.id() as libc::pid_t)?;
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
        usage,
        wall,
    })
}

/// `wait4` reports the reaped child's resource usage; Linux counts `ru_maxrss` in KiB.
#[cfg(target_os = "linux")]
const MAXRSS_UNIT_BYTES: u64 = 1024;
/// `timeval` counts microseconds.
#[cfg(target_os = "linux")]
const NANOS_PER_MICRO: u32 = 1_000;

/// Reap `child` if it has exited, recording its peak RSS.
#[cfg(target_os = "linux")]
fn try_wait(
    child: &mut std::process::Child,
    usage: &mut Option<ResourceUsage>,
) -> std::io::Result<Option<std::process::ExitStatus>> {
    reap(child, libc::WNOHANG, usage)
}

/// Reap `child`, blocking until it exits, recording its peak RSS.
#[cfg(target_os = "linux")]
fn wait(
    child: &mut std::process::Child,
    usage: &mut Option<ResourceUsage>,
) -> std::io::Result<std::process::ExitStatus> {
    loop {
        if let Some(status) = reap(child, 0, usage)? {
            return Ok(status);
        }
    }
}

#[cfg(target_os = "linux")]
fn reap(
    child: &mut std::process::Child,
    options: libc::c_int,
    observed: &mut Option<ResourceUsage>,
) -> std::io::Result<Option<std::process::ExitStatus>> {
    use std::os::unix::process::ExitStatusExt;
    let mut status = 0;
    // SAFETY: zeroed rusage is a valid output buffer for wait4.
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    // SAFETY: `child` is this process's unreaped child.
    let pid = unsafe { libc::wait4(child.id() as libc::pid_t, &mut status, options, &mut usage) };
    match pid {
        0 => Ok(None),
        pid if pid < 0 => Err(std::io::Error::last_os_error()),
        _ => {
            let time = |time: libc::timeval| {
                Duration::new(time.tv_sec as u64, (time.tv_usec as u32) * NANOS_PER_MICRO)
            };
            *observed = u64::try_from(usage.ru_maxrss)
                .ok()
                .map(|kib| ResourceUsage {
                    peak_rss_bytes: kib * MAXRSS_UNIT_BYTES,
                    cpu: time(usage.ru_utime) + time(usage.ru_stime),
                });
            Ok(Some(std::process::ExitStatus::from_raw(status)))
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn try_wait(
    child: &mut std::process::Child,
    _: &mut Option<ResourceUsage>,
) -> std::io::Result<Option<std::process::ExitStatus>> {
    child.try_wait()
}

#[cfg(not(target_os = "linux"))]
fn wait(
    child: &mut std::process::Child,
    _: &mut Option<ResourceUsage>,
) -> std::io::Result<std::process::ExitStatus> {
    child.wait()
}

// Containers may have a PID 1 that does not reap orphaned fixture children.
// Adopt those children here so each case can reap its own process group.
#[cfg(target_os = "linux")]
fn enable_child_subreaper() -> Result<(), String> {
    static SETUP: std::sync::OnceLock<Result<(), i32>> = std::sync::OnceLock::new();
    SETUP
        .get_or_init(|| {
            if unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) } == 0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error()
                    .raw_os_error()
                    .unwrap_or(libc::EINVAL))
            }
        })
        .map_err(|code| {
            format!(
                "enable fixture child reaping: {}",
                std::io::Error::from_raw_os_error(code)
            )
        })
}

#[cfg(target_os = "linux")]
fn reap_process_group(group: libc::pid_t) -> Result<(), String> {
    loop {
        if unsafe { libc::waitpid(-group, std::ptr::null_mut(), 0) } >= 0 {
            continue;
        }
        let error = std::io::Error::last_os_error();
        match error.raw_os_error() {
            Some(libc::ECHILD) => return Ok(()),
            Some(libc::EINTR) => continue,
            _ => return Err(format!("reap fixture process group: {error}")),
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn parallel_workers_use_checkout_working_directory() {
        let directory = tempfile::tempdir().unwrap();
        let parallel = directory.path().join("test/parallel");
        fs::create_dir_all(&parallel).unwrap();
        let fixture = parallel.join("test-cwd.js");
        fs::write(&fixture, "inert input").unwrap();
        let executable = directory.path().join("worker");
        fs::write(
            &executable,
            "#!/bin/sh\npwd\nprintf '%s' '{\"outcome\":\"pass\"}' >\"$3\"\n",
        )
        .unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
        let observation =
            observe_parallel_case(&executable, &fixture, Duration::from_secs(5)).unwrap();
        assert_eq!(observation.outcome(), RunResult::Pass);
        assert_eq!(
            String::from_utf8(observation.stdout).unwrap().trim(),
            directory.path().to_string_lossy()
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn completed_workers_cannot_leave_live_children() {
        const CONTROL_TIMEOUT: Duration = Duration::from_secs(5);
        const CLEANUP_TIMEOUT: Duration = Duration::from_secs(1);
        let directory = tempfile::tempdir().unwrap();
        let fixture = directory.path().join("case.js");
        let executable = directory.path().join("worker");
        let child_pid = directory.path().join("child.pid");
        fs::write(&fixture, "inert input").unwrap();
        for (completion, expected) in [
            (
                "printf '%s' '{\"outcome\":\"pass\"}' >\"$3\"",
                RunResult::Pass,
            ),
            (
                "printf '%s' '{\"outcome\":\"fail\",\"reason\":\"control\"}' >\"$3\"\nexit 1",
                RunResult::Fail,
            ),
            ("kill -TERM $$", RunResult::Crash),
        ] {
            fs::write(
                &executable,
                format!(
                    "#!/bin/sh\nsleep 30 &\necho $! > '{}'\n{completion}\n",
                    child_pid.display(),
                ),
            )
            .unwrap();
            fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
            assert_eq!(
                observe_case(&executable, &fixture, CONTROL_TIMEOUT)
                    .unwrap()
                    .outcome(),
                expected
            );
            let pid = fs::read_to_string(&child_pid).unwrap();
            let stat = format!("/proc/{}/stat", pid.trim());
            let deadline = Instant::now() + CLEANUP_TIMEOUT;
            loop {
                let state = fs::read_to_string(&stat).ok().and_then(|stat| {
                    stat.rsplit_once(") ")
                        .and_then(|(_, fields)| fields.chars().next())
                });
                if state.is_none() {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "worker left live child {}",
                    pid.trim()
                );
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }

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
