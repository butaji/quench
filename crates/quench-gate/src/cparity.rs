//! Step 5: the C-parity gate. Each kernel (`crates/quench-tier-corpus/cparity/js`, with its C
//! reference in `.../c`) runs on every JIT tier with the compiled regions dumped. The hottest
//! loop's machine code is measured per iteration (`classes`): instruction count and the residual
//! classes C would not spend. The measurement is compared with the ratchet table
//! (`crates/quench-tier-corpus/cparity.txt`): growth fails, improvement lowers the ceiling,
//! and a tier's output must equal the interpreter's.
//!
//! The hot loop is the on-stack-replacement site that took the most back-edges (`stats`), taken
//! from the first tier's run (headers are bytecode positions, the same on every tier). It is
//! measured in the loop region compiled for it, else in the body compiled for its function; its
//! hottest loop is `flow::hottest`. A kernel with no loop (a recursive function) is measured
//! per call, along the scalar entry of the body that has one (`flow::call_path`). Dumps are
//! deterministic only when compiles run synchronously, so the runs set `QUENCH_DEV=sync-compile`.

mod asm;
mod classes;
mod flow;
mod stats;
mod table;

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use quench_tier_corpus::residuals::Tier;

use crate::{
    build::NODE,
    config::Config,
    process::{self, Finished},
    step::Verdict,
    tiers::{self, INTERPRETER},
};
use asm::Instruction;
use classes::Fields;
use stats::{Dump, Hot, Site};
use table::{Measured, Table, Tally};

pub const KERNELS: &str = "crates/quench-tier-corpus/cparity/js";
const RATCHET: &str = "crates/quench-tier-corpus/cparity.txt";
const DUMPS: [&str; 2] = ["quench-gate", "cparity"];
const JAVASCRIPT: &str = "js";
const TRACE_VARIABLE: &str = "QUENCH_TRACE";
const TRACE_CHANNELS: &str = "js-jit,loop-kernel,jit-dump=";
const DEV_VARIABLE: &str = "QUENCH_DEV";
const SYNCHRONOUS_COMPILE: &str = "sync-compile";
/// The longest one kernel run may take before it counts as hung.
const RUN_SECONDS: u32 = 300;

/// The kernel sources, in name order.
pub fn kernels(directory: &Path) -> io::Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path.extension().is_some_and(|extension| extension == JAVASCRIPT) {
            found.push(path);
        }
    }
    found.sort();
    Ok(found)
}

/// A fresh, empty directory for the dumps of one run.
fn fresh_directory(config: &Config, name: &str) -> io::Result<PathBuf> {
    let directory = DUMPS.iter().fold(config.target.clone(), |path, part| path.join(part)).join(name);
    match fs::remove_dir_all(&directory) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
        _ => {}
    }
    fs::create_dir_all(&directory)?;
    Ok(directory)
}

fn node(config: &Config, host: &Path, kernel: &Path, flag: &str, dumps: Option<&Path>) -> Result<Finished, String> {
    let mut command = config.command(host);
    command.arg(flag).arg(kernel);
    if let Some(directory) = dumps {
        command.env(TRACE_VARIABLE, format!("{TRACE_CHANNELS}{}", directory.display())).env(DEV_VARIABLE, SYNCHRONOUS_COMPILE);
    }
    // A kernel runs in seconds; one past the deadline is hung (a sync compile that never returns) and fails the step.
    let run = process::run_within(&command, RUN_SECONDS)?;
    if run.success { Ok(run) } else { Err(format!("{} {flag} failed:\n{}", kernel.display(), run.excerpt())) }
}

/// The disassembled code of `dump`.
fn code(directory: &Path, dump: &Dump) -> Result<Vec<Instruction>, String> {
    asm::disassemble(&directory.join(format!("{}.bin", dump.name)))
}

/// The hottest loop of the last dump compiled for `site`, if it has one.
fn loop_of(directory: &Path, dumps: &[Dump], site: impl Fn(Site) -> bool) -> Result<Option<Fields>, String> {
    let Some(dump) = dumps.iter().rfind(|dump| site(dump.site)) else {
        return Ok(None);
    };
    Ok(flow::hottest(&code(directory, dump)?).map(classes::measure))
}

