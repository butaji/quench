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
//! The tier ladder is gated per op, by design (Futamura, Ershov): b-jit is the interpreter
//! partially evaluated on the bytecode (dispatch and decode folded, the handlers' residue left), so
//! it runs every op in at most the interpreter's instructions; u-jit is b-jit further specialized
//! on proven facts, so it runs in at most b-jit's, and its target is the budget, 3x C + 6. Every op
//! runs under `--no-jit`, `--b-jit` and `--u-jit` (JIT tiers with `--smc-check=all`), and the step
//! fails when
//! 1. a tier's count grows past its ceiling (each tier has one; a u-jit count at or under the
//!    budget makes the budget its ceiling);
//! 2. the ladder inverts (b-jit above no-jit, u-jit above b-jit, measured in the same run on the same
//!    host, so container drift cancels) for an op `tier_inversions.txt` does not list. That file
//!    is the inversion debt: shrink-only, an entry leaves it once its op is in order.
//!
//! Alone: `quench-gate --only ir-budget`. `QUENCH_IR_BUDGET_OPS=a,b` measures only those ops,
//! `QUENCH_IR_BUDGET_MODE=no-jit` (or `b-jit`, `u-jit`) measures one tier: its ceilings are
//! checked, the ladder is not judged.

use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::Path,
};

use crate::{build::NODE, config::Config, process, step::Verdict, tiers::INTERPRETER};

const BUDGETS: &str = "crates/quench-tier-corpus/ir_budget.txt";
const INVERSIONS: &str = "crates/quench-tier-corpus/tier_inversions.txt";
const HANDLERS: &str = "crates/quench-tier-corpus/ir_budget_handlers.txt";
/// A handler share under this many instructions per iteration is left out of the split.
const SHARE_FLOOR: f64 = 0.5;
/// Handlers the report's ranking lists.
const RANKED: usize = 30;
const RUNNER: &str = "quench-bench/primitives/ir_budget.cjs";
/// Iterations of the shorter measured run; the longer runs twice as many.
const ITERATIONS: u64 = 20_000;
const MARK: &str = "quench_ir_mark";
/// The longest one tier's callgrind run of every op may take (a JIT tier under `--smc-check=all` takes about half of it).
const MEASURE_SECONDS: u64 = 3600;
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

/// The tiers of the ladder, slowest first, as their runner flags: each must run every op in at
/// most the instructions of the one before it.
const TIERS: [&str; 3] = [INTERPRETER, "--b-jit", "--u-jit"];
/// The tier the budget (3x C + 6) is the target of: the last.
const ULTRA: usize = TIERS.len() - 1;
/// The column of the first tier's ceiling in a budget row.
const FIRST_CEILING_COLUMN: usize = 3;
const UNRECORDED: &str = "-";

/// A tier's name in reports and in `tier_inversions.txt`: its flag without `--`.
fn tier_name(tier: usize) -> &'static str {
    TIERS[tier].trim_start_matches('-')
}

/// One op's row in the budget file: the C twin's count, the budget and a ceiling per tier.
#[derive(Clone, Copy)]
struct Row {
    c: f64,
    budget: f64,
    ceilings: [Option<f64>; TIERS.len()],
}

fn parse(text: &str) -> BTreeMap<String, Row> {
    let rows = text.lines().filter(|line| !line.starts_with('#')).map(|line| line.split_whitespace().collect::<Vec<_>>());
    rows.filter_map(|fields| {
        let number = |index: usize| fields.get(index).and_then(|field| field.parse::<f64>().ok());
        let ceilings = std::array::from_fn(|tier| number(FIRST_CEILING_COLUMN + tier));
        Some(((*fields.first()?).to_owned(), Row { c: number(1)?, budget: number(2)?, ceilings }))
    })
    .collect()
}

fn header(text: &str) -> String {
    text.lines().take_while(|line| line.starts_with('#')).map(|line| format!("{line}\n")).collect()
}

