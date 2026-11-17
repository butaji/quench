//! Step: every language primitive has an instruction budget. Each op of
//! `quench-bench/primitives/ops.js` runs under the interpreter in one callgrind process, between
//! profiling marks (`process.__irMark`, `QUENCH_DEV=ir-mark`, `--dump-before=quench_ir_mark`), so
//! its instructions per loop iteration are exact: (Ir(2n) - Ir(n)) / n, with start-up, warm-up and
//! per-call costs cancelled. `crates/quench-tier-corpus/ir_budget.txt` holds, per op, the C
//! twin's count (`ops.c`), the budget (3x C plus one dispatch allowance) and the ceiling: the
//! measured count that may not grow. A drop lowers the ceiling; a count at or under its budget
//! makes the budget the ceiling. Ops over budget are the debt the report ranks.
//!
//! Alone: `quench-gate --only ir-budget`. `QUENCH_IR_BUDGET_OPS=a,b` measures only those ops,
//! `QUENCH_IR_BUDGET_MODE=b-jit` (or `u-jit`) measures a JIT tier against the same budgets
//! without touching the ceilings, which hold for the interpreter.

use std::{collections::BTreeMap, env, fs, path::Path};

use crate::{build::NODE, config::Config, process, step::Verdict, tiers::INTERPRETER};

const BUDGETS: &str = "crates/quench-tier-corpus/ir_budget.txt";
const RUNNER: &str = "quench-bench/primitives/ir_budget.cjs";
/// Iterations of the shorter measured run; the longer runs twice as many.
const ITERATIONS: u64 = 20_000;
const MARK: &str = "quench_ir_mark";
/// Marks per op: before op(n), between op(n) and op(2n), after op(2n).
const MARKS_PER_OP: usize = 3;
const OPS_VARIABLE: &str = "QUENCH_IR_BUDGET_OPS";
const MODE_VARIABLE: &str = "QUENCH_IR_BUDGET_MODE";
const LINE_PREFIX: &str = "IRBUDGET ";
/// A count within this many instructions per iteration of its ceiling is the same count.
const SAME_COUNT: f64 = 0.5;
/// Ceilings are kept to tenths of an instruction per iteration.
const CEILING_STEPS_PER_INSTRUCTION: f64 = 10.0;
/// The columns of a budget row after its op name.
const C_COLUMN: usize = 1;
const BUDGET_COLUMN: usize = 2;
const CEILING_COLUMN: usize = 3;
/// The marks bounding op(2n) among an op's marks.
const DOUBLE_RUN_START: usize = 1;
const DOUBLE_RUN_END: usize = 2;

/// One op's row in the budget file.
#[derive(Clone, Copy)]
struct Row {
    c: f64,
    budget: f64,
    ceiling: Option<f64>,
}

fn parse(text: &str) -> BTreeMap<String, Row> {
    let rows = text.lines().filter(|line| !line.starts_with('#')).map(|line| line.split_whitespace().collect::<Vec<_>>());
    rows.filter_map(|fields| {
        let number = |index: usize| fields.get(index).and_then(|field| field.parse::<f64>().ok());
        Some(((*fields.first()?).to_owned(), Row { c: number(C_COLUMN)?, budget: number(BUDGET_COLUMN)?, ceiling: number(CEILING_COLUMN) }))
    })
    .collect()
}

fn header(text: &str) -> String {
    text.lines().take_while(|line| line.starts_with('#')).map(|line| format!("{line}\n")).collect()
}

fn render(header: &str, rows: &BTreeMap<String, Row>) -> String {
    let body: String = rows.iter().map(|(name, row)| match row.ceiling {
        Some(ceiling) => format!("{name} {} {} {}\n", row.c, row.budget, ceiling),
        None => format!("{name} {} {}\n", row.c, row.budget),
    }).collect();
    format!("{header}{body}")
}

/// The total instructions of each callgrind dump in `directory`, in dump order.
fn dumps(directory: &Path) -> Result<Vec<u64>, String> {
    let mut numbered = Vec::new();
    for entry in fs::read_dir(directory).map_err(|error| format!("{}: {error}", directory.display()))? {
        let path = entry.map_err(|error| error.to_string())?.path();
        let Some(index) = path.extension().and_then(|extension| extension.to_str()?.parse::<u64>().ok()) else { continue };
        let text = fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        let total = text.lines().find_map(|line| line.strip_prefix("summary: ").or_else(|| line.strip_prefix("totals: ")));
        numbered.push((index, total.and_then(|total| total.split_whitespace().next()?.parse::<u64>().ok()).unwrap_or(0)));
    }
    numbered.sort_unstable();
    Ok(numbered.into_iter().map(|(_, total)| total).collect())
}

