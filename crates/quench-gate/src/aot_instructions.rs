//! Step: the produced image's own ahead code, measured per iteration.
//!
//! `c-parity` measures the loop a run-time tier compiles; this step measures the loop an
//! *image* runs. Each corpus kernel is built (`quench-node build … --dynamic-source`) with
//! `QUENCH_TRACE=ahead-dump=<dir>`, which makes the build write every ahead-lowered loop's
//! relocatable code as `loop-<function>-<header>-<backedge>.bin` (`AheadImages::compile`).
//! Each dump is disassembled by the host's disassembler (`cparity::asm`), and the executed
//! path of its hot loop is counted (`cparity::flow`), as `c-parity` counts a region's.
//!
//! The representative loop is the one C measures: the largest loop that holds no other, in
//! the function that has one. `loops` is how many ahead loops the program ships, so a kernel
//! whose loop is not lowered ahead reads `0` and `-`.
//!
//! The table (`aot-instructions-aarch64.txt`) is the shrink-only ratchet, beside the C twin's
//! scalar loop from `cparity-c-aarch64.txt`.
//!
//! Alone: `quench-gate --only aot-instructions`.

use std::{collections::BTreeMap, fs, path::{Path, PathBuf}, process::Command};

use crate::{build::NODE, config::Config, step::Verdict};

use crate::cparity::{asm, flow};

const KERNELS: &str = "crates/quench-tier-corpus/cparity/js";
const C_TABLE: &str = "crates/quench-tier-corpus/cparity-c-aarch64.txt";
const TABLE: &str = "crates/quench-tier-corpus/aot-instructions-aarch64.txt";
const LOOP_PREFIX: &str = "loop-";
/// The table's metric, named in its header: a table written for another one is re-recorded.
const METRIC: &str = "ahead-loop";

/// One kernel's ahead code: its loops, their spans (header..backedge, per function) and the
/// instructions the representative loop spends per iteration.
#[derive(Default)]
struct Ahead {
    spans: Vec<(u32, u32, u32)>,
    instructions: Option<u32>,
}

pub fn run(config: &Config) -> Verdict {
    let host = config.binary(NODE)?;
    let directory = config.target.join("aot-instructions");
    reset(&directory)?;
    let sources = kernels(&config.root.join(KERNELS))?;
    let c_costs = c_costs(&config.root.join(C_TABLE))?;
    let recorded = read_table(&config.root.join(TABLE));
    let mut rows = Vec::with_capacity(sources.len());
    for (name, source) in &sources {
        let image = directory.join(name);
        let loops = directory.join(format!("{name}-loops"));
        fs::create_dir_all(&loops).map_err(|error| format!("{}: {error}", loops.display()))?;
        let output = Command::new(&host).arg("build").arg(source).arg("-o").arg(&image)
            .arg("--dynamic-source")
            .env("QUENCH_TRACE", format!("ahead-dump={}", loops.display()))
            .output().map_err(|error| error.to_string())?;
        if !output.status.success() {
            return Err(format!("{name}: build failed: {}", String::from_utf8_lossy(&output.stderr).trim()));
        }
        rows.push((name.clone(), measure(&loops)?));
    }
    let mut failures = Vec::new();
    for (name, ahead) in &rows {
        if let (Some(limit), Some(count)) = (recorded.get(name).copied().flatten(), ahead.instructions)
            && count > limit
        {
            failures.push(format!("{name}: {count} instructions per iteration exceed its ceiling {limit}"));
        }
    }
    let rendered = render(&rows, &c_costs);
    if fs::read_to_string(&config.root.join(TABLE)).ok().as_deref() != Some(rendered.as_str()) {
        fs::write(&config.root.join(TABLE), rendered).map_err(|error| error.to_string())?;
    }
    if !failures.is_empty() {
        return Err(failures.join("\n"));
    }
    let lowered = rows.iter().filter(|(_, ahead)| ahead.instructions.is_some()).count();
    Ok(format!("{lowered} of {} kernels ship their work loop ahead; table {TABLE}", rows.len()))
}

/// A fresh dump directory: what a previous run wrote is not this run's measurement.
fn reset(directory: &Path) -> Result<(), String> {
    match fs::remove_dir_all(directory) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(format!("{}: {error}", directory.display())),
        _ => {}
    }
    fs::create_dir_all(directory).map_err(|error| format!("{}: {error}", directory.display()))
}

