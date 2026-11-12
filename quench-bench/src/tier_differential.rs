//! The cross-tier differential: seeded random programs (`quench-tier-corpus`) run under every
//! execution mode of one Quench binary, and every mode must exit alike and print the same digest.
//!
//! `--tier-differential --seed S --count N --quench BIN [--timeout-ms T] [--iterations I]
//! [--out-dir DIR]`. A mismatching program is saved to the output directory with a report of
//! each mode's outcome. Modes: `--no-jit` (the reference), `--b-jit`, `--u-jit`, the default,
//! and an AOT executable when `BIN compile` works. Forced deopt (R26) is a test-only runtime
//! hook: `cargo test -p quench-runtime --lib forced_deopt_corpus` runs the same corpus through it.

use super::{fail, usage};
use quench_tier_corpus::{generate, program_seed, Options, Program, Sink, DEFAULT_ITERATIONS};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    time::Duration,
};

pub(crate) const FLAG: &str = "--tier-differential";

const DEFAULT_SEED: u64 = 1;
const DEFAULT_COUNT: u64 = 20;
const DEFAULT_TIMEOUT_MS: u64 = 60_000;
const STDERR_TAIL_BYTES: usize = 400;
const REFERENCE: &str = "no-jit";
const AOT_LABEL: &str = "aot";
const SIGKILL: i32 = 9;

/// Mode labels and the flag each adds to the command line; the reference comes first.
const MODES: &[(&str, Option<&str>)] =
    &[(REFERENCE, Some("--no-jit")), ("b-jit", Some("--b-jit")), ("u-jit", Some("--u-jit")), ("default", None)];

struct Config {
    seed: u64,
    count: u64,
    quench: PathBuf,
    timeout: Duration,
    iterations: u32,
    out_dir: PathBuf,
}

#[derive(PartialEq, Eq)]
struct Outcome {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

impl Outcome {
    /// Modes agree when they exit alike and print alike; stderr carries host noise.
    fn agrees_with(&self, other: &Self) -> bool {
        self.code == other.code && self.stdout == other.stdout
    }

    fn describe(&self) -> String {
        format!("exit {:?}\nstdout: {}\nstderr tail: {}", self.code, self.stdout.trim_end(), self.stderr)
    }
}

fn parse_value<T: std::str::FromStr>(value: Option<&String>, flag: &str) -> T {
    value.and_then(|value| value.parse().ok()).unwrap_or_else(|| usage(&format!("{flag} requires a number")))
}

fn parse(args: &[String]) -> Config {
    let mut config = Config {
        seed: DEFAULT_SEED,
        count: DEFAULT_COUNT,
        quench: PathBuf::new(),
        timeout: Duration::from_millis(DEFAULT_TIMEOUT_MS),
        iterations: DEFAULT_ITERATIONS,
        out_dir: std::env::temp_dir().join("quench-tier-differential"),
    };
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            FLAG => {}
            "--seed" => config.seed = parse_value(args.next(), arg),
            "--count" => config.count = parse_value(args.next(), arg),
            "--iterations" => config.iterations = parse_value(args.next(), arg),
            "--timeout-ms" => config.timeout = Duration::from_millis(parse_value(args.next(), arg)),
            "--quench" => config.quench = args.next().map(PathBuf::from).unwrap_or_else(|| usage("--quench requires a path")),
            "--out-dir" => config.out_dir = args.next().map(PathBuf::from).unwrap_or_else(|| usage("--out-dir requires a path")),
            other => usage(&format!("--tier-differential: unknown argument {other}")),
        }
    }
    if config.quench.as_os_str().is_empty() {
        usage("--tier-differential requires --quench BIN");
    }
    config
}

/// Runs `command` to completion or kills it at the timeout (reported as no exit code).
fn run_command(mut command: Command, timeout: Duration) -> Outcome {
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let child = command.spawn().unwrap_or_else(|error| fail(&format!("cannot run {:?}: {error}", command.get_program())));
    let pid = child.id() as i32;
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || sender.send(child.wait_with_output()));
    let output = receiver.recv_timeout(timeout).unwrap_or_else(|_| {
        // SAFETY: `pid` names the child this function spawned and has not yet reaped.
        unsafe { libc::kill(pid, SIGKILL) };
        receiver.recv().expect("reaper thread")
    });
    let output = output.unwrap_or_else(|error| fail(&format!("wait failed: {error}")));
    let tail_start = output.stderr.len().saturating_sub(STDERR_TAIL_BYTES);
    Outcome {
        code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr[tail_start..]).into_owned(),
    }
}

fn run_mode(config: &Config, flag: Option<&str>, file: &Path) -> Outcome {
    let mut command = Command::new(&config.quench);
    command.args(flag).arg(file);
    run_command(command, config.timeout)
}

