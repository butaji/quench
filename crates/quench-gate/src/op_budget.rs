//! The per-op budgets of the ir-budget step: each primitive op has an absolute instruction budget
//! made of the operations it runs, and every tier is held to it. `a.x` costs at most 5
//! instructions, `i + 1` at most 3 and a call at most 20 (`crates/quench-tier-corpus/op_budgets.txt`
//! names the units and each op's composition). An op's count is taken net of the `empty` op (the
//! loop's own step, which has its own budget), so the budget is what the op's body may cost.
//!
//! An op over its budget in a tier is a fault. Each tier's faults are held by its shrink-only debt
//! list (`OVER`), as the step's other debt lists are (`ir_budget::hold_debt`): an unlisted fault
//! fails the step, and an entry that no longer faults must be removed. The report
//! names, per overrun, the handlers the count is made of, largest first: where the waste is.

use std::collections::{BTreeMap, BTreeSet};

pub const BUDGETS: &str = "crates/quench-tier-corpus/op_budgets.txt";
/// Each tier's debt list: the ops over their budget in that tier.
pub const OVER: [(&str, &str); 3] = [
    ("no-jit", "crates/quench-tier-corpus/op_over_budget_no_jit.txt"),
    ("b-jit", "crates/quench-tier-corpus/op_over_budget_b_jit.txt"),
    ("u-jit", "crates/quench-tier-corpus/op_over_budget_u_jit.txt"),
];
/// The op whose count is the loop's own step, which every other op's count is taken net of.
pub const LOOP: &str = "empty";
/// A net count within this many instructions of its budget is within it.
const SAME_COUNT: f64 = 0.5;
/// Handlers named per overrun.
const NAMED: usize = 4;

/// The budget file: the unit budgets (`@unit count`) and each op's budget, the sum of its terms
/// (`op unit+unit*2+...`).
#[derive(Debug, Default, PartialEq)]
pub struct Budgets {
    pub units: BTreeMap<String, f64>,
    pub ops: BTreeMap<String, (f64, String)>,
}

pub fn parse(text: &str) -> Result<Budgets, String> {
    let mut budgets = Budgets::default();
    let lines = text.lines().map(|line| line.split('#').next().unwrap_or_default().trim()).filter(|line| !line.is_empty());
    let (units, ops): (Vec<&str>, Vec<&str>) = lines.partition(|line| line.starts_with('@'));
    for line in units {
        let mut fields = line[1..].split_whitespace();
        let (Some(unit), Some(count)) = (fields.next(), fields.next().and_then(|count| count.parse::<f64>().ok())) else {
            return Err(format!("{BUDGETS}: bad unit line `{line}`"));
        };
        budgets.units.insert(unit.to_owned(), count);
    }
    for line in ops {
        let mut fields = line.split_whitespace();
        let (Some(op), Some(terms)) = (fields.next(), fields.next()) else { return Err(format!("{BUDGETS}: bad op line `{line}`")) };
        let mut total = 0.0;
        for term in terms.split('+') {
            let (unit, times) = match term.split_once('*') {
                Some((unit, times)) => (unit, times.parse::<f64>().map_err(|_| format!("{BUDGETS}: bad count in `{term}`"))?),
                None => (term, 1.0),
            };
            let unit = budgets.units.get(unit).ok_or_else(|| format!("{BUDGETS}: `{op}` names unknown unit `{unit}`"))?;
            total += unit * times;
        }
        budgets.ops.insert(op.to_owned(), (total, terms.to_owned()));
    }
    Ok(budgets)
}

/// One tier's run of an op: its count per iteration and that count split by handler.
pub struct Run<'a> {
    pub op: &'a str,
    pub count: f64,
    pub split: &'a [(String, f64)],
}

/// The net count of each budgeted op in one tier's run: the loop op's own count, every other op's
/// count less the loop's. `None` where the run did not measure the loop op.
fn net_counts<'a>(runs: &'a [Run<'a>], budgets: &Budgets) -> Option<Vec<(&'a Run<'a>, f64)>> {
    let empty = runs.iter().find(|run| run.op == LOOP)?.count;
    Some(runs.iter().filter(|run| budgets.ops.contains_key(run.op))
        .map(|run| (run, if run.op == LOOP { run.count } else { run.count - empty })).collect())
}

