//! Step: every language primitive has an instruction budget. Each op of
//! `quench-bench/primitives/ops.js` runs under the interpreter in one callgrind process, between
//! profiling marks (`process.__irMark`, `QUENCH_DEV=ir-mark`, `--dump-before=quench_ir_mark`), so
//! its instructions per loop iteration are exact: (Ir(2n) - Ir(n)) / n, with start-up, warm-up and
//! per-call costs cancelled. `crates/quench-tier-corpus/ir_budget.txt` holds, per op, the C
//! twin's count (`ops.c`), the budget (3x C plus one dispatch allowance) and the ceiling: the
//! measured count that may not grow. A drop lowers the ceiling; a count at or under its budget
//! makes the budget the ceiling. Ops over budget are the debt the report ranks.
//!
//! Each op's count is also split by function (callgrind's self cost per symbol, differenced the
//! same way), so a count names the handlers it is made of. A passing interpreter run records the
//! split in `ir_budget_handlers.txt`; an op that grows is reported with the handlers that grew
//! against that record, and the report ranks handlers by their instructions summed over all ops.
//!
//! `QUENCH_IR_BUDGET_MODE=tiers` measures every op under `--no-jit`, `--b-jit` and `--u-jit`
//! and reports the tier order the design asks for, no-jit >= b-jit >= u-jit, with u-jit within
//! 5x of the C twin, naming each op that breaks it (report only).
//!
//! Alone: `quench-gate --only ir-budget`. `QUENCH_IR_BUDGET_OPS=a,b` measures only those ops,
//! `QUENCH_IR_BUDGET_MODE=b-jit` (or `u-jit`) measures a JIT tier against the same budgets
//! without touching the ceilings, which hold for the interpreter.

use std::{collections::BTreeMap, env, fs, path::Path};

use crate::{build::NODE, config::Config, process, step::Verdict, tiers::INTERPRETER};

const BUDGETS: &str = "crates/quench-tier-corpus/ir_budget.txt";
const HANDLERS: &str = "crates/quench-tier-corpus/ir_budget_handlers.txt";
/// A handler share under this many instructions per iteration is left out of the split.
const SHARE_FLOOR: f64 = 0.5;
/// Handlers the report's ranking lists.
const RANKED: usize = 30;
const RUNNER: &str = "quench-bench/primitives/ir_budget.cjs";
/// Iterations of the shorter measured run; the longer runs twice as many.
const ITERATIONS: u64 = 20_000;
const MARK: &str = "quench_ir_mark";
/// Marks per op: around op(n), then around op(2n), each run after a collection (`gc()`).
const MARKS_PER_OP: usize = 4;
/// Whether each measured run starts from a collected heap (`--expose-gc`, the runner's
/// `collect`). Off while `gc()` corrupts a closure's captured value on v2-merged (a read of
/// `k` in `call_closure` yields a Symbol after field_store, call_direct, call_closure each
/// ran around `gc()`; repro in Linear TOD-1807); the unmeasured run before each mark keeps
/// the heap state comparable meanwhile.
const COLLECT: bool = false;
const OPS_VARIABLE: &str = "QUENCH_IR_BUDGET_OPS";
const MODE_VARIABLE: &str = "QUENCH_IR_BUDGET_MODE";
const LINE_PREFIX: &str = "IRBUDGET ";
/// A count within this many instructions per iteration of its ceiling is the same count...
const SAME_COUNT: f64 = 0.5;
/// ...or within this fraction of it: ops that run library code (`parseInt`, string builders)
/// drift by an instruction or so with code layout, which is not a regression.
const SAME_FRACTION: f64 = 0.001;

/// The count difference that still counts as the same at `ceiling`.
fn tolerance(ceiling: f64) -> f64 {
    SAME_COUNT.max(ceiling * SAME_FRACTION)
}

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
        Some(((*fields.first()?).to_owned(), Row { c: number(1)?, budget: number(2)?, ceiling: number(3) }))
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

/// One callgrind dump: its total, and the self instructions of each function in it.
#[derive(Default)]
struct Dump {
    total: u64,
    functions: BTreeMap<String, u64>,
}

