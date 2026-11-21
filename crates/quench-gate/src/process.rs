//! Running a child process to completion and reporting what it said.

use std::process::{Command, Output};

/// The lines of a failing command's output a report keeps: the end, where the cause is.
const EXCERPT_LINES: usize = 40;

/// What a finished command wrote, and whether it succeeded.
pub struct Finished {
    pub success: bool,
    /// The exit code, absent when a signal ended the process.
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl From<Output> for Finished {
    fn from(output: Output) -> Self {
        Self {
            success: output.status.success(),
            code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }
}

impl Finished {
    /// The end of both streams, for a failure report.
    pub fn excerpt(&self) -> String {
        let text = format!("{}{}", self.stdout, self.stderr);
        let lines: Vec<&str> = text.lines().collect();
        lines[lines.len().saturating_sub(EXCERPT_LINES)..].join("\n")
    }
}

/// Runs `command` to completion.
pub fn run(command: &mut Command) -> Result<Finished, String> {
    command.output().map(Finished::from).map_err(|error| format!("cannot run {:?}: {error}", command.get_program()))
}

/// Runs `command` to completion, or kills it once `seconds` pass (`timeout -s KILL`), where a hung
/// run must fail its step rather than stall the gate: a killed run is unsuccessful, its code absent.
pub fn run_within(command: &Command, seconds: u32) -> Result<Finished, String> {
    let mut bounded = Command::new("timeout");
    bounded.arg("-s").arg("KILL").arg(seconds.to_string()).arg(command.get_program()).args(command.get_args());
    for (key, value) in command.get_envs() {
        match value {
            Some(value) => { bounded.env(key, value); }
            None => { bounded.env_remove(key); }
        }
    }
    if let Some(directory) = command.get_current_dir() {
        bounded.current_dir(directory);
    }
    let mut finished = run(&mut bounded)?;
    // `timeout` exits 137 after its KILL; the report names the deadline.
    if finished.code == Some(137) {
        finished.success = false;
        finished.stderr.push_str(&format!("\nkilled after {seconds} s\n"));
    }
    Ok(finished)
}