/// One tier's verdict: the budgeted ops it judged and those over their budget.
#[derive(Debug, Default, PartialEq)]
pub struct Judged {
    pub judged: BTreeSet<String>,
    pub over: BTreeSet<String>,
}

/// Each tier's verdict against the budgets (a tier whose run lacks the loop op judges nothing), and
/// the report: each overrun with its net count and the handlers it is made of, worst ratio first.
pub fn judge(tiers: &[(&str, &[Run])], budgets: &Budgets) -> (BTreeMap<String, Judged>, String) {
    let mut verdicts: BTreeMap<String, Judged> = BTreeMap::new();
    let mut overruns = 0;
    let mut lines: Vec<(f64, String)> = Vec::new();
    let mut skipped = Vec::new();
    for (tier, runs) in tiers {
        let Some(nets) = net_counts(runs, budgets) else { skipped.push(*tier); continue };
        let verdict = verdicts.entry((*tier).to_owned()).or_default();
        for (run, net) in nets {
            let (budget, terms) = &budgets.ops[run.op];
            verdict.judged.insert(run.op.to_owned());
            if net <= budget + SAME_COUNT { continue }
            verdict.over.insert(run.op.to_owned());
            overruns += 1;
            let handlers: Vec<String> = run.split.iter().take(NAMED).map(|(handler, share)| format!("{share:.0} {handler}")).collect();
            lines.push((net / budget.max(1.0), format!("{:<16}{tier:<8}{net:>9.1}{budget:>8.1}{:>7.1}x  {terms:<24} {}", run.op, net / budget.max(1.0), handlers.join(", "))));
        }
    }
    lines.sort_by(|a, b| b.0.total_cmp(&a.0));
    let head = format!("{:<16}{:<8}{:>9}{:>8}{:>8}  {:<24} {}", "op", "tier", "net", "budget", "over", "composition", "handlers (Ir/iter, largest first)");
    let note = if skipped.is_empty() { String::new() } else { format!(" ({} not judged: `{LOOP}` not measured)", skipped.join(", ")) };
    let title = format!("op budgets: {overruns} overruns over {} budgeted ops, net of `{LOOP}`{note}", budgets.ops.len());
    let report = std::iter::once(title).chain(std::iter::once(head)).chain(lines.into_iter().map(|(_, line)| line)).collect::<Vec<_>>().join("\n");
    (verdicts, report)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "# units\n@arith 3\n@field 5\n@loop 9\nempty loop\nint_add arith*2 # (a + i) | 0\nfield_read field+arith*2\n";

    #[test]
    fn budgets_sum_their_units() {
        let budgets = parse(FILE).unwrap();
        assert_eq!(budgets.ops["empty"].0, 9.0);
        assert_eq!(budgets.ops["int_add"].0, 6.0);
        assert_eq!(budgets.ops["field_read"].0, 11.0);
        assert!(parse("@a 1\nx b\n").is_err());
    }

    #[test]
    fn counts_are_net_of_the_loop() {
        let budgets = parse(FILE).unwrap();
        let split = vec![("lane_get_field".to_owned(), 30.0)];
        let runs = [Run { op: "empty", count: 31.0, split: &[] }, Run { op: "int_add", count: 37.0, split: &[] }, Run { op: "field_read", count: 69.0, split: &split }];
        let (verdicts, report) = judge(&[("no-jit", &runs)], &budgets);
        // empty 31 > 9; int_add 6 net is within 6; field_read 38 net > 11.
        assert_eq!(verdicts["no-jit"].over, ["empty", "field_read"].into_iter().map(str::to_owned).collect());
        assert_eq!(verdicts["no-jit"].judged.len(), 3);
        assert!(report.contains("30 lane_get_field"));
    }

    #[test]
    fn a_run_without_the_loop_is_not_judged() {
        let budgets = parse(FILE).unwrap();
        let runs = [Run { op: "int_add", count: 99.0, split: &[] }];
        let (verdicts, report) = judge(&[("u-jit", &runs)], &budgets);
        assert!(verdicts.is_empty());
        assert!(report.contains("u-jit not judged"));
    }
}