/// A function's name as the split shows it: the item without its module path, with the host
/// type argument and callgrind's recursion suffix dropped; code without a symbol (JIT output) is
/// one row.
fn short_name(full: &str) -> String {
    if full.starts_with("0x") { return "(jit code)".to_owned() }
    let full = full.split('\'').next().unwrap_or(full);
    let full = full.replace("::<", "<").replace("<quench_node::host::node_host::NodeHost, ", "<").replace("<quench_node::host::node_host::NodeHost>", "");
    // The path's segments at angle depth 0: the item is the last, its impl type (`<Vm>::f`) or its
    // function (`f::{{closure}}`) the one before.
    let (mut segments, mut depth, mut start) = (Vec::new(), 0usize, 0usize);
    let bytes = full.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'<' => depth += 1,
            b'>' => depth = depth.saturating_sub(1),
            b':' if depth == 0 && bytes.get(index + 1) == Some(&b':') => {
                segments.push(&full[start..index]);
                start = index + 2;
                index += 1;
            }
            _ => {}
        }
        index += 1;
    }
    segments.push(&full[start..]);
    let last = segments.pop().unwrap_or_default();
    match segments.pop() {
        Some(owner) if last.starts_with("{{") => format!("{}::{last}", owner),
        Some(owner) if owner.starts_with('<') => {
            let inner = owner.trim_start_matches('<').trim_end_matches('>');
            let inner = inner.split(" as ").next().unwrap_or(inner);
            let inner = inner.split('<').next().unwrap_or(inner);
            format!("{}::{last}", inner.rsplit("::").next().unwrap_or(inner))
        }
        _ => last.to_owned(),
    }
}

/// Every callgrind dump in `directory`, in dump order. Callgrind compresses a name to `(id)`
/// after its first use, across the dumps of one run, so the dumps are read in order.
fn dumps(directory: &Path) -> Result<Vec<Dump>, String> {
    let mut numbered = Vec::new();
    for entry in fs::read_dir(directory).map_err(|error| format!("{}: {error}", directory.display()))? {
        let path = entry.map_err(|error| error.to_string())?.path();
        let Some(index) = path.extension().and_then(|extension| extension.to_str()?.parse::<u64>().ok()) else { continue };
        numbered.push((index, path));
    }
    numbered.sort_unstable();
    let mut names: BTreeMap<String, String> = BTreeMap::new();
    let mut result = Vec::new();
    for (_, path) in numbered {
        let text = fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        let mut dump = Dump::default();
        let mut current: Option<String> = None;
        let mut after_call = false;
        for line in text.lines() {
            let named = |rest: &str, names: &mut BTreeMap<String, String>| -> Option<String> {
                let (id, name) = match rest.strip_prefix('(').and_then(|rest| rest.split_once(')')) {
                    Some((id, name)) => (id.to_owned(), name.trim()),
                    None => (rest.to_owned(), rest),
                };
                if !name.is_empty() { names.insert(id.clone(), short_name(name)); }
                names.get(&id).cloned()
            };
            if let Some(rest) = line.strip_prefix("fn=") {
                current = named(rest, &mut names);
            } else if let Some(rest) = line.strip_prefix("cfn=") {
                named(rest, &mut names);
            } else if line.starts_with("calls=") {
                after_call = true;
            } else if let Some(total) = line.strip_prefix("summary: ").or_else(|| line.strip_prefix("totals: ")) {
                dump.total = total.split_whitespace().next().and_then(|total| total.parse().ok()).unwrap_or(0);
            } else if line.starts_with(|first: char| first.is_ascii_digit() || matches!(first, '+' | '-' | '*')) {
                // A cost line: position(s), then the instruction count. The line after `calls=`
                // is the call's inclusive cost, not this function's own.
                if std::mem::take(&mut after_call) { continue }
                let (Some(function), Some(count)) = (&current, line.split_whitespace().last().and_then(|count| count.parse::<u64>().ok())) else { continue };
                *dump.functions.entry(function.clone()).or_default() += count;
            }
        }
        result.push(dump);
    }
    Ok(result)
}

/// One op's count per iteration split by function, largest share first.
type Split = Vec<(String, f64)>;

