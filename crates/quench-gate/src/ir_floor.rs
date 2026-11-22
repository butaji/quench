//! The C floor of the ir-budget step: C-shaped code runs as C. The interpreter may spend on an
//! op only its C twin's instructions plus a fixed dispatch and decode allowance per record it
//! dispatches (`DISPATCH_ALLOWANCE`); every instruction above that floor is waste, and the report
//! names the layer that owns it. The JIT tiers' target is the op's budget in `ir_budget.txt`
//! (C parity: max(C + 1, 1.25x C), the same budget the interpreter rows are held to); their ceilings ratchet toward it.
//!
//! Enforced by design, in `crates/quench-tier-corpus/ir_floor.txt`, one row per op:
//! - `records`: the records the interpreter dispatches per iteration (callgrind's entries into lane
//!   handlers). It only shrinks: an op that dispatches more records than recorded fails, so the
//!   allowance can never grow by splitting work into more records.
//! - `b-jit` and `default` (the shipped mode): each mode's ceiling, ratcheted like the interpreter's (shrink-only, a
//!   count at or under the budget makes the budget the ceiling); a count past it fails.

use std::{collections::BTreeMap, fs};

use crate::config::Config;

pub const FLOORS: &str = "crates/quench-tier-corpus/ir_floor.txt";
/// Instructions per dispatched record the interpreter may add to C: the next record's handler
/// load and jump (2) and its operands' decode and register-file reads and write (4).
pub const DISPATCH_ALLOWANCE: f64 = 6.0;
/// A records count within this of its recorded value is the same count.
const SAME_RECORDS: f64 = 0.5;
const SAME_COUNT: f64 = 0.5;
const SAME_FRACTION: f64 = 0.001;

fn tolerance(ceiling: f64) -> f64 {
    SAME_COUNT.max(ceiling * SAME_FRACTION)
}

/// One op's row in the floor file.
#[derive(Clone, Copy, Default)]
pub struct Floor {
    pub records: Option<f64>,
    pub jit: [Option<f64>; 2],
}

pub fn parse(text: &str) -> BTreeMap<String, Floor> {
    let rows = text.lines().filter(|line| !line.starts_with('#')).map(|line| line.split_whitespace().collect::<Vec<_>>());
    rows.filter_map(|fields| {
        let number = |index: usize| fields.get(index).and_then(|field| field.parse::<f64>().ok());
        Some(((*fields.first()?).to_owned(), Floor { records: number(1), jit: [number(2), number(3)] }))
    })
    .collect()
}

const HEADER: &str = "# C floor (quench-gate step `ir-budget`, `crates/quench-gate/src/ir_floor.rs`): C-shaped code runs as C.
# op records b-jit default, per loop iteration. The interpreter's floor is C + records x 6 (dispatch and
# decode); records only shrink. b-jit and default-mode ceilings ratchet down toward the op's budget (C parity: max(C + 1, 1.25x C),
# ir_budget.txt). Not edited by hand.
";

pub fn render(rows: &BTreeMap<String, Floor>) -> String {
    let field = |value: Option<f64>| value.map_or_else(|| "-".to_owned(), |value| format!("{value:.1}"));
    let body: String = rows.iter().map(|(name, row)| format!("{name} {} {} {}\n", field(row.records), field(row.jit[0]), field(row.jit[1]))).collect();
    format!("{HEADER}{body}")
}

pub fn read(config: &Config) -> (String, BTreeMap<String, Floor>) {
    let text = fs::read_to_string(config.root.join(FLOORS)).unwrap_or_default();
    let rows = parse(&text);
    (text, rows)
}

pub fn write(config: &Config, before: &str, rows: &BTreeMap<String, Floor>) -> Result<(), String> {
    let after = render(rows);
    if after == before { return Ok(()) }
    let path = config.root.join(FLOORS);
    fs::write(&path, after).map_err(|error| format!("{}: {error}", path.display()))
}

