//! Step `ir-predict` (S1.12, TOD-2153): the static instruction prediction of each staged row, judged before
//! any benchmark op measures it. quench-runtime-engine's `ir_predict` test compiles each row's u-jit (`Emit`)
//! projection on the row's common operand facts, counts the machine instructions of its fast path and holds
//! `crates/quench-tier-corpus/ir_predict.txt` to them; this step runs that test, then joins each prediction
//! with the row's C twin (`c_twin_rows.txt`, `ir_budget.txt` column C) and its 3C+6 bound (S1.17), and writes
//! the table to `<target>/ir-predict.txt`. The ir-budget step's c-bound section reads the same file for a
//! row whose kernel its run did not measure.
//!
//! `QUENCH_IR_PREDICT_MEASURED=<ir-budget report>` adds each row's measured default-mode count from that
//! report's tier table, and the correlation of prediction and measurement over the rows that have both.
//!
//! Alone: `quench-gate --only ir-predict`.

use std::{collections::BTreeMap, env, fs};

use crate::{c_bound::{self, Twin}, config::{Config, PROFILE}, ir_budget, runtime_tests, step::Verdict};

/// The crate whose test generates and holds the predictions (`Emit` is private to it).
const PACKAGE: &str = "quench-runtime-engine";
const TEST: &str = "ir_predict";
/// The report, under the cargo target directory.
const REPORT: &str = "ir-predict.txt";
/// An ir-budget report whose tier table supplies measured default-mode counts.
const MEASURED: &str = "QUENCH_IR_PREDICT_MEASURED";
/// The tier table's header in an ir-budget report: `op no-jit default aot C d/C breaks`.
const TIER_HEADER: [&str; 4] = ["op", "no-jit", "default", "aot"];
/// The tier table's default-mode column.
const DEFAULT_COLUMN: usize = 2;

/// The fewest pairs a correlation is stated over: two points always lie on a line.
const FEWEST_PAIRS: usize = 3;

/// One predicted row joined to its twin: the C count and the measured default count where known.
struct Joined<'a> {
    row: &'a str,
    predicted: f64,
    facts: &'a str,
    kernel: Option<&'a str>,
    c: Option<f64>,
    measured: Option<f64>,
}

pub fn run(config: &Config) -> Verdict {
    let tally = runtime_tests::package_tally(config, PACKAGE, PROFILE, &[TEST.to_owned()])?;
    if !tally.failed.is_empty() {
        return Err(tally.excerpt);
    }
    let read = |path: &str| fs::read_to_string(config.root.join(path)).map_err(|error| format!("{path}: {error}"));
    let rows = c_bound::parse_rows(&read(c_bound::ROWS)?);
    let predictions = read(c_bound::PREDICTIONS)?;
    let c = ir_budget::checked_in_c_counts(config)?;
    let measured = match env::var(MEASURED) {
        Ok(path) => tier_defaults(&fs::read_to_string(&path).map_err(|error| format!("{MEASURED}={path}: {error}"))?),
        Err(_) => BTreeMap::new(),
    };
    let report = report(&join(&rows, &predictions, &c, &measured), &unpredicted(&predictions));
    let path = config.target.join(REPORT);
    fs::write(&path, &report).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(format!("{report}\nwritten to {}", path.display()))
}

/// Each op's default-mode count in an ir-budget report's tier table (`-` where the mode was not measured).
fn tier_defaults(text: &str) -> BTreeMap<String, f64> {
    let table = text.lines().skip_while(|line| !line.split_whitespace().take(TIER_HEADER.len()).eq(TIER_HEADER)).skip(1);
    table.take_while(|line| !line.trim().is_empty()).filter_map(|line| {
        let fields: Vec<&str> = line.split_whitespace().collect();
        Some(((*fields.first()?).to_owned(), fields.get(DEFAULT_COLUMN)?.parse().ok()?))
    }).collect()
}

/// The predicted rows (`row insns facts`) joined to their kernels' C and measured counts.
fn join<'a>(rows: &'a [(String, Twin)], predictions: &'a str, c: &BTreeMap<String, f64>, measured: &BTreeMap<String, f64>) -> Vec<Joined<'a>> {
    predictions.lines().filter(|line| !line.starts_with('#')).filter_map(|line| {
        let (row, rest) = line.split_once(' ')?;
        let (predicted, facts) = rest.split_once(' ').unwrap_or((rest, ""));
        let predicted = predicted.parse().ok()?;
        let kernel = rows.iter().find(|(name, _)| name == row).and_then(|(_, twin)| match twin {
            Twin::Kernel(kernel) => Some(kernel.as_str()),
            Twin::None(_) => None,
        });
        let count = |counts: &BTreeMap<String, f64>| kernel.and_then(|kernel| counts.get(kernel).copied()).filter(|count| count.is_finite());
        Some(Joined { row, predicted, facts, kernel, c: count(c), measured: count(measured) })
    }).collect()
}

/// The rows the predictor could not compile, with why (`row - reason`).
fn unpredicted(predictions: &str) -> Vec<&str> {
    predictions.lines().filter(|line| !line.starts_with('#') && line.split(' ').nth(1) == Some("-")).collect()
}

