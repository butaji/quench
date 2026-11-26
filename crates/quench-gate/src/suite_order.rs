//! The tier order of a suite (TOD-2152, user: interpreter < b-jit < u-jit at every level): for each
//! bench and case, going up the ladder no-jit -> b-jit -> default must not make it slower (5% band
//! for time noise) or run more instructions per op (2% band). RSS order is reported, not judged,
//! since a JIT holds its code.
//!
//! Input: `order.txt` in the sweep directory, ending in `DONE`, one line per measured cell:
//! `<bench>/<case> <mode> <ms> <ir_per_op|-> <peak_rss_kb|?>`. A pair is judged on a metric only
//! where both modes have it.
//!
//! The debt list (`Suite::order`) is shrink-only like the score list: `<bench>/<case> <from>-<to>
//! <time|ir> <ratio>` per inversion, the ratio upper/lower recorded when it inverted. An unlisted
//! inversion fails, a listed one that no longer inverts fails ("remove it"), and a listed ratio
//! that grew past its band fails as a regression. `QUENCH_DEBT_WRITE=1` seeds a header-only list
//! and otherwise only removes and tightens.

use std::collections::{BTreeMap, BTreeSet};

/// The ladder, lowest tier first.
const LADDER: [&str; 3] = ["no-jit", "b-jit", "default"];
const TIME_BAND: f64 = 0.05;
const IR_BAND: f64 = 0.02;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Metric {
    Time,
    Ir,
}

impl Metric {
    fn name(self) -> &'static str {
        match self {
            Self::Time => "time",
            Self::Ir => "ir",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        match text {
            "time" => Some(Self::Time),
            "ir" => Some(Self::Ir),
            _ => None,
        }
    }