/// The ahead loops `directory` holds, measured: every dump's hot loop is counted, and the
/// representative one is the largest loop that holds no other in its function.
fn measure(directory: &Path) -> Result<Ahead, String> {
    let mut measured: Vec<(u32, u32, u32, u32)> = Vec::new();
    for entry in fs::read_dir(directory).map_err(|error| format!("{}: {error}", directory.display()))? {
        let path = entry.map_err(|error| error.to_string())?.path();
        let Some((function, header, backedge)) = loop_of(&path) else { continue };
        let code = asm::disassemble(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        let Some(body) = flow::hottest(&code) else { continue };
        let count = u32::try_from(body.iter().filter(|instruction| !instruction.is_nop()).count()).unwrap_or(u32::MAX);
        measured.push((function, header, backedge, count));
    }
    let ahead = Ahead {
        spans: measured.iter().map(|(function, header, backedge, _)| (*function, *header, *backedge)).collect(),
        instructions: innermost(&measured),
    };
    Ok(ahead)
}

/// The representative loop of `measured`: the largest loop that holds no other, in its
/// function — the specialized version a region runs once its entry checks pass.
fn innermost(measured: &[(u32, u32, u32, u32)]) -> Option<u32> {
    let holds = |outer: &(u32, u32, u32, u32), inner: &(u32, u32, u32, u32)| {
        outer != inner && outer.0 == inner.0 && outer.1 <= inner.1 && inner.2 <= outer.2
    };
    measured
        .iter()
        .filter(|loop_| !measured.iter().any(|other| holds(loop_, other)))
        .max_by_key(|(_, header, backedge, _)| backedge - header)
        .map(|(_, _, _, count)| *count)
}

/// `loop-<function>-<header>-<backedge>.bin`'s numbers.
fn loop_of(path: &Path) -> Option<(u32, u32, u32)> {
    let name = path.file_name()?.to_str()?.strip_prefix(LOOP_PREFIX)?.strip_suffix(".bin")?;
    let mut fields = name.split('-').map(str::parse::<u32>);
    Some((fields.next()?.ok()?, fields.next()?.ok()?, fields.next()?.ok()?))
}

fn kernels(directory: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    let entries = fs::read_dir(directory).map_err(|error| format!("{}: {error}", directory.display()))?;
    let mut kernels = Vec::new();
    for entry in entries {
        let path = entry.map_err(|error| error.to_string())?.path();
        if path.extension().is_some_and(|extension| extension == "js")
            && let Some(name) = path.file_stem().and_then(|stem| stem.to_str())
        {
            kernels.push((name.to_owned(), path));
        }
    }
    kernels.sort();
    Ok(kernels)
}

/// The C twin's scalar work loop per kernel (`cparity-c-aarch64.txt`).
fn c_costs(path: &Path) -> Result<BTreeMap<String, u32>, String> {
    let text = fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(text.lines().filter(|line| !line.starts_with('#')).filter_map(|line| {
        let fields: Vec<&str> = line.split_whitespace().collect();
        Some(((*fields.first()?).to_owned(), fields.get(2)?.parse().ok()?))
    }).collect())
}

fn read_table(path: &Path) -> BTreeMap<String, Option<u32>> {
    let text = fs::read_to_string(path).unwrap_or_default();
    // A table measured another way says nothing about this one.
    if !text.lines().next().unwrap_or_default().contains(METRIC) {
        return BTreeMap::new();
    }
    text.lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            Some((fields.next()?.to_owned(), fields.nth(1).and_then(|field| field.parse().ok())))
        })
        .collect()
}

fn render(rows: &[(String, Ahead)], c_costs: &BTreeMap<String, u32>) -> String {
    let mut text = format!(
        "# {METRIC}: the work loop of the ahead code a produced image ships, instructions per\n\
         # iteration, against the C twin's scalar loop (`cparity-c-aarch64.txt`); `loops` is how\n\
         # many ahead loops the program ships, and `-` that its work loop is not among them.\n\
         # Measured by building each kernel with `QUENCH_TRACE=ahead-dump=<dir>` and\n\
         # disassembling every `loop-*` dump (`quench-gate --only aot-instructions`).\n\
         # Counts only shrink.\n\
         # kernel loops aot c-aot\n");
    for (name, ahead) in rows {
        let loops = ahead.spans.len();
        let instructions = ahead.instructions.map_or_else(|| "-".to_owned(), |count| count.to_string());
        let c = c_costs.get(name).map_or_else(|| "-".to_owned(), |count| count.to_string());
        text.push_str(&format!("{name} {loops} {instructions} {c}\n"));
    }
    text
}