fn split(shorter: &Dump, longer: &Dump) -> Split {
    let mut shares: Split = longer.functions.iter().map(|(name, count)| {
        (name.clone(), (*count as f64 - shorter.functions.get(name).copied().unwrap_or(0) as f64) / ITERATIONS as f64)
    }).filter(|(_, share)| share.abs() >= SHARE_FLOOR).collect();
    shares.sort_by(|a, b| b.1.total_cmp(&a.1));
    shares
}

/// Instructions per iteration of each op the runner measured, in run order.
fn measure(config: &Config, mode: &str, ops: &[String]) -> Result<Vec<(String, f64, Split)>, String> {
    let host = config.binary(NODE)?;
    let scratch = config.target.join("ir-budget");
    let _ = fs::remove_dir_all(&scratch);
    fs::create_dir_all(&scratch).map_err(|error| format!("{}: {error}", scratch.display()))?;
    let mut command = config.command("valgrind");
    command.arg("--tool=callgrind").arg(format!("--dump-before={MARK}")).arg("--dump-instr=no")
        .arg(format!("--callgrind-out-file={}", scratch.join("cg.out").display()))
        // JIT tiers patch the code they emitted (call links, stubs): valgrind must see each write.
        .args((mode != INTERPRETER).then_some("--smc-check=all"))
        .arg(host).arg(mode).args(COLLECT.then_some("--expose-gc")).arg(RUNNER).arg(ITERATIONS.to_string()).args(ops)
        .env("QUENCH_DEV", if mode == INTERPRETER { "ir-mark" } else { "ir-mark,sync-compile" })
        .env("IRBUDGET_WARMUP", if mode == INTERPRETER { "3" } else { "12" })
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
    let measured = |index: usize| {
        // Dump k holds the counts from mark k-1 to mark k: per op, op(n) then op(2n).
        let (shorter, longer) = (&totals[index * MARKS_PER_OP + 1], &totals[index * MARKS_PER_OP + 3]);
        ((longer.total as f64 - shorter.total as f64) / ITERATIONS as f64, split(shorter, longer))
    };
    Ok(names.into_iter().enumerate().map(|(index, name)| { let (count, split) = measured(index); (name, count, split) }).collect())
}

/// One op as measured: its count per iteration and that count split by function.
type Measured = (String, f64, Split);

/// The report: every measured op, worst budget ratio first, with its three largest handlers.
fn table(measured: &[Measured], rows: &BTreeMap<String, Row>) -> String {
    let mut lines: Vec<(f64, String)> = measured.iter().filter_map(|(name, count, split)| {
        let row = rows.get(name)?;
        let ceiling = row.ceiling.map_or_else(|| "-".to_owned(), |ceiling| format!("{ceiling:.1}"));
        let state = if *count <= row.budget + SAME_COUNT { "ok  " } else { "over" };
        let top: Vec<String> = split.iter().take(3).map(|(handler, share)| format!("{share:.0} {handler}")).collect();
        Some((count / row.budget, format!("{name:<16}{count:>10.1}{:>8.1}{ceiling:>9}{:>7.1}{:>7.1}x  {state}  {}", row.budget, row.c, count / row.c.max(1.0), top.join(", "))))
    }).collect();
    lines.sort_by(|a, b| b.0.total_cmp(&a.0));
    let head = format!("{:<16}{:>10}{:>8}{:>9}{:>7}{:>8}        {}", "op", "Ir/iter", "budget", "ceiling", "C", "vs C", "largest handlers");
    std::iter::once(head).chain(lines.into_iter().map(|(_, line)| line)).collect::<Vec<_>>().join("\n")
}

/// The handlers ranked by their instructions per iteration summed over every measured op: what
/// one cut in one handler buys across the primitives.
fn ranking(measured: &[Measured]) -> String {
    let mut totals: BTreeMap<&str, (f64, usize)> = BTreeMap::new();
    for (_, _, split) in measured {
        for (handler, share) in split {
            let entry = totals.entry(handler).or_default();
            entry.0 += share;
            entry.1 += 1;
        }
    }
    let mut ranked: Vec<_> = totals.into_iter().collect();
    ranked.sort_by(|a, b| b.1.0.total_cmp(&a.1.0));
    let lines = ranked.iter().take(RANKED).map(|(handler, (sum, ops))| format!("{sum:>8.1}{ops:>5}  {handler}"));
    std::iter::once(format!("{:>8}{:>5}  handler (Ir/iter summed over ops, ops it runs in)", "Ir", "ops")).chain(lines).collect::<Vec<_>>().join("\n")
}

