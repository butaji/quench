use serde_json::{Value, json};
use std::{fs::File, path::Path, process::{Command, Stdio}, time::{Duration, Instant}};
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use wait_timeout::ChildExt;

const BYTES_PER_KIBIBYTE: u64 = 1024;

/// Measure each command in a fresh process group, keeping compiler memory out
/// of runtime observations. OS time reports a process maximum, not a sampled
/// concurrent process-tree resident sum.
pub fn run(program: &Path, arguments: &[String], directory: &Path, label: &str, timeout: Duration, empty_environment: bool) -> Result<Value, String> {
    let stdout = directory.join(format!("{label}.stdout"));
    let stderr = directory.join(format!("{label}.stderr"));
    let resources = directory.join(format!("{label}.resources"));
    let mut command = Command::new("/usr/bin/time");
    if cfg!(target_os = "macos") { command.arg("-l"); }
    else if cfg!(target_os = "linux") { command.arg("-v"); }
    else { return Err("AOT stage measurements support macOS and Linux".into()); }
    command.arg("-o").arg(&resources).arg(program).args(arguments)
        .stdout(Stdio::from(File::create(&stdout).map_err(|e| e.to_string())?))
        .stderr(Stdio::from(File::create(&stderr).map_err(|e| e.to_string())?));
    #[cfg(unix)]
    command.process_group(0);
    if empty_environment { command.env_clear().current_dir("/"); }
    let started = Instant::now();
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let status = child.wait_timeout(timeout).map_err(|e| e.to_string())?;
    let timed_out = status.is_none();
    let status = if let Some(status) = status { status } else {
        // SAFETY: the child created its own process group with this PID. The
        // unreaped child still owns that identifier; no unrelated group is hit.
        #[cfg(unix)]
        unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL); }
        #[cfg(not(unix))]
        child.kill().map_err(|e| e.to_string())?;
        child.wait().map_err(|e| e.to_string())?
    };
    let elapsed = started.elapsed();
    let text = std::fs::read_to_string(&resources).unwrap_or_default();
    let maximum_resident_bytes = if cfg!(target_os = "macos") {
        text.lines().find(|line| line.contains("maximum resident set size"))
            .and_then(|line| line.split_whitespace().next()?.parse::<u64>().ok())
    } else {
        text.lines().find(|line| line.contains("Maximum resident set size (kbytes):"))
            .and_then(|line| line.split_once(':')?.1.trim().parse::<u64>().ok())
            .and_then(|value| value.checked_mul(BYTES_PER_KIBIBYTE))
    };
    Ok(json!({"program":program,"arguments":arguments,"exit":status.code(),"success":status.success() && !timed_out,
        "timed_out":timed_out,"wall_nanoseconds":elapsed.as_nanos(),"maximum_resident_bytes":maximum_resident_bytes,
        "memory_scope":"OS time per-command maximum RSS; concurrent process-tree peak not measured",
        "stdout":stdout,"stderr":stderr,"resources":resources,"empty_environment":empty_environment}))
}

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
mod tests {
    use super::*;
    #[test]
    fn command_measurements_preserve_failures_and_report_resident_memory() {
        let root = tempfile::tempdir().unwrap();
        for (program, success) in [("/usr/bin/true",true),("/usr/bin/false",false)] {
            let record = run(Path::new(program),&[],root.path(),if success {"pass"} else {"fail"},Duration::from_secs(5),true).unwrap();
            assert_eq!(record["success"],success);
            assert_eq!(record["timed_out"],false);
            assert!(record["maximum_resident_bytes"].as_u64().unwrap() > 0);
        }
    }
    #[test]
    fn command_timeout_terminates_the_owned_process_group() {
        let root = tempfile::tempdir().unwrap();
        let record = run(Path::new("/bin/sleep"),&["5".into()],root.path(),"timeout",Duration::from_millis(20),true).unwrap();
        assert_eq!(record["timed_out"],true);
        assert_eq!(record["success"],false);
        assert!(record["wall_nanoseconds"].as_u64().unwrap() < Duration::from_secs(5).as_nanos() as u64);
    }
}