    fn band(self) -> f64 {
        match self {
            Self::Time => TIME_BAND,
            Self::Ir => IR_BAND,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Cell {
    ms: Option<f64>,
    ir: Option<f64>,
    rss: Option<f64>,
}

/// `(cell, mode)` -> its measurements.
pub type Cells = BTreeMap<(String, String), Cell>;
/// `(cell, "from-to", metric)` -> the ratio upper/lower recorded when it inverted.
pub type Debt = BTreeMap<(String, String, Metric), f64>;

pub struct Judgement {
    pub failures: Vec<String>,
    /// The inversions as they stand now.
    pub inversions: Debt,
    /// Listed inversions that no longer invert.
    pub met: BTreeSet<(String, String, Metric)>,
    pub report: String,
}

fn number(text: &str) -> Option<f64> {
    text.parse().ok()
}

pub fn parse_cells(lines: &[&str]) -> Result<Cells, String> {
    let mut cells = BTreeMap::new();
    for line in lines {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let [cell, mode, ms, ir, rss] = fields[..] else {
            return Err(format!("order.txt: `{line}` is not `<bench>/<case> <mode> <ms> <ir_per_op|-> <peak_rss_kb|?>`"));
        };
        if !LADDER.contains(&mode) {
            return Err(format!("order.txt: `{line}`: unknown mode `{mode}`"));
        }
        let value = Cell { ms: number(ms), ir: number(ir), rss: number(rss) };
        if cells.insert((cell.to_owned(), mode.to_owned()), value).is_some() {
            return Err(format!("order.txt: `{cell} {mode}` is listed twice"));
        }
    }
    Ok(cells)
}

pub fn parse_debt(file: &str, text: &str) -> Result<Debt, String> {
    let mut debt = BTreeMap::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty() && !line.starts_with('#')) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let [cell, pair, metric, ratio] = fields[..] else {
            return Err(format!("{file}: `{line}` is not `<bench>/<case> <from>-<to> <time|ir> <ratio>`"));
        };
        let metric = Metric::parse(metric).ok_or_else(|| format!("{file}: `{line}`: the metric is not time or ir"))?;
        let ratio = number(ratio).ok_or_else(|| format!("{file}: `{line}`: the ratio is not a number"))?;
        if debt.insert((cell.to_owned(), pair.to_owned(), metric), ratio).is_some() {
            return Err(format!("{file}: `{cell} {pair} {}` is listed twice", metric.name()));
        }
    }
    Ok(debt)
}

pub fn render_debt(header: &str, debt: &Debt) -> String {
    let lines: String = debt.iter().map(|((cell, pair, metric), ratio)| format!("{cell} {pair} {} {ratio:.3}\n", metric.name())).collect();
    format!("{header}{lines}")
}

pub fn default_header(step: &str) -> String {
    format!("# Tier order (quench-gate step `{step}`, TOD-2152): no-jit -> b-jit -> default must not get slower (5% band) or run more Ir per op (2% band).\n\
# `<bench>/<case> <from>-<to> <time|ir> <ratio>`: an inversion, the ratio upper/lower recorded when it inverted.\n\
# Shrink-only: an unlisted inversion fails, a listed one that no longer inverts fails (\"remove it\"), a ratio that grew past its band fails.\n\
# Seeded by `QUENCH_DEBT_WRITE=1 quench-gate --only {step}` on a finished sweep. Never add a line by hand.\n")
}

/// The value of `metric` in `cell`.
fn value(cell: &Cell, metric: Metric) -> Option<f64> {
    match metric {
        Metric::Time => cell.ms,
        Metric::Ir => cell.ir,
    }
}

pub fn judge(file: &str, cells: &Cells, debt: &Debt) -> Judgement {
    let names: BTreeSet<&String> = cells.keys().map(|(cell, _)| cell).collect();
    let mut failures = Vec::new();
    let mut inversions = BTreeMap::new();
    let mut met = BTreeSet::new();
    let mut lines = Vec::new();
    let mut rss_inversions = 0;
    for name in names {
        for pair in LADDER.windows(2) {
            let (Some(lower), Some(upper)) = (cells.get(&(name.clone(), pair[0].to_owned())), cells.get(&(name.clone(), pair[1].to_owned()))) else { continue };
            let label = format!("{}-{}", pair[0], pair[1]);
            if let (Some(low), Some(up)) = (lower.rss, upper.rss)
                && up > low
            {
                rss_inversions += 1;
            }
            for metric in [Metric::Time, Metric::Ir] {
                let (Some(low), Some(up)) = (value(lower, metric), value(upper, metric)) else { continue };
                if low <= 0.0 {
                    continue;
                }
                let ratio = up / low;
                let inverted = ratio > 1.0 + metric.band();
                let key = (name.clone(), label.clone(), metric);
                let what = format!("{name} {label} {}", metric.name());
                match (inverted, debt.get(&key)) {
                    (false, None) => {}
                    (false, Some(_)) => {
                        failures.push(format!("{what}: {ratio:.3} no longer inverts; remove it from {file}"));
                        met.insert(key);
                    }
                    (true, None) => {
                        failures.push(format!("{what}: {ratio:.3}x slower going up the ladder, not listed in {file}"));
                        inversions.insert(key, ratio);
                    }
                    (true, Some(recorded)) => {
                        if ratio > recorded * (1.0 + metric.band()) {
                            failures.push(format!("{what}: {ratio:.3} is worse than the recorded {recorded:.3}"));
                        }
                        inversions.insert(key, ratio);
                    }
                }
                if inverted {
                    lines.push(format!("  {what} {ratio:.2}x"));
                }
            }
        }
    }
    for (cell, pair, metric) in debt.keys() {
        if !cells.keys().any(|(name, _)| name == cell) {
            failures.push(format!("{cell} {pair} {}: not measured in this sweep; remove it from {file}", metric.name()));
        }
    }
    let report = format!("tier order: {} inversions ({} listed), {rss_inversions} RSS inversions (reported, not judged)\n{}", inversions.len(), debt.len(), lines.join("\n"));
    Judgement { failures, inversions, met, report }
}

/// Shrink-only rewrite: a header-only list is seeded with every inversion; otherwise listed
/// inversions that no longer invert are removed, the rest tightened to the lower ratio, and
/// unlisted ones are never added.
pub fn shrink(before: &Debt, judged: &Judgement) -> Debt {
    if before.is_empty() {
        return judged.inversions.clone();
    }
    before
        .iter()
        .filter(|(key, _)| !judged.met.contains(*key))
        .map(|(key, recorded)| (key.clone(), judged.inversions.get(key).map_or(*recorded, |now| now.min(*recorded))))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cells(rows: &[(&str, &str, f64, Option<f64>)]) -> Cells {
        rows.iter().map(|(cell, mode, ms, ir)| ((cell.to_string(), mode.to_string()), Cell { ms: Some(*ms), ir: *ir, rss: None })).collect()
    }

    #[test]
    fn an_ordered_ladder_passes() {
        let judged = judge("f", &cells(&[("a/x", "no-jit", 100.0, Some(50.0)), ("a/x", "b-jit", 60.0, Some(30.0)), ("a/x", "default", 61.0, Some(30.5))]), &Debt::new());
        assert!(judged.failures.is_empty(), "{:?}", judged.failures);
    }

    #[test]
    fn an_unlisted_inversion_fails_and_seeds() {
        let measured = cells(&[("a/x", "no-jit", 100.0, None), ("a/x", "b-jit", 120.0, None)]);
        let judged = judge("f", &measured, &Debt::new());
        assert_eq!(judged.failures.len(), 1);
        let seeded = shrink(&Debt::new(), &judged);
        assert!(judge("f", &measured, &seeded).failures.is_empty());
    }

    #[test]
    fn a_fixed_inversion_must_leave_the_list_and_a_worse_one_fails() {
        let debt = parse_debt("f", "a/x no-jit-b-jit time 1.2\n").unwrap();
        let fixed = judge("f", &cells(&[("a/x", "no-jit", 100.0, None), ("a/x", "b-jit", 90.0, None)]), &debt);
        assert!(fixed.failures[0].contains("remove it"));
        assert!(shrink(&debt, &fixed).is_empty());
        let worse = judge("f", &cells(&[("a/x", "no-jit", 100.0, None), ("a/x", "b-jit", 140.0, None)]), &debt);
        assert!(worse.failures[0].contains("worse than the recorded"));
    }
}