/// The recorded split: op, handler, share, one per line.
fn parse_splits(text: &str) -> BTreeMap<String, BTreeMap<String, f64>> {
    let mut splits: BTreeMap<String, BTreeMap<String, f64>> = BTreeMap::new();
    for line in text.lines().filter(|line| !line.starts_with('#')) {
        let mut fields = line.splitn(3, ' ');
        let (Some(op), Some(share), Some(handler)) = (fields.next(), fields.next().and_then(|share| share.parse().ok()), fields.next()) else { continue };
        splits.entry(op.to_owned()).or_default().insert(handler.to_owned(), share);
    }
    splits
}

fn render_splits(splits: &BTreeMap<String, BTreeMap<String, f64>>) -> String {
    let mut text = String::from("# Each op's Ir/iter split by handler (callgrind self cost), recorded by a passing `quench-gate --only ir-budget`.\n# op share handler; the gate names the handlers that grew against these. Not edited by hand.\n");
    for (op, split) in splits {
        let mut shares: Vec<_> = split.iter().collect();
        shares.sort_by(|a, b| b.1.total_cmp(a.1).then_with(|| a.0.cmp(b.0)));
        for (handler, share) in shares { text.push_str(&format!("{op} {share:.1} {handler}\n")); }
    }
    text
}

/// The handlers of `split` that grew against `recorded`, as `handler +delta`.
fn grown_handlers(split: &Split, recorded: Option<&BTreeMap<String, f64>>) -> String {
    let Some(recorded) = recorded else { return "no recorded split".to_owned() };
    let mut deltas: Vec<(String, f64)> = split.iter().map(|(handler, share)| (handler.clone(), share - recorded.get(handler).copied().unwrap_or(0.0))).collect();
    deltas.extend(recorded.iter().filter(|(handler, _)| !split.iter().any(|(name, _)| name == *handler)).map(|(handler, share)| (handler.clone(), -share)));
    deltas.retain(|(_, delta)| delta.abs() >= SHARE_FLOOR);
    deltas.sort_by(|a, b| b.1.total_cmp(&a.1));
    if deltas.is_empty() { return "no handler moved by a whole instruction".to_owned() }
    deltas.iter().map(|(handler, delta)| format!("{delta:+.1} {handler}")).collect::<Vec<_>>().join(", ")
}

/// Ratchets the ceilings to `measured`; returns the ops that grew past theirs.
fn ratchet(config: &Config, measured: &[Measured], rows: &mut BTreeMap<String, Row>, recorded: &BTreeMap<String, BTreeMap<String, f64>>, every_op: bool) -> Vec<String> {
    let mut grown = Vec::new();
    for (name, count, split) in measured {
        let Some(row) = rows.get_mut(name) else { grown.push(format!("{name}: no budget row in {BUDGETS}")); continue };
        let floor = (count * 10.0).ceil() / 10.0;
        match row.ceiling {
            Some(ceiling) if *count > ceiling + tolerance(ceiling) => grown.push(format!("{name}: {count:.1} Ir/iter, ceiling {ceiling}; {}", grown_handlers(split, recorded.get(name)))),
            // Only a run of every op lowers a ceiling: an op measured alone can run other handler
            // forms (a call site's binding is resolved differently) and count fewer.
            Some(ceiling) if *count + SAME_COUNT < ceiling && every_op => row.ceiling = Some(floor.max(row.budget.min(ceiling))),
            Some(_) => {}
            None if config.record => row.ceiling = Some(floor.max(row.budget)),
            None => grown.push(format!("{name}: {count:.1} Ir/iter, no ceiling recorded (--record)")),
        }
    }
    grown
}

/// u-jit's allowance over the C twin.
const UJIT_OVER_C: f64 = 5.0;