/// Pearson's r over `pairs`; `None` under `FEWEST_PAIRS` or with no spread.
fn correlation(pairs: &[(f64, f64)]) -> Option<f64> {
    let n = pairs.len() as f64;
    if pairs.len() < FEWEST_PAIRS { return None; }
    let (mean_x, mean_y) = (pairs.iter().map(|p| p.0).sum::<f64>() / n, pairs.iter().map(|p| p.1).sum::<f64>() / n);
    let covariance: f64 = pairs.iter().map(|(x, y)| (x - mean_x) * (y - mean_y)).sum();
    let spread = |mean: f64, pick: fn(&(f64, f64)) -> f64| pairs.iter().map(|p| (pick(p) - mean) * (pick(p) - mean)).sum::<f64>().sqrt();
    let denominator = spread(mean_x, |p| p.0) * spread(mean_y, |p| p.1);
    (denominator > 0.0).then(|| covariance / denominator)
}

/// One table line: the prediction, the C twin and bound, and the measurement where known.
fn line(joined: &Joined<'_>) -> String {
    let number = |value: Option<f64>| value.map_or("-".to_owned(), |value| format!("{value:.1}"));
    let bound = joined.c.map(c_bound::bound);
    let verdict = match bound { Some(bound) if joined.predicted > bound => "over", Some(_) => "within", None => "-" };
    format!("{:<34}{:>6}  {:<16}{:>6}{:>8}  {:<7}{:>8}  {}", joined.row, joined.predicted, joined.kernel.unwrap_or("-"),
        number(joined.c), number(bound), verdict, number(joined.measured), joined.facts)
}

/// The report: the summary, the table and the unpredicted rows.
fn report(joined: &[Joined<'_>], unpredicted: &[&str]) -> String {
    let twinned = joined.iter().filter(|joined| joined.c.is_some()).count();
    let over = joined.iter().filter(|joined| joined.c.is_some_and(|c| joined.predicted > c_bound::bound(c))).count();
    let pairs: Vec<(f64, f64)> = joined.iter().filter_map(|joined| Some((joined.predicted, joined.measured?))).collect();
    let agreement = match correlation(&pairs) {
        Some(r) => format!("; vs measured default over {} rows: Pearson r = {r:.2}", pairs.len()),
        None if pairs.is_empty() => String::new(),
        None => format!("; vs measured default over {} rows: too few for a correlation", pairs.len()),
    };
    let mut lines = vec![format!("ir-predict: {} rows predicted ({twinned} with a C twin, {over} predicted over 3C+6), {} unpredicted{agreement}",
        joined.len(), unpredicted.len())];
    lines.push(format!("{:<34}{:>6}  {:<16}{:>6}{:>8}  {:<7}{:>8}  {}", "row", "u-jit", "kernel", "C", "3C+6", "bound", "default", "facts"));
    lines.extend(joined.iter().map(line));
    lines.extend(unpredicted.iter().map(|line| format!("unpredicted {line}")));
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    const PREDICTIONS: &str = "# h\nNumberOperation::Arithmetic 3 Add int32\nNumberOperation::BitwiseNot 2 int32\nNumberOperation::Math - unresolved\n";

    #[test]
    fn predictions_join_their_twins_and_measurements() {
        let rows = c_bound::parse_rows("NumberOperation::Arithmetic int_add\nNumberOperation::BitwiseNot - none\nNumberOperation::Math math_sqrt\n");
        let c: BTreeMap<String, f64> = [("int_add".to_owned(), 4.0)].into_iter().collect();
        let measured = tier_defaults("x\nop   no-jit  default  aot  C  d/C  breaks\nint_add  1.7  5.0  4.0  4.0  0.4x\n\nint_add 9 9 9\n");
        assert_eq!(measured.get("int_add"), Some(&5.0));
        let joined = join(&rows, PREDICTIONS, &c, &measured);
        assert_eq!(joined.len(), 2);
        assert_eq!((joined[0].kernel, joined[0].c, joined[0].measured), (Some("int_add"), Some(4.0), Some(5.0)));
        assert_eq!((joined[1].kernel, joined[1].c), (None, None));
        let report = report(&joined, &unpredicted(PREDICTIONS));
        assert!(report.starts_with("ir-predict: 2 rows predicted (1 with a C twin, 0 predicted over 3C+6), 1 unpredicted; vs measured default over 1 rows: too few"), "{report}");
        assert!(report.contains("unpredicted NumberOperation::Math - unresolved"), "{report}");
    }

    #[test]
    fn correlation_is_pearsons() {
        assert_eq!(correlation(&[(1.0, 2.0), (2.0, 4.0), (3.0, 6.0)]).map(|r| (r * 1e9).round()), Some(1e9));
        assert_eq!(correlation(&[(1.0, 2.0), (2.0, 4.0)]), None);
        assert_eq!(correlation(&[(1.0, 2.0), (1.0, 4.0), (1.0, 6.0)]), None);
    }
}
