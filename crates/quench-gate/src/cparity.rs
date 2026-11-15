//! Step 5: the C-parity gate. Each kernel (`crates/quench-tier-corpus/cparity/js`, with its C
//! reference in `.../c`) runs on every JIT tier with the compiled regions dumped. The hottest
//! loop's machine code is measured per iteration (`classes`): instruction count and the residual
//! classes C would not spend. The measurement is compared with the ratchet table
//! (`crates/quench-tier-corpus/cparity.txt`): growth fails, improvement lowers the ceiling,
//! and a tier's output must equal the interpreter's.
//!
//! The hot loop is the compiled region of the on-stack-replacement site that took the most
//! back-edges (`stats`), taken from the first tier's run (headers are bytecode positions, the
//! same on every tier); its hottest loop is `flow::hottest`. Dumps are deterministic only when
//! compiles run synchronously, so the runs set `QUENCH_DEV=sync-compile`.

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
};
use classes::Fields;
use table::{Table, Tally};

const KERNELS: &str = "crates/quench-tier-corpus/cparity/js";
const RATCHET: &str = "crates/quench-tier-corpus/cparity.txt";
const DUMPS: [&str; 2] = ["quench-gate", "cparity"];
const INTERPRETER: &str = "--no-jit";
const JAVASCRIPT: &str = "js";
const TRACE_VARIABLE: &str = "QUENCH_TRACE";
const TRACE_CHANNELS: &str = "js-jit,jit-dump=";
const DEV_VARIABLE: &str = "QUENCH_DEV";
const SYNCHRONOUS_COMPILE: &str = "sync-compile";

/// The kernel sources, in name order.
fn kernels(directory: &Path) -> io::Result<Vec<PathBuf>> {
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
    let run = process::run(&mut command)?;
    if run.success { Ok(run) } else { Err(format!("{} {flag} failed:\n{}", kernel.display(), run.excerpt())) }
}

/// The measure of the hottest loop in the region of `header`, if the run dumped one with a loop.
fn hottest(directory: &Path, stderr: &str, header: u32) -> Result<Option<Fields>, String> {
    let Some(dump) = stats::dumps(stderr).into_iter().rfind(|dump| dump.header == header) else {
        return Ok(None);
    };
    let code = asm::disassemble(&directory.join(format!("{}.bin", dump.name)))?;
    Ok(flow::hottest(&code).map(classes::measure))
}

/// Runs one kernel on every tier and settles each measurement against the ratchet.
fn measure(config: &Config, host: &Path, kernel: &Path, table: &mut Table, tally: &mut Tally) -> Result<(), String> {
    let name = kernel.file_stem().and_then(|stem| stem.to_str()).ok_or("a kernel has no name")?;
    let expected = node(config, host, kernel, INTERPRETER, None)?.stdout;
    let mut hot = None;
    for tier in Tier::ALL {
        let directory = fresh_directory(config, &format!("{name}-{}", tier.name())).map_err(|error| error.to_string())?;
        let run = node(config, host, kernel, &format!("--{}", tier.name()), Some(&directory))?;
        if run.stdout != expected {
            tally.failures.push(format!("{name} {}: printed {:?}, the interpreter {expected:?}", tier.name(), run.stdout));
            continue;
        }
        hot = stats::hot_header(&run.stderr).or(hot);
        let measured = hot.map(|header| hottest(&directory, &run.stderr, header)).transpose()?.flatten();
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