fn render(header: &str, rows: &BTreeMap<String, Row>) -> String {
    let body: String = rows.iter().map(|(name, row)| {
        let ceilings: Vec<String> = row.ceilings.iter().map(|ceiling| ceiling.map_or_else(|| UNRECORDED.to_owned(), |value| value.to_string())).collect();
        format!("{name} {} {} {}\n", row.c, row.budget, ceilings.join(" "))
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
    let scratch = config.target.join("ir-budget").join(mode.trim_start_matches('-'));
    let _ = fs::remove_dir_all(&scratch);
    fs::create_dir_all(&scratch).map_err(|error| format!("{}: {error}", scratch.display()))?;
    // A tier that hangs on an op (a miscompiled loop) fails the step rather than holding the gate.
    let mut command = config.command("timeout");
    command.arg(MEASURE_SECONDS.to_string()).arg("valgrind").arg("--tool=callgrind").arg(format!("--dump-before={MARK}")).arg("--dump-instr=no")
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
        return Err(format!("{RUNNER} failed or timed out under callgrind in {mode}\n{}", run.excerpt()));
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
fn table(measured: &[Measured], rows: &BTreeMap<String, Row>, tier: usize) -> String {
    let mut lines: Vec<(f64, String)> = measured.iter().filter_map(|(name, count, split)| {
        let row = rows.get(name)?;
        let ceiling = row.ceilings[tier].map_or_else(|| UNRECORDED.to_owned(), |ceiling| format!("{ceiling:.1}"));
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

/// Name parts of the lane handlers that run only where a fast form missed: in steady state each
/// is waste (a site that never settles, a cache that never fills), whose budget is zero.
const SLOW_PATHS: [&str; 6] = ["_miss", "_refresh", "_slow", "deopt", "bail", "_general"];

/// The slow-path handlers (`SLOW_PATHS`) each op runs in steady state, largest first.
fn waste(measured: &[Measured]) -> String {
    let mut rows: Vec<(f64, String)> = measured.iter().flat_map(|(op, _, split)| {
        split.iter().filter(|(handler, share)| *share >= 1.0 && handler.starts_with("lane_") && SLOW_PATHS.iter().any(|part| handler.contains(part)))
            .map(move |(handler, share)| (*share, format!("{share:>8.1}  {op:<16}{handler}")))
    }).collect();
    rows.sort_by(|a, b| b.0.total_cmp(&a.0));
    let head = format!("{:>8}  {:<16}{}", "Ir", "op", "slow path run in steady state (zero budget)");
    std::iter::once(head).chain(rows.into_iter().map(|(_, row)| row)).collect::<Vec<_>>().join("\n")
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
fn ratchet(config: &Config, tier: usize, measured: &[Measured], rows: &mut BTreeMap<String, Row>, recorded: &BTreeMap<String, BTreeMap<String, f64>>, every_op: bool) -> Vec<String> {
    let mut grown = Vec::new();
    for (name, count, split) in measured {
        let Some(row) = rows.get_mut(name) else { grown.push(format!("{name}: no budget row in {BUDGETS}")); continue };
        let floor = (count * 10.0).ceil() / 10.0;
        // The budget is the floor of the interpreter's ceiling (its historical target) and u-jit's
        // (the design's); b-jit's ceiling follows its count down.
        let lowest = if tier == 0 || tier == ULTRA { row.budget } else { 0.0 };
        let (budget, tier_label) = (row.budget, tier_name(tier));
        let ceiling = &mut row.ceilings[tier];
        match *ceiling {
            Some(value) if *count > value + tolerance(value) => grown.push(format!("{name}: {tier_label} {count:.1} Ir/iter, ceiling {value}; {}", if tier == 0 { grown_handlers(split, recorded.get(name)) } else { format!("budget {budget}") })),
            // Only a run of every op lowers a ceiling: an op measured alone can run other handler
            // forms (a call site's binding is resolved differently) and count fewer.
            Some(value) if *count + SAME_COUNT < value && every_op => *ceiling = Some(floor.max(lowest.min(value))),
            Some(_) => {}
            None if config.record => *ceiling = Some(floor.max(lowest)),
            None => grown.push(format!("{name}: {tier_label} {count:.1} Ir/iter, no ceiling recorded (--record)")),
        }
    }
    grown
}

/// The inversion debt: `op tier` pairs whose tier may stay above the tier before it.
fn inversions(text: &str) -> BTreeSet<(String, usize)> {
    let entries = text.lines().filter(|line| !line.starts_with('#')).filter_map(|line| line.split_once(' '));
    entries.filter_map(|(op, tier)| Some((op.to_owned(), (1..TIERS.len()).find(|index| tier_name(*index) == tier.trim())?))).collect()
}

fn render_inversions(header: &str, debt: &BTreeSet<(String, usize)>) -> String {
    let body: String = debt.iter().map(|(op, tier)| format!("{op} {}\n", tier_name(*tier))).collect();
    format!("{header}{body}")
}

/// The tiers of `counts` (one per tier, ladder order) that run more instructions than the tier before.
fn ladder_breaks(counts: &[f64; TIERS.len()]) -> Vec<usize> {
    (1..TIERS.len()).filter(|tier| counts[*tier] > counts[tier - 1] + tolerance(counts[tier - 1])).collect()
}

/// Judges every op's ladder against the inversion debt, which shrinks to what is still inverted
/// (only on a run of every op); returns the inversions the debt does not hold.
fn judge_ladder(config: &Config, counts: &BTreeMap<String, [f64; TIERS.len()]>, debt: &mut BTreeSet<(String, usize)>, every_op: bool) -> Vec<String> {
    let mut new = Vec::new();
    for (name, by_tier) in counts {
        let breaks = ladder_breaks(by_tier);
        if every_op { debt.retain(|(op, tier)| op != name || breaks.contains(tier)) }
        for tier in breaks {
            if debt.contains(&(name.clone(), tier)) { continue }
            if config.record { debt.insert((name.clone(), tier)); continue }
            new.push(format!("{name}: {} {:.1} Ir/iter > {} {:.1}: the ladder inverts (by design {} <= {})",
                tier_name(tier), by_tier[tier], tier_name(tier - 1), by_tier[tier - 1], tier_name(tier), tier_name(tier - 1)));
        }
    }
    new
}

/// The ladder report: each op's count per tier, worst u-jit-to-budget ratio first.
fn ladder_table(counts: &BTreeMap<String, [f64; TIERS.len()]>, rows: &BTreeMap<String, Row>) -> String {
    let mut lines: Vec<(f64, String)> = counts.iter().filter_map(|(name, by_tier)| {
        let row = rows.get(name)?;
        let cells: String = by_tier.iter().map(|count| format!("{count:>9.1}")).collect();
        let ultra = by_tier[ULTRA];
        let breaks: Vec<String> = ladder_breaks(by_tier).into_iter().map(|tier| format!("{}>{}", tier_name(tier), tier_name(tier - 1))).collect();
        let state = if ultra <= row.budget + SAME_COUNT { "ok  " } else { "over" };
        Some((ultra / row.budget, format!("{name:<16}{cells}{:>8.1}{:>7.1}{:>7.1}x  {state}  {}", row.budget, row.c, ultra / row.c.max(1.0), breaks.join(" "))))
    }).collect();
    lines.sort_by(|a, b| b.0.total_cmp(&a.0));
    let heads: String = (0..TIERS.len()).map(|tier| format!("{:>9}", tier_name(tier))).collect();
    let head = format!("{:<16}{heads}{:>8}{:>7}{:>8}  {}", "op", "budget", "C", "u/C", "u-jit vs 3xC+6, inversions");
    std::iter::once(head).chain(lines.into_iter().map(|(_, line)| line)).collect::<Vec<_>>().join("\n")
}

fn write_if_changed(path: &Path, before: &str, after: &str) -> Result<(), String> {
    if after == before { return Ok(()) }
    fs::write(path, after).map_err(|error| format!("{}: {error}", path.display()))
}

pub fn run(config: &Config) -> Verdict {
    let path = config.root.join(BUDGETS);
    let before = fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut rows = parse(&before);
    let handlers_path = config.root.join(HANDLERS);
    let handlers_before = fs::read_to_string(&handlers_path).unwrap_or_default();
    let mut recorded = parse_splits(&handlers_before);
    let inversions_path = config.root.join(INVERSIONS);
    let debt_before = fs::read_to_string(&inversions_path).unwrap_or_default();
    let mut debt = inversions(&debt_before);
    let ops: Vec<String> = env::var(OPS_VARIABLE).unwrap_or_default().split(',').filter(|op| !op.is_empty()).map(str::to_owned).collect();
    let every_op = ops.is_empty();
    let selected: Vec<usize> = match env::var(MODE_VARIABLE) {
        Ok(mode) => vec![(0..TIERS.len()).find(|tier| tier_name(*tier) == mode).ok_or_else(|| format!("{MODE_VARIABLE}={mode}: expected no-jit, b-jit or u-jit"))?],
        Err(_) => (0..TIERS.len()).collect(),
    };
    let mut failures = Vec::new();
    let mut reports = Vec::new();
    let mut counts: BTreeMap<String, [f64; TIERS.len()]> = BTreeMap::new();
    let mut interpreter = None;
    // The tiers run at once, one callgrind process each: under `--smc-check=all` a JIT tier's run
    // is the step's long pole.
    let runs: Vec<Result<Vec<Measured>, String>> = std::thread::scope(|scope| {
        let handles: Vec<_> = selected.iter().map(|&tier| { let ops = &ops; scope.spawn(move || measure(config, TIERS[tier], ops)) }).collect();
        handles.into_iter().map(|handle| handle.join().unwrap_or_else(|_| Err("a measuring thread panicked".to_owned()))).collect()
    });
    for (&tier, run) in selected.iter().zip(runs) {
        let measured = run?;
        failures.extend(ratchet(config, tier, &measured, &mut rows, &recorded, every_op));
        for (name, count, _) in &measured { counts.entry(name.clone()).or_insert([f64::NAN; TIERS.len()])[tier] = *count }
        if tier == 0 {
            reports.push(format!("{}\n\n{}\n\n{}", table(&measured, &rows, 0), ranking(&measured), waste(&measured)));
            interpreter = Some(measured);
        } else if selected.len() == 1 {
            reports.push(format!("{}\n\n{}", table(&measured, &rows, tier), ranking(&measured)));
        }
    }
    if selected.len() == TIERS.len() {
        failures.extend(judge_ladder(config, &counts, &mut debt, every_op));
        reports.insert(0, ladder_table(&counts, &rows));
    }
    let ultra_over = counts.iter().filter(|(name, by_tier)| rows.get(*name).is_some_and(|row| by_tier[ULTRA] > row.budget + SAME_COUNT)).count();
    let summary = format!("{} ops; {} ladder inversions in debt; {ultra_over} u-jit counts over 3xC+6", counts.len(), debt.len());
    let report = reports.join("\n\n");
    // A failing run records nothing (a ceiling it lowered would hide the trade it made), unless it
    // was asked to record: then the entries it had none for are written beside the failures.
    if !failures.is_empty() && !config.record {
        return Err(format!("{}\n\n{summary}\n{report}", failures.join("\n")));
    }
    write_if_changed(&path, &before, &render(&header(&before), &rows))?;
    write_if_changed(&inversions_path, &debt_before, &render_inversions(&header(&debt_before), &debt))?;
    if !failures.is_empty() {
        return Err(format!("{}\n\n{summary} (recorded)\n{report}", failures.join("\n")));
    }
    if !every_op {
        return Ok(format!("{summary} (a subset: ceilings, the debt and the split are lowered by a run of every op)\n{report}"));
    }
    if let Some(measured) = interpreter {
        for (name, _, split) in &measured {
            recorded.insert(name.clone(), split.iter().cloned().collect());
        }
        write_if_changed(&handlers_path, &handlers_before, &render_splits(&recorded))?;
    }
    Ok(format!("{summary}\n{report}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ladder_breaks_where_a_tier_runs_more_than_the_one_before() {
        assert!(ladder_breaks(&[100.0, 50.0, 20.0]).is_empty());
        assert_eq!(ladder_breaks(&[100.0, 120.0, 20.0]), vec![1]);
        assert_eq!(ladder_breaks(&[100.0, 50.0, 60.0]), vec![2]);
        assert!(ladder_breaks(&[100.0, 100.4, 100.4]).is_empty());
    }

    #[test]
    fn rows_round_trip_with_unrecorded_tiers() {
        let text = "# h\nadd 4 18 46.3 - 20\nold 3 15 31.9\n";
        let rows = parse(text);
        assert_eq!(rows["add"].ceilings, [Some(46.3), None, Some(20.0)]);
        assert_eq!(render(&header(text), &rows), "# h\nadd 4 18 46.3 - 20\nold 3 15 31.9 - -\n");
    }

    #[test]
    fn inversion_debt_only_shrinks_and_new_inversions_fail() {
        let config = Config::parse(Vec::new()).unwrap();
        let mut debt = inversions("call_direct b-jit\nfield_read u-jit\n");
        let counts = BTreeMap::from([
            ("call_direct".to_owned(), [222.0, 2557.0, 7.0]),
            ("field_read".to_owned(), [70.0, 26.8, 6.8]),
            ("bitwise".to_owned(), [76.0, 90.0, 20.0]),
        ]);
        let new = judge_ladder(&config, &counts, &mut debt, true);
        assert_eq!(debt, BTreeSet::from([("call_direct".to_owned(), 1)]));
        assert_eq!(new.len(), 1);
        assert!(new[0].starts_with("bitwise: b-jit"));
    }
}
