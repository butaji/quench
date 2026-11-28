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
    tiers::{AOT, INTERPRETER, launch},
};

/// Each timed binary runs this many times; the fastest run is its time.
const REPEATS: usize = 3;
const OUTPUT_OPTION: &str = "-o";
const C_COMPILER: &str = "cc";
const CXX_COMPILER: &str = "c++";
/// The extensions of C++ sources: an analogue with one of them compiles with `c++`.
const CXX_EXTENSIONS: [&str; 2] = ["cc", "cpp"];
const C_OPTIMIZATION: &str = "-O2";
const C_LIBRARIES: &str = "-lm";
const PROGRAM_NAME: &str = "program.js";
const C_NAME: &str = "c";

/// The directory the analogue's program and binaries are built in.
fn scratch(config: &Config, row: &str) -> PathBuf {
    config.target.join("quench-gate").join("aot").join(row)
}

/// Runs `command` to success, or reports its excerpt under `what`.
fn checked(command: &mut Command, what: &str) -> Result<Finished, String> {
    let finished = process::run(command)?;
    if finished.success { Ok(finished) } else { Err(format!("{what} failed:\n{}", finished.excerpt())) }
}

/// Whether `path` names a C++ source.
fn is_cxx_source(path: &Path) -> bool {
    path.extension().and_then(|extension| extension.to_str()).is_some_and(|extension| CXX_EXTENSIONS.contains(&extension))
}

/// The compiler for `analogue`: `c++` when its driver or one of its `c-sources` is C++, else `cc`.
fn c_compiler(analogue: &Analogue) -> &'static str {
    let cxx = is_cxx_source(&analogue.source) || analogue.c_sources.iter().any(|token| is_cxx_source(Path::new(token)));
    if cxx { CXX_COMPILER } else { C_COMPILER }
}

/// A `c-sources` token as cc takes it: `-I` directories and sources resolve against `jetstream`,
/// other flags pass through unchanged.
fn resolve(token: &str, jetstream: &Path) -> String {
    if let Some(directory) = token.strip_prefix("-I") {
        format!("-I{}", jetstream.join(directory).display())
    } else if token.starts_with('-') {
        token.to_owned()
    } else {
        jetstream.join(token).display().to_string()
    }
}

/// The compile inputs in cc's order: the driver, then the `c-sources` tokens resolved against
/// `jetstream`. A path after `-include` is a header of the analogue's own directory, so it
/// resolves there.
fn compile_inputs(analogue: &Analogue, jetstream: &Path) -> Vec<String> {
    let directory = analogue.source.parent().unwrap_or(Path::new("."));
    let mut inputs = vec![analogue.source.display().to_string()];
    let mut included = false;
    for token in &analogue.c_sources {
        let resolved = if included { directory.join(token).display().to_string() } else { resolve(token, jetstream) };
        included = token == "-include";
        inputs.push(resolved);
    }
    inputs
}

/// The C reference: the analogue's driver and sources compiled with `-O2` into `directory`.
fn compile_c(config: &Config, analogue: &Analogue, directory: &Path) -> Result<PathBuf, String> {
    let binary = directory.join(C_NAME);
    let jetstream = config.jetstream.clone();
    let mut command = config.command(c_compiler(analogue));
    command.arg(C_OPTIMIZATION).arg(OUTPUT_OPTION).arg(&binary).args(compile_inputs(analogue, &jetstream)).arg(C_LIBRARIES);
    checked(&mut command, "the C compile")?;
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
/// interpreter's. `host` is the interpreter.
pub fn measure(config: &Config, analogue: &Analogue, host: &Path) -> Result<f64, String> {
    let directory = scratch(config, &analogue.row);
    fresh(&directory)?;
    let script = directory.join(PROGRAM_NAME);
    let text = program::compose(analogue, &config.jetstream)?;
    fs::write(&script, text).map_err(|error| format!("{}: {error}", script.display()))?;
    let c = compile_c(config, analogue, &directory)?;
    let aot = PathBuf::from(launch(config, host, AOT, &script, &directory, &[])?.swap_remove(0));
    let expected = checked(config.command(host).arg(INTERPRETER).arg(&script), "the interpreter")?;
    let actual = process::run(&mut config.command(&aot))?;
    if actual.stdout != expected.stdout || actual.code != expected.code {
        return Err(format!(
            "the AOT binary printed {:?} and exited {:?}; the interpreter printed {:?} and exited {:?}",
            actual.stdout, actual.code, expected.stdout, expected.code
        ));
    }
    let c_arguments = [analogue.iterations.to_string(), config.jetstream.display().to_string()];
    let c_time = best(config, &c, &c_arguments)?;
    let aot_time = best(config, &aot, &[])?;
    Ok(aot_time.as_secs_f64() / c_time.as_secs_f64())
}
