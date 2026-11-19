//! Step: the ops.js primitives' **ahead** instruction counts.
//!
//! `primitive-instructions` measures each op on the run-time tier: it runs the ops through
//! `run.cjs`, which loads the module with `eval`, so nothing in it is a build-time unit. This
//! step measures the code a produced image *ships* for the same ops: `run-static.cjs` requires
//! the module, so the build compiles it into the image, and each op's loop is lowered ahead
//! (`AheadImages::compile`) and written by `QUENCH_TRACE=ahead-dump=<dir>` as
//! `loop-<function>-<header>-<backedge>.bin`.
//!
//! The op-to-function map comes from the run-time tier's own statistics (`QUENCH_TRACE=js-jit`):
//! the `ops.js` function whose sites took the most back-edges is the op's. Each mapped dump is
//! disassembled by the host's disassembler and measured per iteration by `cparity::flow` — the
//! same walk `c-parity` and `aot-instructions` use — and compared with `ir_budget.txt`'s C twin
//! and its budget (`3 x C + 6`).
//!
//! A loop is measured in the region the op ships for it, else in the function's body: a loop
//! whose iteration cannot run natively as a region (one that holds a call) still runs in the
//! body, which is what the image executes for it. The body is dumped as `body-<function>.bin`.
//!
//! Over budget is a report line, not a failure (as in `primitive-instructions`); the table
//! (`aot-primitives-aarch64.txt`) is a shrink-only ratchet. A build that embeds a compiling
//! process address fails the step: the image would fault where it runs.
//!
//! Alone: `quench-gate --only aot-primitives`.

use std::{collections::BTreeMap, fs, path::{Path, PathBuf}};

use crate::{build::NODE, config::Config, process, step::Verdict};

use crate::cparity::{asm, flow};

const BUDGETS: &str = "crates/quench-tier-corpus/ir_budget.txt";
const RUNNER: &str = "quench-bench/primitives/run-static.cjs";
const TABLE: &str = "crates/quench-tier-corpus/aot-primitives-aarch64.txt";
const MODULE: &str = "ops.js";
const IMAGE: &str = "ops";
const DUMPS: &str = "loops";
const STATS: &str = "QUENCH_JS_JIT_STATS ";
const EMBEDDED: &str = "embeds the compiling process's address";

/// One op's row of the budget file.
struct Budget {
    c: u32,
    budget: u32,
}

pub fn run(config: &Config) -> Verdict {
    let host = config.binary(NODE)?;
    let budgets = read_budgets(&config.root.join(BUDGETS))?;
    let runner = config.root.join(RUNNER);
    let directory = config.target.join("aot-primitives");
    let dumps = reset(&directory)?;
    let image = directory.join(IMAGE);
    let mut build = config.command(&host);
    build.arg("build").arg(&runner).arg("-o").arg(&image).arg("--dynamic-source")
        .env("QUENCH_TRACE", format!("ahead-dump={}", dumps.display()));
    let built = process::run(&mut build)?;
    if !built.success {
        return Err(format!("the primitives image did not build:\n{}", built.excerpt()));
    }
    if built.stderr.contains(EMBEDDED) {
        return Err(format!("the primitives image embeds a compiling process address:\n{}", built.excerpt()));
    }
    let recorded = read_table(&config.root.join(TABLE));
    let mut rows = Vec::with_capacity(budgets.len());
    for (op, budget) in &budgets {
        let function = op_function(config, &host, &runner, op)?;
        let instructions = function.and_then(|function| measure(&dumps, function));
        rows.push((op.clone(), budget.c, budget.budget, instructions));
    }
    let mut failures = Vec::new();
    for (op, _, _, ahead) in &rows {
        if let (Some(limit), Some(count)) = (recorded.get(op).copied().flatten(), *ahead)
            && count > limit
        {
            failures.push(format!("{op}: {count} instructions per iteration exceed its ceiling {limit}"));
        }
    }
    let rendered = render(&rows);
    if fs::read_to_string(&config.root.join(TABLE)).ok().as_deref() != Some(rendered.as_str()) {
        fs::write(&config.root.join(TABLE), rendered).map_err(|error| error.to_string())?;
    }
    if !failures.is_empty() {
        return Err(failures.join("\n"));
    }
    let measured = rows.iter().filter(|row| row.3.is_some()).count();
    let within = rows.iter().filter(|(_, _, budget, ahead)| ahead.is_some_and(|count| count <= *budget)).count();
    Ok(format!("{measured} of {} ops measured ahead, {within} within 3 x C + 6; table {TABLE}", rows.len()))
}