/// Compiles `file` to an executable and runs it; `None` when the binary cannot compile.
fn run_aot(config: &Config, file: &Path) -> Option<Outcome> {
    let image = file.with_extension("aot");
    let mut compile = Command::new(&config.quench);
    compile.arg("compile").arg(file).arg("-o").arg(&image);
    let built = run_command(compile, config.timeout);
    let outcome = (built.code == Some(0) && image.exists()).then(|| run_command(Command::new(&image), config.timeout));
    let _ = fs::remove_file(&image);
    outcome
}

fn run_all(config: &Config, file: &Path, aot: bool) -> Vec<(&'static str, Outcome)> {
    let mut outcomes: Vec<_> =
        MODES.iter().map(|&(label, flag)| (label, run_mode(config, flag, file))).collect();
    outcomes.extend(aot.then(|| run_aot(config, file)).flatten().map(|outcome| (AOT_LABEL, outcome)));
    outcomes
}

fn mismatches(outcomes: &[(&'static str, Outcome)]) -> Vec<&'static str> {
    let reference = &outcomes[0].1;
    outcomes[1..].iter().filter(|(_, outcome)| !outcome.agrees_with(reference)).map(|(label, _)| *label).collect()
}

fn save_failure(config: &Config, program: &Program, outcomes: &[(&'static str, Outcome)], bad: &[&str]) -> PathBuf {
    let path = config.out_dir.join(format!("mismatch-{:016x}.js", program.seed));
    fs::write(&path, &program.source).unwrap_or_else(|error| fail(&format!("{}: {error}", path.display())));
    let report: String = outcomes.iter().map(|(label, outcome)| format!("== {label}\n{}\n", outcome.describe())).collect();
    let header = format!("features: {}\nmismatching modes: {}\n", program.features.join(","), bad.join(","));
    let _ = fs::write(path.with_extension("txt"), header + &report);
    path
}

/// A binary supports AOT when it compiles a trivial program and the image runs.
fn probe_aot(config: &Config, scratch: &Path) -> bool {
    let file = scratch.join("probe.js");
    fs::write(&file, "console.log('digest', 0);\n").unwrap_or_else(|error| fail(&error.to_string()));
    run_aot(config, &file).is_some_and(|outcome| outcome.code == Some(0))
}

/// Runs the corpus; the exit status is non-zero on any mismatch.
pub(crate) fn run(args: &[String]) -> i32 {
    let config = parse(args);
    fs::create_dir_all(&config.out_dir).unwrap_or_else(|error| fail(&format!("{}: {error}", config.out_dir.display())));
    let aot = probe_aot(&config, &config.out_dir);
    println!("tier differential: seed {} count {} aot {}", config.seed, config.count, if aot { "yes" } else { "unsupported" });
    let options = Options { iterations: config.iterations, sink: Sink::Print };
    let mut failures = 0;
    for index in 0..config.count {
        let program = generate(program_seed(config.seed, index), options);
        let file = config.out_dir.join(format!("current-{index}.js"));
        fs::write(&file, &program.source).unwrap_or_else(|error| fail(&error.to_string()));
        let outcomes = run_all(&config, &file, aot);
        let _ = fs::remove_file(&file);
        let bad = mismatches(&outcomes);
        let status = if bad.is_empty() { "ok".to_string() } else { format!("MISMATCH {}", bad.join(",")) };
        println!("{index}: seed {:016x} [{}] {status}", program.seed, program.features.join(","));
        if !bad.is_empty() {
            failures += 1;
            println!("  saved {}", save_failure(&config, &program, &outcomes, &bad).display());
        }
    }
    println!("tier differential: {failures} mismatch(es) in {} program(s)", config.count);
    i32::from(failures > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(code: i32, stdout: &str) -> Outcome {
        Outcome { code: Some(code), stdout: stdout.into(), stderr: String::new() }
    }

    #[test]
    fn modes_agree_on_exit_and_stdout_only() {
        let reference = outcome(0, "digest 5\n");
        assert!(Outcome { stderr: "noise".into(), ..outcome(0, "digest 5\n") }.agrees_with(&reference));
        assert!(!outcome(0, "digest 6\n").agrees_with(&reference));
        assert!(!outcome(1, "digest 5\n").agrees_with(&reference));
    }

    #[test]
    fn the_reference_mode_is_first() {
        assert_eq!(MODES[0].0, REFERENCE);
    }

    #[test]
    fn own_flags_parse() {
        let args = [FLAG, "--seed", "9", "--count", "3", "--quench", "bin"].map(String::from);
        let config = parse(&args);
        assert_eq!((config.seed, config.count, config.quench), (9, 3, PathBuf::from("bin")));
    }

    #[test]
    fn mismatching_modes_are_named() {
        let outcomes = vec![(REFERENCE, outcome(0, "a")), ("b-jit", outcome(0, "b")), ("default", outcome(0, "a"))];
        assert_eq!(mismatches(&outcomes), ["b-jit"]);
    }
}