/// The layer that owns a handler's instructions, by the first name part it holds (the thread
/// ownership of `/mnt/project-files/rebalance/plan-2026-11-18b.md`).
const OWNERS: &[(&str, &str)] = &[
    ("loop_kernel", "Specialize u-jit on facts"),
    ("LoopKernel", "Specialize u-jit on facts"),
    ("jit code", "Specialize u-jit on facts"),
    ("{closure", "Cut calls and frames to C"),
    ("regexp", "Cut builtin library code to C"),
    ("string", "Cut builtin library code to C"),
    ("concat", "Cut builtin library code to C"),
    ("intern", "Cut builtin library code to C"),
    ("Program::", "Cut builtin library code to C"),
    ("typed_array", "Unify fact store and object model"),
    ("call", "Cut calls and frames to C"),
    ("return", "Cut calls and frames to C"),
    ("construct", "Cut calls and frames to C"),
    ("native", "Cut calls and frames to C"),
    ("rest", "Cut calls and frames to C"),
    ("spread", "Cut calls and frames to C"),
    ("activation", "Cut calls and frames to C"),
    ("root_binding", "Cut environments, GC and heap memory"),
    ("name", "Cut environments, GC and heap memory"),
    ("capture", "Cut environments, GC and heap memory"),
    ("environment", "Cut environments, GC and heap memory"),
    ("closure", "Cut environments, GC and heap memory"),
    ("collect", "Cut environments, GC and heap memory"),
    ("sweep", "Cut environments, GC and heap memory"),
    ("alloc", "Cut environments, GC and heap memory"),
    ("field", "Unify fact store and object model"),
    ("index", "Unify fact store and object model"),
    ("length", "Unify fact store and object model"),
    ("typed", "Unify fact store and object model"),
    ("object", "Unify fact store and object model"),
    ("element", "Unify fact store and object model"),
    ("Vm::", "Cut builtin library code to C"),
    ("lane_row", "Derive every tier from one semantics"),
    ("operator", "Derive every tier from one semantics"),
    ("jump", "Derive every tier from one semantics"),
    ("inc_dec", "Derive every tier from one semantics"),
    ("move", "Derive every tier from one semantics"),
    ("load", "Derive every tier from one semantics"),
];

/// The owner of `handler`'s instructions; the ir-budget kit's own thread triages the rest.
pub fn owner(handler: &str) -> &'static str {
    OWNERS.iter().find(|(part, _)| handler.contains(part)).map_or("Cut calls and frames to C", |(_, owner)| owner)
}

/// One op as the interpreter ran it: name, Ir per iteration, its split, records per iteration.
pub struct Op<'a> {
    pub name: &'a str,
    pub count: f64,
    pub split: &'a [(String, f64)],
    pub records: f64,
}

