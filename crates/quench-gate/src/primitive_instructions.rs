//! Step: the ops.js primitives' instruction counts, against the C column `ir_budget.txt` keeps.
//!
//! `ir-budget` measures each primitive under the interpreter with callgrind, which this host has
//! neither of. The same numbers can be read from machine code: each op of
//! `quench-bench/primitives/ops.js` runs through `run.cjs` on the u-jit with its regions dumped
//! (the route `c-parity` takes for the kernel corpus), and the hottest loop's instructions per
//! iteration are counted, exactly as the budget's C column counts `ops.c`'s loop step.
//!
//! The counts are recorded in `primitive-instructions-aarch64.txt` and only shrink. An op whose
//! count is over its budget (`3 x C + 6`) is a report line, not a failure: the budget is the
//! target the work closes.

use std::{collections::BTreeMap, fs, path::Path};

use crate::{build::NODE, config::Config, process, step::Verdict};

use crate::cparity::{asm, flow, stats};

/// The budget file: op, C instructions per iteration, budget (`3 x C + 6`), ceiling.
const BUDGETS: &str = "crates/quench-tier-corpus/ir_budget.txt";
const RUNNER: &str = "quench-bench/primitives/run.cjs";
const TABLE: &str = "crates/quench-tier-corpus/primitive-instructions-aarch64.txt";
/// Iterations the measured call runs: enough for the loop to reach its tier, short enough that
/// the run stays quick.
const ITERATIONS: &str = "20000";
const REPETITIONS: &str = "2";
const TRACE_VARIABLE: &str = "QUENCH_TRACE";
const TRACE_CHANNELS: &str = "js-jit,loop-kernel,jit-dump=";
const DEV_VARIABLE: &str = "QUENCH_DEV";
const SYNCHRONOUS_COMPILE: &str = "sync-compile";

/// One op's row of the budget file.
struct Budget {
    c: u32,
    budget: u32,
}

pub fn run(config: &Config) -> Verdict {
    let host = config.binary(NODE)?;
    let budgets = read_budgets(&config.root.join(BUDGETS))?;
    let runner = config.root.join(RUNNER);
    let directory = config.target.join("primitive-instructions");
    let recorded = read_table(&config.root.join(TABLE));
    let mut rows = Vec::with_capacity(budgets.len());
    for (op, budget) in &budgets {
        let dumps = directory.join(op);
        let _ = fs::remove_dir_all(&dumps);
        fs::create_dir_all(&dumps).map_err(|error| format!("{}: {error}", dumps.display()))?;
        let mut command = config.command(&host);
        command.arg("--u-jit").arg(&runner).arg(op).arg(ITERATIONS).arg(REPETITIONS)
            .env(TRACE_VARIABLE, format!("{TRACE_CHANNELS}{}", dumps.display()))
            .env(DEV_VARIABLE, SYNCHRONOUS_COMPILE);
        let run = process::run(&mut command)?;
        if !run.success {
            return Err(format!("{op}: {}", run.excerpt()));
        }
        rows.push((op.clone(), budget.c, budget.budget, hottest(&dumps, &run.stderr)));
    }
    let mut failures = Vec::new();
    for (op, _, _, loop_) in &rows {
        if let (Some(limit), Some(loop_)) = (recorded.get(op).copied().flatten(), loop_)
            && loop_.instructions > limit
        {
            failures.push(format!("{op}: {} instructions exceed its ceiling {limit}", loop_.instructions));
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
    let over = rows.iter().filter(|(_, _, budget, loop_)| loop_.as_ref().is_some_and(|loop_| loop_.instructions > *budget)).count();
    let calls = rows.iter().filter(|row| row.3.as_ref().is_some_and(|loop_| loop_.calls > 0)).count();
    Ok(format!("{measured} of {} ops measured, {over} over budget, {calls} with a callout in the loop; table {TABLE}", rows.len()))
}

/// One op's measured loop: its instructions per iteration and the calls it makes (a store or a
/// load lowered as a helper callout costs a call the C twin does not have).
struct Loop {
    instructions: u32,
    calls: u32,
}

/// The instructions per iteration of the op's hot loop: the dumped site the run's statistics
/// name, else its last loop dump.
fn hottest(directory: &Path, stderr: &str) -> Option<Loop> {
    let dumps = stats::dumps(stderr);
    let hot = stats::hot_loop(stderr);
    let chosen = hot.and_then(|hot| {
        dumps.iter().rev().find(|dump| dump.site == stats::Site::Loop { header: hot.header })
            .or_else(|| dumps.iter().rev().find(|dump| matches!(dump.site, stats::Site::Body { function, .. } if function == hot.function)))
    }).or_else(|| dumps.iter().rev().find(|dump| matches!(dump.site, stats::Site::Loop { .. })))?;
    let code = asm::disassemble(&directory.join(format!("{}.bin", chosen.name))).ok()?;
    flow::hottest(&code).map(|body| {
        let count = |holds: &dyn Fn(&str) -> bool| {
            u32::try_from(body.iter().filter(|instruction| holds(instruction.mnemonic())).count()).unwrap_or(u32::MAX)
        };
        Loop {
            instructions: u32::try_from(body.iter().filter(|instruction| !instruction.is_nop()).count()).unwrap_or(u32::MAX),
            calls: count(&|mnemonic| mnemonic == "bl" || mnemonic == "blr"),
        }
    })
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

fn render(rows: &[(String, u32, u32, Option<Loop>)]) -> String {
    let mut text = String::from(
        "# The ops.js primitives' hottest loops on the u-jit, instructions per iteration, against\n\
         # the C column `ir_budget.txt` keeps (`clang -O2`, loop step included) and its budget\n\
         # `3 x C + 6`. Measured from dumped machine code (`quench-gate --only\n\
         # primitive-instructions`); `-` is not measured. Counts only shrink. `calls` is the\n\
         # `bl`/`blr` instructions in the loop: a load or store the tier makes as a helper\n\
         # callout, which C does in place.\n\
         # op u-jit C budget calls\n");
    for (op, c, budget, loop_) in rows {
        let (instructions, calls) = loop_.as_ref().map_or_else(
            || ("-".to_owned(), "-".to_owned()),
            |loop_| (loop_.instructions.to_string(), loop_.calls.to_string()));
        text.push_str(&format!("{op} {instructions} {c} {budget} {calls}\n"));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_budget_row_is_read_with_its_c_count_and_budget() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let rows = read_budgets(&root.join(BUDGETS)).expect("budgets");
        let row = rows.get("arr_length").expect("arr_length");
        assert_eq!((row.c, row.budget), (4, 18));
    }
}