/// Instructions per iteration of each op the runner measured, in run order.
fn measure(config: &Config, mode: &str, ops: &[String]) -> Result<Vec<(String, f64)>, String> {
    let host = config.binary(NODE)?;
    let scratch = config.target.join("ir-budget");
    let _ = fs::remove_dir_all(&scratch);
    fs::create_dir_all(&scratch).map_err(|error| format!("{}: {error}", scratch.display()))?;
    let mut command = config.command("valgrind");
    command.arg("--tool=callgrind").arg(format!("--dump-before={MARK}")).arg("--dump-instr=no")
        .arg(format!("--callgrind-out-file={}", scratch.join("cg.out").display()))
        .arg(host).arg(mode).arg(RUNNER).arg(ITERATIONS.to_string()).args(ops)
        .env("QUENCH_DEV", if mode == INTERPRETER { "ir-mark" } else { "ir-mark,sync-compile" })
        .env("QUENCH_COMPILE_CACHE", "").env("QUENCH_STARTUP_SNAPSHOT", "off").env("XDG_CACHE_HOME", &scratch);
    let run = process::run(&mut command)?;
    let names: Vec<String> = run.stdout.lines().filter_map(|line| line.strip_prefix(LINE_PREFIX)).map(str::to_owned).collect();
    if !run.success || names.is_empty() {
        return Err(format!("{RUNNER} failed under callgrind\n{}", run.excerpt()));
    }
    let totals = dumps(&scratch)?;
    let _ = fs::remove_dir_all(&scratch);
    if totals.len() < names.len() * MARKS_PER_OP {
        return Err(format!("{} callgrind dumps for {} ops; expected {} marks each", totals.len(), names.len(), MARKS_PER_OP));
    }
    // Dump k holds the counts from mark k-1 to mark k: per op, op(n) then op(2n).
    let per_iteration = |index: usize| (totals[index * MARKS_PER_OP + DOUBLE_RUN_END] as f64 - totals[index * MARKS_PER_OP + DOUBLE_RUN_START] as f64) / ITERATIONS as f64;
    Ok(names.into_iter().enumerate().map(|(index, name)| (name, per_iteration(index))).collect())
}

/// The report: every measured op, worst budget ratio first.
fn table(measured: &[(String, f64)], rows: &BTreeMap<String, Row>) -> String {
    let mut lines: Vec<(f64, String)> = measured.iter().filter_map(|(name, count)| {
        let row = rows.get(name)?;
        let ceiling = row.ceiling.map_or_else(|| "-".to_owned(), |ceiling| format!("{ceiling:.1}"));
        let state = if *count <= row.budget + SAME_COUNT { "ok" } else { "over" };
        Some((count / row.budget, format!("{name:<16}{count:>10.1}{:>8.1}{ceiling:>9}{:>7.1}{:>7.1}x  {state}", row.budget, row.c, count / row.c.max(1.0))))
    }).collect();
    lines.sort_by(|a, b| b.0.total_cmp(&a.0));
    let head = format!("{:<16}{:>10}{:>8}{:>9}{:>7}{:>8}", "op", "Ir/iter", "budget", "ceiling", "C", "vs C");
    std::iter::once(head).chain(lines.into_iter().map(|(_, line)| line)).collect::<Vec<_>>().join("\n")
}

/// Ratchets the ceilings to `measured`; returns the ops that grew past theirs.
fn ratchet(config: &Config, measured: &[(String, f64)], rows: &mut BTreeMap<String, Row>) -> Vec<String> {
    let mut grown = Vec::new();
    for (name, count) in measured {
        let Some(row) = rows.get_mut(name) else { grown.push(format!("{name}: no budget row in {BUDGETS}")); continue };
        let floor = (count * CEILING_STEPS_PER_INSTRUCTION).ceil() / CEILING_STEPS_PER_INSTRUCTION;
        match row.ceiling {
            Some(ceiling) if *count > ceiling + SAME_COUNT => grown.push(format!("{name}: {count:.1} Ir/iter, ceiling {ceiling}")),
            Some(ceiling) if *count + SAME_COUNT < ceiling => row.ceiling = Some(floor.max(row.budget.min(ceiling))),
            Some(_) => {}
            None if config.record => row.ceiling = Some(floor.max(row.budget)),
            None => grown.push(format!("{name}: {count:.1} Ir/iter, no ceiling recorded (--record)")),
        }
    }
    grown
}

pub fn run(config: &Config) -> Verdict {
    let path = config.root.join(BUDGETS);
    let before = fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut rows = parse(&before);
    let ops: Vec<String> = env::var(OPS_VARIABLE).unwrap_or_default().split(',').filter(|op| !op.is_empty()).map(str::to_owned).collect();
    let mode = env::var(MODE_VARIABLE).map_or_else(|_| INTERPRETER.to_owned(), |mode| format!("--{mode}"));
    let measured = measure(config, &mode, &ops)?;
    let report = table(&measured, &rows);
    let over = measured.iter().filter(|(name, count)| rows.get(name).is_some_and(|row| *count > row.budget + SAME_COUNT)).count();
    if mode != INTERPRETER {
        return Ok(format!("{mode}: {over} of {} ops over budget (ceilings hold for --no-jit only)\n{report}", measured.len()));
    }
    let grown = ratchet(config, &measured, &mut rows);
    let after = render(&header(&before), &rows);
    if after != before {
        fs::write(&path, &after).map_err(|error| format!("{}: {error}", path.display()))?;
    }
    if grown.is_empty() {
        Ok(format!("{} ops within their Ir ceilings; {over} over budget\n{report}", measured.len()))
    } else {
        Err(format!("{}\n\n{report}", grown.join("\n")))
    }
}