/// The interpreter's floor check: the waste report (every op above C + records x allowance, the
/// owner of its largest handler), and the ops that dispatch more records than recorded. Ratchets
/// the records down (a run of every op) and fills missing ones.
pub fn interpreter(ops: &[Op], c: &BTreeMap<String, f64>, floors: &mut BTreeMap<String, Floor>, every_op: bool) -> (String, Vec<String>) {
    let mut grown = Vec::new();
    let mut rows: Vec<(f64, String)> = Vec::new();
    let (mut total, mut waste_total) = (0.0, 0.0);
    for op in ops {
        let Some(&c) = c.get(op.name) else { continue };
        let floor_row = floors.entry(op.name.to_owned()).or_default();
        let measured = (op.records * 10.0).round() / 10.0;
        let records = match floor_row.records {
            Some(recorded) if measured > recorded + SAME_RECORDS => {
                grown.push(format!("{}: {measured:.1} records/iter dispatched, recorded {recorded:.1} (the C floor's allowance only shrinks) -> {}", op.name, owner(op.split.first().map_or("", |(handler, _)| handler))));
                recorded
            }
            Some(recorded) if measured + SAME_RECORDS < recorded && every_op => { floor_row.records = Some(measured); measured }
            Some(recorded) => recorded.min(measured),
            None => { floor_row.records = Some(measured); measured }
        };
        let floor = c + records * DISPATCH_ALLOWANCE;
        let waste = (op.count - floor).max(0.0);
        total += op.count;
        waste_total += waste;
        let top = op.split.first().map_or("-", |(handler, _)| handler.as_str());
        rows.push((waste, format!("{:<16}{:>10.1}{:>7.1}{:>8.1}{:>9.1}{:>9.1}{:>6.1}x  {:<36} {top}", op.name, op.count, c, records, floor, waste, op.count / floor.max(1.0), owner(top))));
    }
    rows.sort_by(|a, b| b.0.total_cmp(&a.0));
    let head = format!("{:<16}{:>10}{:>7}{:>8}{:>9}{:>9}{:>7}  {:<36} {}", "op", "Ir/iter", "C", "records", "floor", "waste", "x", "owner", "largest handler");
    let summary = format!("C floor: interpreter = C + records x {DISPATCH_ALLOWANCE} (dispatch and decode); {waste_total:.0} of {total:.0} Ir/iter summed over ops is above it");
    let table = std::iter::once(summary).chain(std::iter::once(head)).chain(rows.into_iter().map(|(_, row)| row)).collect::<Vec<_>>().join("\n");
    (table, grown)
}

/// The JIT tiers' check (`index` 0 b-jit, 1 default) against each op's `budgets` entry (the budget
/// `ir_budget.txt` holds, C parity: max(C + 1, 1.25x C)): a count past its ceiling fails; a drop lowers the ceiling (a
/// run of every op) toward the budget; a missing one is filled.
pub fn jit(index: usize, mode: &str, measured: &[(String, f64, &[(String, f64)])], c: &BTreeMap<String, f64>, budgets: &BTreeMap<String, f64>, floors: &mut BTreeMap<String, Floor>, every_op: bool) -> (String, Vec<String>) {
    let mut grown = Vec::new();
    let mut rows: Vec<(f64, String)> = Vec::new();
    for (name, count, split) in measured {
        let (Some(&c), Some(&budget)) = (c.get(name), budgets.get(name)) else { continue };
        let floor = (count * 10.0).ceil() / 10.0;
        let row = floors.entry(name.clone()).or_default();
        let top = split.first().map_or("-", |(handler, _)| handler.as_str());
        match row.jit[index] {
            Some(ceiling) if *count > ceiling + tolerance(ceiling) => grown.push(format!("{name}: {mode} {count:.1} Ir/iter, ceiling {ceiling:.1} (budget {budget:.1}) -> {}", owner(top))),
            Some(ceiling) if *count + SAME_COUNT < ceiling && every_op => row.jit[index] = Some(floor.max(budget.min(ceiling))),
            Some(_) => {}
            None => row.jit[index] = Some(floor.max(budget)),
        }
        rows.push((count / budget, format!("{name:<16}{count:>10.1}{c:>7.1}{budget:>8.1}{:>7.1}x  {:<36} {top}", count / budget, owner(top))));
    }
    rows.sort_by(|a, b| b.0.total_cmp(&a.0));
    let over = rows.iter().filter(|(ratio, _)| *ratio > 1.0).count();
    let head = format!("{:<16}{:>10}{:>7}{:>8}{:>8}  {:<36} {}", "op", "Ir/iter", "C", "budget", "x", "owner", "largest handler");
    let summary = format!("{mode}: {over} of {} ops above their budget (C parity: max(C + 1, 1.25x C))", rows.len());
    let table = std::iter::once(summary).chain(std::iter::once(head)).chain(rows.into_iter().map(|(_, row)| row)).collect::<Vec<_>>().join("\n");
    (table, grown)
}
