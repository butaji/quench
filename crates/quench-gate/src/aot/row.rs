//! One analogue measured: its program compiled ahead of time, the interpreter's run of the same
//! program as the reference, and the wall-clock time of the AOT binary against the C reference.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

use super::{analogue::Analogue, fresh, program};
use crate::{
    config::Config,
    process::{self, Finished},
    tiers::INTERPRETER,
};

/// Each timed binary runs this many times; the fastest run is its time.
const REPEATS: usize = 3;
const COMPILE_OPTION: &str = "--build-aot";
const OUTPUT_OPTION: &str = "-o";
const C_COMPILER: &str = "cc";
const C_OPTIMIZATION: &str = "-O2";
const C_LIBRARIES: &str = "-lm";
const JETSTREAM: &str = "quench-bench/jetstream";
const PROGRAM_NAME: &str = "program.js";
const C_NAME: &str = "c";
const AOT_NAME: &str = "aot";

/// The directory the analogue's program and binaries are built in.
fn scratch(config: &Config, row: &str) -> PathBuf {
    config.target.join("quench-gate").join("aot").join(row)
}

/// Runs `command` to success, or reports its excerpt under `what`.
fn checked(command: &mut Command, what: &str) -> Result<Finished, String> {
    let finished = process::run(command)?;
    if finished.success { Ok(finished) } else { Err(format!("{what} failed:\n{}", finished.excerpt())) }
}

/// The C reference: the analogue's source compiled with `cc -O2` into `directory`.
fn compile_c(config: &Config, analogue: &Analogue, directory: &Path) -> Result<PathBuf, String> {
    let binary = directory.join(C_NAME);
    checked(config.command(C_COMPILER).arg(C_OPTIMIZATION).arg(OUTPUT_OPTION).arg(&binary).arg(&analogue.source).arg(C_LIBRARIES), "the C compile")?;
    Ok(binary)
}

/// The program compiled ahead of time by `compiler` into `directory`.
fn compile_aot(config: &Config, compiler: &Path, program: &Path, directory: &Path) -> Result<PathBuf, String> {
    let binary = directory.join(AOT_NAME);
    checked(config.command(compiler).arg(COMPILE_OPTION).arg(program).arg(OUTPUT_OPTION).arg(&binary), "the AOT compile")?;
    Ok(binary)
}

/// Runs `command` once, returning what it wrote and how long it took.
fn timed(command: &mut Command) -> Result<(Finished, Duration), String> {
    let started = Instant::now();
    let finished = process::run(command)?;
    Ok((finished, started.elapsed()))
}

/// The fastest of `REPEATS` successful runs of `program` with `arguments`.
fn best(config: &Config, program: &Path, arguments: &[String]) -> Result<Duration, String> {
    let mut fastest: Option<Duration> = None;
    for _ in 0..REPEATS {
        let (finished, elapsed) = timed(config.command(program).args(arguments))?;
        if !finished.success {
            return Err(format!("{} failed:\n{}", program.display(), finished.excerpt()));
        }
        fastest = Some(fastest.map_or(elapsed, |lead| lead.min(elapsed)));
    }
    fastest.ok_or_else(|| "no timed run".to_owned())
}

/// Measures `analogue`: the AOT binary's wall-clock time over C's, once its output matches the
/// interpreter's. `host` is the interpreter, `compiler` the AOT-enabled `quench-node`.
pub fn measure(config: &Config, analogue: &Analogue, host: &Path, compiler: &Path) -> Result<f64, String> {
    let directory = scratch(config, &analogue.row);
    fresh(&directory)?;
    let script = directory.join(PROGRAM_NAME);
    let text = program::compose(analogue, &config.root.join(JETSTREAM))?;
    fs::write(&script, text).map_err(|error| format!("{}: {error}", script.display()))?;
    let c = compile_c(config, analogue, &directory)?;
    let aot = compile_aot(config, compiler, &script, &directory)?;
    let expected = checked(config.command(host).arg(INTERPRETER).arg(&script), "the interpreter")?;
    let actual = process::run(&mut config.command(&aot))?;
    if actual.stdout != expected.stdout || actual.code != expected.code {
        return Err(format!(
            "the AOT binary printed {:?} and exited {:?}; the interpreter printed {:?} and exited {:?}",
            actual.stdout, actual.code, expected.stdout, expected.code
        ));
    }
    let c_time = best(config, &c, &[analogue.iterations.to_string()])?;
    let aot_time = best(config, &aot, &[])?;
    Ok(aot_time.as_secs_f64() / c_time.as_secs_f64())
}