/// The per-call path of the last body dumped with a scalar entry.
fn scalar_call(directory: &Path, dumps: &[Dump]) -> Result<Option<Fields>, String> {
    let Some((dump, entry)) = dumps.iter().rev().find_map(|dump| match dump.site {
        Site::Body { scalar: Some(entry), .. } => Some((dump, entry)),
        _ => None,
    }) else {
        return Ok(None);
    };
    Ok(flow::call_path(&code(directory, dump)?, entry).map(|path| classes::measure(&path)))
}

/// The measure of the program's hot code: the hot loop in its region or its function's body,
/// or the interpreter's kernel running it, else the per-call path of a scalar entry.
fn hottest(directory: &Path, stderr: &str, hot: Option<Hot>) -> Result<Measured, String> {
    let dumps = stats::dumps(stderr);
    if let Some(hot) = hot {
        if let Some(fields) = loop_of(directory, &dumps, |site| site == Site::Loop { header: hot.header })? {
            return Ok(Measured::Code(fields));
        }
        if let Some(fields) = loop_of(directory, &dumps, |site| matches!(site, Site::Body { function, .. } if function == hot.function))? {
            return Ok(Measured::Code(fields));
        }
        if stats::kernel_runs(stderr, hot.header) {
            return Ok(Measured::Kernel);
        }
    }
    // A tier that reports no loop statistics (b-jit) has its regions' dumps alone.
    if let Some(fields) = loop_of(directory, &dumps, |site| matches!(site, Site::Loop { .. }))? {
        return Ok(Measured::Code(fields));
    }
    Ok(scalar_call(directory, &dumps)?.map_or(Measured::Nothing, Measured::Code))
}

/// Runs one kernel on every tier and settles each measurement against the ratchet.
fn measure(config: &Config, host: &Path, kernel: &Path, table: &mut Table, tally: &mut Tally) -> Result<(), String> {
    let name = kernel.file_stem().and_then(|stem| stem.to_str()).ok_or("a kernel has no name")?;
    let expected = node(config, host, kernel, INTERPRETER, None)?.stdout;
    let mut hot = None;
    for tier in Tier::ALL {
        let directory = fresh_directory(config, &format!("{name}-{}", tier.name())).map_err(|error| error.to_string())?;
        let run = node(config, host, kernel, &tiers::flag(tier), Some(&directory))?;
        if run.stdout != expected {
            tally.failures.push(format!("{name} {}: printed {:?}, the interpreter {expected:?}", tier.name(), run.stdout));
            continue;
        }
        hot = stats::hot_loop(&run.stderr).or(hot);
        let measured = hottest(&directory, &run.stderr, hot)?;
        table.settle(name, tier.name(), measured, config.record, tally);
    }
    Ok(())
}

pub fn run(config: &Config) -> Verdict {
    let host = config.binary(NODE)?;
    let ratchet = config.root.join(RATCHET);
    let before = fs::read_to_string(&ratchet).map_err(|error| format!("{}: {error}", ratchet.display()))?;
    let mut table = Table::parse(&before)?;
    let sources = kernels(&config.root.join(KERNELS)).map_err(|error| format!("{KERNELS}: {error}"))?;
    let mut tally = Tally::default();
    for kernel in &sources {
        if let Err(error) = measure(config, &host, kernel, &mut table, &mut tally) {
            tally.failures.push(error);
        }
    }
    let after = table.render();
    if after != before {
        fs::write(&ratchet, after).map_err(|error| format!("{}: {error}", ratchet.display()))?;
    }
    if !tally.failures.is_empty() {
        return Err(tally.failures.join("\n"));
    }
    Ok(format!(
        "{} kernels: {} ceilings lowered, {} recorded, {} not recorded yet (--record)",
        sources.len(),
        tally.lowered,
        tally.recorded,
        tally.unrecorded
    ))
}