/// A fresh directory of dumps: what a previous run wrote is not this run's measurement.
fn reset(directory: &Path) -> Result<PathBuf, String> {
    match fs::remove_dir_all(directory) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(format!("{}: {error}", directory.display())),
        _ => {}
    }
    let dumps = directory.join(DUMPS);
    fs::create_dir_all(&dumps).map_err(|error| format!("{}: {error}", dumps.display()))?;
    Ok(dumps)
}

/// The `ops.js` function whose sites took the most back-edges in a run of `op`.
fn op_function(config: &Config, host: &Path, runner: &Path, op: &str) -> Result<Option<u32>, String> {
    let mut command = config.command(host);
    command.arg(runner).arg(op).env("QUENCH_TRACE", "js-jit");
    let run = process::run(&mut command)?;
    let mut best: Option<(u32, u64)> = None;
    for line in run.stderr.lines().filter(|line| line.starts_with(STATS)) {
        let Ok(stats) = serde_json::from_str::<serde_json::Value>(&line[STATS.len()..]) else { continue };
        let source = stats["source_name"].as_str().unwrap_or_default();
        if !source.ends_with(MODULE) { continue; }
        let Some(function) = stats["function"].as_u64() else { continue };
        let edges: u64 = stats["sites"].as_array().map_or(0, |sites| {
            sites.iter().filter_map(|site| site["lane_backedge_count"].as_u64()).sum()
        });
        let score = edges + stats["lane_entries"].as_u64().unwrap_or(0);
        if best.is_none_or(|(_, seen)| score > seen) {
            best = Some((u32::try_from(function).unwrap_or(u32::MAX), score));
        }
    }
    Ok(best.map(|(function, _)| function))
}

/// The instructions per iteration of the hottest ahead loop `function` ships: a loop region of
/// its own where one ships, else the body's own loop — a loop whose iteration cannot run
/// natively as a region (one holding a call) still runs in the body, which is what the image
/// executes for it.
fn measure(dumps: &Path, function: u32) -> Option<u32> {
    let region = |name: &str| name.starts_with(&format!("loop-{function}-")) && name.ends_with(".bin");
    let body = |name: &str| name == format!("body-{function}.bin");
    hottest(dumps, function, region).or_else(|| hottest(dumps, function, body))
}

/// The hottest loop's instructions per iteration among the dumps of `function` that `wanted`
/// names.
fn hottest(dumps: &Path, _function: u32, wanted: impl Fn(&str) -> bool) -> Option<u32> {
    let entries = fs::read_dir(dumps).ok()?;
    let mut best: Option<u32> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name()?.to_str()?;
        if !wanted(name) { continue; }
        let code = asm::disassemble(&path).ok()?;
        let Some(body) = flow::hottest(&code) else { continue };
        let count = u32::try_from(body.iter().filter(|instruction| !instruction.is_nop()).count()).unwrap_or(u32::MAX);
        best = Some(best.map_or(count, |seen| seen.max(count)));
    }
    best
}

fn read_budgets(path: &Path) -> Result<BTreeMap<String, Budget>, String> {
    let text = fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(text.lines().filter(|line| !line.starts_with('#')).filter_map(|line| {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let c = fields.get(1)?.parse().ok()?;
        let budget = fields.get(2)?.parse().ok()?;
        Some(((*fields.first()?).to_owned(), Budget { c, budget }))
    }).collect())
}

fn read_table(path: &Path) -> BTreeMap<String, Option<u32>> {
    fs::read_to_string(path).unwrap_or_default().lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            Some((fields.next()?.to_owned(), fields.next().and_then(|field| field.parse().ok())))
        })
        .collect()
}

fn render(rows: &[(String, u32, u32, Option<u32>)]) -> String {
    let mut text = String::from(
        "# The ops.js primitives' ahead code: the hottest loop each op ships in a produced image,\n\
         # instructions per iteration, against `ir_budget.txt`'s C twin and budget (3 x C + 6).\n\
         # `-` is an op whose loop neither a region nor its body lowers ahead. Measured by\n\
         # `quench-gate --only aot-primitives`; counts only shrink.\n\
         # op aot c budget\n");
    for (op, c, budget, ahead) in rows {
        let instructions = ahead.map_or_else(|| "-".to_owned(), |count| count.to_string());
        text.push_str(&format!("{op} {instructions} {c} {budget}\n"));
    }
    text
}