/// The tier report: each op's count per tier, worst u-jit-to-C ratio first, with the order it
/// breaks.
fn tiers(config: &Config, ops: &[String], rows: &BTreeMap<String, Row>) -> Verdict {
    let mut counts: BTreeMap<String, [f64; 3]> = BTreeMap::new();
    for (index, mode) in [INTERPRETER, "--b-jit", "--u-jit"].into_iter().enumerate() {
        for (name, count, _) in measure(config, mode, ops)? {
            counts.entry(name).or_insert([f64::NAN; 3])[index] = count;
        }
    }
    let mut lines: Vec<(f64, String)> = Vec::new();
    let (mut broken, mut over_c) = (0, 0);
    for (name, [interpreter, baseline, optimized]) in &counts {
        let c = rows.get(name).map_or(f64::NAN, |row| row.c);
        let mut faults = Vec::new();
        if *baseline > interpreter + tolerance(*interpreter) { faults.push("b-jit>no-jit") }
        if *optimized > baseline + tolerance(*baseline) { faults.push("u-jit>b-jit") }
        if !faults.is_empty() { broken += 1 }
        if *optimized > UJIT_OVER_C * c { faults.push("u-jit>5C"); over_c += 1 }
        lines.push((optimized / c.max(1.0), format!("{name:<16}{interpreter:>10.1}{baseline:>10.1}{optimized:>10.1}{c:>7.1}{:>7.1}x  {}", optimized / c.max(1.0), faults.join(" "))));
    }
    lines.sort_by(|a, b| b.0.total_cmp(&a.0));
    let head = format!("{:<16}{:>10}{:>10}{:>10}{:>7}{:>8}  {}", "op", "no-jit", "b-jit", "u-jit", "C", "u/C", "breaks");
    let table = std::iter::once(head).chain(lines.into_iter().map(|(_, line)| line)).collect::<Vec<_>>().join("\n");
    Ok(format!("tiers: {broken} of {} ops out of order, {over_c} with u-jit past 5x C (report only)\n{table}", counts.len()))
}

pub fn run(config: &Config) -> Verdict {
    let path = config.root.join(BUDGETS);
    let before = fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut rows = parse(&before);
    let handlers_path = config.root.join(HANDLERS);
    let handlers_before = fs::read_to_string(&handlers_path).unwrap_or_default();
    let mut recorded = parse_splits(&handlers_before);
    let ops: Vec<String> = env::var(OPS_VARIABLE).unwrap_or_default().split(',').filter(|op| !op.is_empty()).map(str::to_owned).collect();
    if env::var(MODE_VARIABLE).as_deref() == Ok("tiers") {
        return tiers(config, &ops, &rows);
    }
    let mode = env::var(MODE_VARIABLE).map_or_else(|_| INTERPRETER.to_owned(), |mode| format!("--{mode}"));
    let measured = measure(config, &mode, &ops)?;
    let report = format!("{}\n\n{}", table(&measured, &rows), ranking(&measured));
    let over = measured.iter().filter(|(name, count, _)| rows.get(name).is_some_and(|row| *count > row.budget + SAME_COUNT)).count();
    if mode != INTERPRETER {
        return Ok(format!("{mode}: {over} of {} ops over budget (ceilings hold for --no-jit only)\n{report}", measured.len()));
    }
    let grown = ratchet(config, &measured, &mut rows, &recorded, ops.is_empty());
    let after = render(&header(&before), &rows);
    if after != before {
        fs::write(&path, &after).map_err(|error| format!("{}: {error}", path.display()))?;
    }
    if !grown.is_empty() {
        return Err(format!("{}\n\n{report}", grown.join("\n")));
    }
    if !ops.is_empty() {
        return Ok(format!("{} ops within their Ir ceilings; {over} over budget (a subset: ceilings and the split are recorded by a run of every op)\n{report}", measured.len()));
    }
    for (name, _, split) in &measured {
        recorded.insert(name.clone(), split.iter().cloned().collect());
    }
    let handlers_after = render_splits(&recorded);
    if handlers_after != handlers_before {
        fs::write(&handlers_path, &handlers_after).map_err(|error| format!("{}: {error}", handlers_path.display()))?;
    }
    Ok(format!("{} ops within their Ir ceilings; {over} over budget\n{report}", measured.len()))
}
