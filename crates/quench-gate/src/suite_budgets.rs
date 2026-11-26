//! Steps: the suite budgets, one judge for every benchmark suite held to the user's target
//! (`Suite`): JetStream 3 (`js3-budgets`) and the data-structure suite (`ds-budgets`, TOD-2275,
//! quench-bench/data-structures). Each reads the outputs of a finished sweep and of the comparator
//! runs (one `<engine> <test> <score|FAIL> <peak_rss_kb>` line each) and judges every bench in every
//! mode against its peers. The step never runs a benchmark.
//!
//! Targets, one rule for every bench:
//! - score: Quench >= 1.5 x the best peer (JetStream no-jit: node-jitless and bun-nojit; every
//!   other suite and mode: node and bun). A peer that FAILed is skipped.
//! - RSS: Quench <= 0.5 x the lowest peer RSS, the same peers.
//! - any Quench FAIL, or a Quench run with no RSS, is a failure and is never listed.
//! - Ir (no-jit callgrind): no growth past the recorded ceiling.
//!
//! The suite's debt file (`crates/quench-tier-corpus/js3_budgets.txt`, `ds_budgets.txt`) is the
//! shrink-only debt list: one line per
//! `<bench> <mode> <score|rss> <value>` that misses its target, with the value recorded when it
//! missed, and one `ir <bench> <ceiling>` line per bench. A miss the list does not hold fails the
//! step, including every miss while the list is header-only (an empty list allows nothing); a
//! listed entry that now meets its target fails it too ("remove it"); a listed value that got
//! worse than recorded fails as a regression (3% tolerance for score and RSS, 1% for Ir).
//!
//! `QUENCH_DEBT_WRITE=1` is shrink-only too. It removes a listed entry that now meets its target,
//! moves a recorded miss to the current value only when that is closer to the target (a score
//! that rose, an RSS that fell), and lowers a recorded ir ceiling to a lower measured count. It
//! keeps every other line as it is, and never raises or loosens a value. It adds only two things:
//! a bench with a measured count and no ceiling gets one, and a header-only (or missing) list is
//! seeded from the inputs. A new miss is never added to a non-empty list. The rewritten list is
//! then judged, so the write exits nonzero if it still fails.
//!
//! Inputs: the suite's sweep variable (`QUENCH_JS3_SWEEP`, `QUENCH_DS_SWEEP`: a sweep directory
//! holding `score.txt` and `ir.txt`, each ending in `DONE`) and its comparators variable
//! (`QUENCH_JS3_COMPARATORS`, `QUENCH_DS_COMPARATORS`). quench-bench/data-structures/run.py and
//! ir.py write the data-structure suite's files with `--gate DIR`. Unset, the step fails: a full run
//! must judge a finished sweep. To leave it out, run with `--quick` or `--only <other step>`.

use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

use crate::{config::Config, step::Verdict, suite_order};

/// One suite the step judges: where its debt list lives, the variables naming its inputs, and
/// whether its no-jit rows are held against the peers' jitless runs.
pub struct Suite {
    step: &'static str,
    debt: &'static str,
    sweep_variable: &'static str,
    comparators_variable: &'static str,
    title: &'static str,
    /// JetStream holds no-jit against node-jitless and bun-nojit; the data-structure suite holds
    /// every mode against node and bun (user, 2026-11-25: "C/Node/Bun you target for").
    jitless_peers: bool,
    /// The tier-order debt list, where the suite's sweep writes `order.txt` (`suite_order`).
    order: Option<&'static str>,
}

pub const JS3: Suite = Suite {
    step: "js3-budgets",
    debt: "crates/quench-tier-corpus/js3_budgets.txt",
    sweep_variable: "QUENCH_JS3_SWEEP",
    comparators_variable: "QUENCH_JS3_COMPARATORS",
    title: "JetStream 3 budgets",
    jitless_peers: true,
    order: None,
};

pub const DS: Suite = Suite {
    step: "ds-budgets",
    debt: "crates/quench-tier-corpus/ds_budgets.txt",
    sweep_variable: "QUENCH_DS_SWEEP",
    comparators_variable: "QUENCH_DS_COMPARATORS",
    title: "Data-structure suite budgets (TOD-2275)",
    jitless_peers: false,
    order: Some("crates/quench-tier-corpus/ds_tier_order.txt"),
};

/// `1` rewrites the debt list shrink-only (seeds it when it is header-only) instead of judging.
const DEBT_WRITE: &str = "QUENCH_DEBT_WRITE";

impl Suite {
    /// The header a seeded debt list starts with.
    fn default_header(&self) -> String {
        format!("# {} (quench-gate step `{}`; reads {} and {}).\n\
# `<bench> <mode> <score|rss> <value>`: a bench and mode that misses its target, with the value recorded when it missed.\n\
# `ir <bench> <ceiling>`: the no-jit instruction ceiling of a bench.\n\
# Shrink-only: an unlisted miss fails the step, a listed entry that meets its target fails it too (\"remove it\"),\n\
# and a listed value that got worse fails as a regression (3% for score and RSS, 1% for Ir).\n\
# Seeded by `QUENCH_DEBT_WRITE=1 quench-gate --only {}` on a finished sweep. Never add a line by hand.\n",
            self.title, self.step, self.sweep_variable, self.comparators_variable, self.step)
    }
}
const DONE: &str = "DONE";
const FAIL: &str = "FAIL";
/// The RSS a run prints when it printed none.
const UNMEASURED: &str = "?";
/// The three shipped modes (user, 2026-11-22): interpreter only, b-jit only, and the default (every
/// tier on), as `sweep3.sh` names them.
const MODES: [&str; 3] = ["no-jit", "b-jit", "default"];
const ENGINES: [&str; 4] = ["node", "bun", "node-jitless", "bun-nojit"];
const SCORE_TARGET: f64 = 1.5;
const RSS_TARGET: f64 = 0.5;
const SCORE_TOLERANCE: f64 = 0.03;
const RSS_TOLERANCE: f64 = 0.03;
const IR_TOLERANCE: f64 = 0.01;

/// `(mode, bench)` -> Quench's run.
type Scores = BTreeMap<(String, String), Run>;
/// `(engine, bench)` -> the comparator's run.
type Comparators = BTreeMap<(String, String), Run>;
/// bench -> its no-jit instruction count (`None` when callgrind printed none).
type IrCounts = BTreeMap<String, Option<f64>>;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Run {
    /// `None` when the run FAILed.
    score: Option<f64>,
    /// Peak RSS in KB; `None` when none was printed.
    rss: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Metric {
    Score,
    Rss,
}

impl Metric {
    fn name(self) -> &'static str {
        match self {
            Self::Score => "score",
            Self::Rss => "rss",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        match text {
            "score" => Some(Self::Score),
            "rss" => Some(Self::Rss),
            _ => None,
        }
    }
}

/// The debt list: what misses, and the ceilings.
#[derive(Debug, Default, PartialEq)]
struct Debt {
    /// `(bench, mode, metric)` -> the value recorded when it missed.
    misses: BTreeMap<(String, String, Metric), f64>,
    /// bench -> the Ir ceiling.
    ceilings: BTreeMap<String, f64>,
}

/// How one metric (or the Ir count) of a row reads.
struct Reading {
    /// The failure it adds, if any.
    failure: Option<String>,
    /// Its word in the report.
    word: &'static str,
    /// Sort key of the row: 0 for a failure, 1 for a listed miss, 2 without a peer, 3 for ok.
    rank: u8,
    /// The current value when it misses, which a debt line records.
    miss: Option<f64>,
    /// Whether it meets its target while the list still holds it (the "remove it" case).
    met: bool,
}

impl Reading {
    fn hard(failure: String, word: &'static str) -> Self {
        Self { failure: Some(failure), word, rank: 0, miss: None, met: false }
    }
}

/// One (bench, mode) row of the report.
struct Row {
    bench: String,
    mode: String,
    score: Option<f64>,
    best_peer: Option<(&'static str, f64)>,
    score_ratio: Option<f64>,
    rss_ratio: Option<f64>,
    ir: Option<f64>,
    verdict: String,
    rank: u8,
}

struct Judgement {
    rows: Vec<Row>,
    failures: Vec<String>,
    /// The misses as they stand now, for `QUENCH_DEBT_WRITE`.
    misses: BTreeMap<(String, String, Metric), f64>,
    /// The listed entries that meet their target now, for `QUENCH_DEBT_WRITE`.
    met: BTreeSet<(String, String, Metric)>,
}

/// The lines of a sweep file, which must end in `DONE`: a sweep still running is not judged.
fn finished<'a>(text: &'a str, file: &str) -> Result<Vec<&'a str>, String> {
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|line| !line.is_empty()).collect();
    match lines.split_last() {
        Some((last, body)) if *last == DONE => Ok(body.to_vec()),
        _ => Err(format!("{file} is not finished: its last line is not {DONE}")),
    }
}

fn fields(line: &str) -> Vec<&str> {
    line.split_whitespace().collect()
}

/// A score as a sweep writes it: `FAIL` is `None`.
fn score_of(text: &str, what: &str) -> Result<Option<f64>, String> {
    if text == FAIL {
        return Ok(None);
    }
    text.parse().map(Some).map_err(|_| format!("{what}: `{text}` is not a score or {FAIL}"))
}

/// An RSS in KB as a run prints it: `?` is `None`.
fn rss_of(text: &str, what: &str) -> Result<Option<f64>, String> {
    if text == UNMEASURED {
        return Ok(None);
    }
    text.parse().map(Some).map_err(|_| format!("{what}: `{text}` is not an RSS in KB or {UNMEASURED}"))
}

fn parse_sweep_scores(text: &str) -> Result<Scores, String> {
    let mut scores = BTreeMap::new();
    for line in finished(text, "score.txt")? {
        let what = format!("score.txt: `{line}`");
        let [mode, bench, score, rss] = fields(line)[..] else {
            return Err(format!("{what} is not `<mode> <test> <score|FAIL> <peak_rss_kb>`"));
        };
        if !MODES.contains(&mode) {
            return Err(format!("{what}: unknown mode `{mode}`"));
        }
        let run = Run { score: score_of(score, &what)?, rss: rss_of(rss, &what)? };
        if scores.insert((mode.to_owned(), bench.to_owned()), run).is_some() {
            return Err(format!("score.txt: `{mode} {bench}` is listed twice"));
        }
    }
    Ok(scores)
}

/// `ir.txt`: `<test> <instructions>`, where the count is empty when callgrind printed none.
fn parse_ir(text: &str) -> Result<IrCounts, String> {
    let mut ir = BTreeMap::new();
    for line in finished(text, "ir.txt")? {
        let (bench, count): (&str, Option<f64>) = match fields(line)[..] {
            [bench] => (bench, None),
            [bench, count] => (bench, count.parse::<f64>().ok()),
            _ => return Err(format!("ir.txt: `{line}` is not `<test> <instructions>`")),
        };
        if ir.insert(bench.to_owned(), count).is_some() {
            return Err(format!("ir.txt: `{bench}` is listed twice"));
        }
    }
    Ok(ir)
}

fn parse_comparators(text: &str) -> Result<Comparators, String> {
    let mut runs = BTreeMap::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty() && *line != DONE) {
        let what = format!("comparators: `{line}`");
        let [engine, bench, score, rss] = fields(line)[..] else {
            return Err(format!("{what} is not `<engine> <test> <score|FAIL> <peak_rss_kb>`"));
        };
        if !ENGINES.contains(&engine) {
            return Err(format!("{what}: unknown engine `{engine}`"));
        }
        let run = Run { score: score_of(score, &what)?, rss: rss_of(rss, &what)? };
        if runs.insert((engine.to_owned(), bench.to_owned()), run).is_some() {
            return Err(format!("comparators: `{engine} {bench}` is listed twice"));
        }
    }
    Ok(runs)
}

fn parse_debt(suite: &Suite, text: &str) -> Result<Debt, String> {
    let debt_file = suite.debt;
    let mut debt = Debt::default();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty() && !line.starts_with('#')) {
        match fields(line)[..] {
            ["ir", bench, ceiling] => {
                let value: f64 = ceiling.parse().map_err(|_| format!("{debt_file}: `{line}`: the ceiling is not a number"))?;
                if debt.ceilings.insert(bench.to_owned(), value).is_some() {
                    return Err(format!("{debt_file}: `ir {bench}` is listed twice"));
                }
            }
            [bench, mode, metric, value] => {
                let Some(metric) = Metric::parse(metric) else {
                    return Err(format!("{debt_file}: `{line}`: the metric is not score or rss"));
                };
                if !MODES.contains(&mode) {
                    return Err(format!("{debt_file}: `{line}`: unknown mode `{mode}`"));
                }
                let value: f64 = value.parse().map_err(|_| format!("{debt_file}: `{line}`: the value is not a number"))?;
                if debt.misses.insert((bench.to_owned(), mode.to_owned(), metric), value).is_some() {
                    return Err(format!("{debt_file}: `{bench} {mode} {}` is listed twice", metric.name()));
                }
            }
            _ => return Err(format!("{debt_file}: `{line}` is not `<bench> <mode> <score|rss> <value>` or `ir <bench> <ceiling>`")),
        }
    }
    Ok(debt)
}

fn render_debt(header: &str, debt: &Debt) -> String {
    let misses: String = debt.misses.iter().map(|((bench, mode, metric), value)| format!("{bench} {mode} {} {value}\n", metric.name())).collect();
    let ceilings: String = debt.ceilings.iter().map(|(bench, ceiling)| format!("ir {bench} {ceiling}\n")).collect();
    format!("{header}{misses}{ceilings}")
}

fn header(text: &str) -> String {
    text.lines().take_while(|line| line.starts_with('#')).map(|line| format!("{line}\n")).collect()
}

/// The peers a mode is held against: the interpreter modes against the jitless engines, the JIT
/// modes against the engines with their JIT.
fn peers(suite: &Suite, mode: &str) -> [&'static str; 2] {
    match mode {
        "no-jit" if suite.jitless_peers => ["node-jitless", "bun-nojit"],
        _ => ["node", "bun"],
    }
}

/// Whether a value misses its target: a score below 1.5 x its peer, an RSS above 0.5 x its peer.
fn missed(metric: Metric, ratio: f64) -> bool {
    match metric {
        Metric::Score => ratio < SCORE_TARGET,
        Metric::Rss => ratio > RSS_TARGET,
    }
}

/// Whether a listed miss got worse than its recorded value, past the tolerance.
fn worse(metric: Metric, current: f64, recorded: f64) -> bool {
    match metric {
        Metric::Score => current < recorded * (1.0 - SCORE_TOLERANCE),
        Metric::Rss => current > recorded * (1.0 + RSS_TOLERANCE),
    }
}

fn listed(debt: &Debt, bench: &str, mode: &str, metric: Metric) -> Option<f64> {
    debt.misses.get(&(bench.to_owned(), mode.to_owned(), metric)).copied()
}

/// One metric against its peers. `missed` is `None` when there is no peer to judge it against.
fn judge_metric(debt_file: &str, bench: &str, mode: &str, metric: Metric, current: f64, missed: Option<bool>, listed: Option<f64>) -> Reading {
    let name = format!("{bench} {mode} {}", metric.name());
    match (missed, listed) {
        (None, _) => Reading { failure: None, word: "no peer", rank: 2, miss: None, met: false },
        (Some(false), None) => Reading { failure: None, word: "ok", rank: 3, miss: None, met: false },
        (Some(false), Some(_)) => Reading { failure: Some(format!("{name}: {current} meets its target; remove it from {debt_file}")), word: "remove it", rank: 0, miss: None, met: true },
        (Some(true), None) => Reading { failure: Some(format!("{name}: {current} misses its target and is not listed in {debt_file}")), word: "NEW MISS", rank: 0, miss: Some(current), met: false },
        (Some(true), Some(recorded)) if worse(metric, current, recorded) => Reading { failure: Some(format!("{name}: {current} is worse than the recorded {recorded}")), word: "REGRESSED", rank: 0, miss: Some(current), met: false },
        (Some(true), Some(_)) => Reading { failure: None, word: "debt", rank: 1, miss: Some(current), met: false },
    }
}

/// The Ir count of a bench against its ceiling.
fn judge_ir(debt_file: &str, bench: &str, measured: Option<f64>, ceiling: Option<f64>) -> Reading {
    let name = format!("{bench} ir");
    match (measured, ceiling) {
        (None, _) => Reading::hard(format!("{name}: no instruction count in the sweep"), "no Ir"),
        (Some(_), None) => Reading::hard(format!("{name}: no ceiling recorded; seed {debt_file} with {DEBT_WRITE}=1"), "unrecorded"),
        (Some(ir), Some(ceiling)) if ir > ceiling * (1.0 + IR_TOLERANCE) => Reading { failure: Some(format!("{name}: {ir} is past its ceiling {ceiling}")), word: "REGRESSED", rank: 0, miss: None, met: false },
        (Some(_), Some(_)) => Reading { failure: None, word: "ok", rank: 3, miss: None, met: false },
    }
}

/// The value a recorded miss moves to on a shrink: the current one only when it is closer to the
/// target than the recorded one, so a worse or equal value keeps the recorded line.
fn tighter(metric: Metric, recorded: f64, current: f64) -> f64 {
    match metric {
        Metric::Score => recorded.max(current),
        Metric::Rss => recorded.min(current),
    }
}

/// The debt list after `QUENCH_DEBT_WRITE=1`, shrink-only. An empty list is seeded with every
/// current miss and ceiling. Otherwise: a listed miss that now meets its target is removed, a
/// listed miss that is still a miss is tightened (never loosened), a listed miss that could not
/// be judged this sweep is kept, and a miss the list does not hold is not added. A recorded
/// ceiling drops to the measured count when that is lower and is never raised; a bench with a
/// measured count and no recorded ceiling gets one (adding a ceiling does not loosen anything).
fn shrink(before: &Debt, judged: &Judgement, ir: &IrCounts) -> Debt {
    if *before == Debt::default() {
        let ceilings = ir.iter().filter_map(|(bench, count)| count.map(|count| (bench.clone(), count))).collect();
        return Debt { misses: judged.misses.clone(), ceilings };
    }
    let mut misses = BTreeMap::new();
    for (key, recorded) in &before.misses {
        match judged.misses.get(key) {
            Some(current) => {
                misses.insert(key.clone(), tighter(key.2, *recorded, *current));
            }
            None if judged.met.contains(key) => {}
            None => {
                misses.insert(key.clone(), *recorded);
            }
        }
    }
    let mut ceilings = BTreeMap::new();
    for (bench, recorded) in &before.ceilings {
        let measured = ir.get(bench).copied().flatten();
        ceilings.insert(bench.clone(), measured.map_or(*recorded, |count| count.min(*recorded)));
    }
    for (bench, count) in ir {
        if let (Some(count), false) = (count, before.ceilings.contains_key(bench)) {
            ceilings.insert(bench.clone(), *count);
        }
    }
    Debt { misses, ceilings }
}

fn judge(suite: &Suite, scores: &Scores, ir: &IrCounts, comparators: &Comparators, debt: &Debt) -> Judgement {
    let debt_file = suite.debt;
    let mut rows = Vec::new();
    let mut failures = Vec::new();
    let mut misses = BTreeMap::new();
    let mut met = BTreeSet::new();
    for ((mode, bench), quench) in scores {
        // A comparator that FAILed is skipped, for both its score and its RSS.
        let runs: Vec<(&'static str, Run)> = peers(suite, mode)
            .into_iter()
            .filter_map(|engine| comparators.get(&(engine.to_owned(), bench.clone())).map(|run| (engine, *run)))
            .filter(|(_, run)| run.score.is_some())
            .collect();
        let best = runs.iter().filter_map(|(engine, run)| run.score.map(|score| (*engine, score))).max_by(|a, b| a.1.total_cmp(&b.1));
        let best_rss = runs.iter().filter_map(|(_, run)| run.rss).min_by(f64::total_cmp);
        let score_ratio = quench.score.zip(best).map(|(score, (_, peer))| score / peer);
        let rss_ratio = quench.rss.zip(best_rss).map(|(rss, peer)| rss / peer);

        let score = match quench.score {
            None => Reading::hard(format!("{bench} {mode}: Quench FAILed"), "FAIL"),
            Some(value) => judge_metric(debt_file, bench, mode, Metric::Score, value, score_ratio.map(|ratio| missed(Metric::Score, ratio)), listed(debt, bench, mode, Metric::Score)),
        };
        let rss = match quench.rss {
            None => Reading::hard(format!("{bench} {mode}: no peak RSS printed"), "no RSS"),
            Some(value) => judge_metric(debt_file, bench, mode, Metric::Rss, value, rss_ratio.map(|ratio| missed(Metric::Rss, ratio)), listed(debt, bench, mode, Metric::Rss)),
        };
        let mut readings = vec![("score", score), ("rss", rss)];
        let ir_count = if mode == "no-jit" { ir.get(bench).copied().flatten() } else { None };
        if mode == "no-jit" {
            readings.push(("ir", judge_ir(debt_file, bench, ir_count, debt.ceilings.get(bench).copied())));
        }
        let key = |metric| (bench.clone(), mode.clone(), metric);
        for (metric, (_, reading)) in [Metric::Score, Metric::Rss].into_iter().zip(&readings) {
            if let Some(value) = reading.miss {
                misses.insert(key(metric), value);
            }
            if reading.met {
                met.insert(key(metric));
            }
        }
        failures.extend(readings.iter().filter_map(|(_, reading)| reading.failure.clone()));
        rows.push(Row {
            bench: bench.clone(),
            mode: mode.clone(),
            score: quench.score,
            best_peer: best,
            score_ratio,
            rss_ratio,
            ir: ir_count,
            verdict: readings.iter().map(|(label, reading)| format!("{label} {}", reading.word)).collect::<Vec<_>>().join(", "),
            rank: readings.iter().map(|(_, reading)| reading.rank).min().unwrap_or(3),
        });
    }
    // Shrink-only: a listed entry for a bench or mode this sweep did not measure is stale.
    for (bench, mode, metric) in debt.misses.keys() {
        if !scores.contains_key(&(mode.clone(), bench.clone())) {
            failures.push(format!("{bench} {mode} {}: not measured in this sweep; remove it from {debt_file}", metric.name()));
        }
    }
    for bench in debt.ceilings.keys() {
        if !ir.contains_key(bench) {
            failures.push(format!("ir {bench}: not measured in this sweep; remove it from {debt_file}"));
        }
    }
    // Worst first: failures, then listed misses, then rows without a peer, then ok; within a rank,
    // the lowest score ratio first.
    let worst = |row: &Row| row.score_ratio.unwrap_or(f64::NEG_INFINITY);
    rows.sort_by(|a, b| a.rank.cmp(&b.rank).then_with(|| worst(a).total_cmp(&worst(b))));
    Judgement { rows, failures, misses, met }
}

fn ratio(value: Option<f64>) -> String {
    value.map_or_else(|| "-".to_owned(), |ratio| format!("{ratio:.2}x"))
}

fn table(rows: &[Row]) -> String {
    let head = format!("{:<18}{:<11}{:>9}  {:<18}{:>9}{:>9}{:>13}  {}", "bench", "mode", "quench", "best peer", "score x", "rss x", "ir", "verdict");
    let lines = rows.iter().map(|row| {
        let score = row.score.map_or_else(|| FAIL.to_owned(), |score| score.to_string());
        let peer = row.best_peer.map_or_else(|| "-".to_owned(), |(engine, score)| format!("{engine} {score}"));
        let ir = row.ir.map_or_else(|| "-".to_owned(), |ir| format!("{ir:.0}"));
        format!("{:<18}{:<11}{:>9}  {:<18}{:>9}{:>9}{:>13}  {}", row.bench, row.mode, score, peer, ratio(row.score_ratio), ratio(row.rss_ratio), ir, row.verdict)
    });
    std::iter::once(head).chain(lines).collect::<Vec<_>>().join("\n")
}

fn read(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))
}

/// The sweep and comparator paths from their variables; both must be set.
fn inputs(suite: &Suite, sweep: Option<OsString>, comparators: Option<OsString>) -> Result<(PathBuf, PathBuf), String> {
    match (sweep, comparators) {
        (Some(sweep), Some(comparators)) => Ok((PathBuf::from(sweep), PathBuf::from(comparators))),
        _ => Err(format!("{} needs a finished sweep: set {} and {}\nto leave the step out, run with --quick or --only <other step>", suite.step, suite.sweep_variable, suite.comparators_variable)),
    }
}

/// The verdict of a judgement: its failures first, then the summary and the table.
fn verdict_of(suite: &Suite, judgement: &Judgement, table: &str, note: &str) -> Verdict {
    let summary = format!("{note}: {} rows, {} failures (debt list {})", judgement.rows.len(), judgement.failures.len(), suite.debt);
    if judgement.failures.is_empty() {
        return Ok(format!("{summary}\n{table}"));
    }
    Err(format!("{}\n\n{summary}\n{table}", judgement.failures.join("\n")))
}

/// The `js3-budgets` step.
pub fn run_js3(config: &Config) -> Verdict {
    run(config, &JS3)
}

/// The `ds-budgets` step.
pub fn run_ds(config: &Config) -> Verdict {
    run(config, &DS)
}

fn run(config: &Config, suite: &Suite) -> Verdict {
    let order = match suite.order {
        Some(file) => Some(order(config, suite.step, file, &env::var_os(suite.sweep_variable).map(PathBuf::from).unwrap_or_default())?),
        None => None,
    };
    let scores = budgets(config, suite);
    match (scores, order) {
        (Ok(scores), None) => Ok(scores),
        (Ok(scores), Some(Ok(order))) => Ok(format!("{scores}\n{order}")),
        (Err(scores), Some(Ok(order) | Err(order))) => Err(format!("{scores}\n{order}")),
        (Ok(scores), Some(Err(order))) => Err(format!("{order}\n\n{scores}")),
        (Err(scores), None) => Err(scores),
    }
}

/// The tier order of the suite's sweep against its debt list (`suite_order`), shrink-only under
/// `QUENCH_DEBT_WRITE=1` like the score list. The outer error is an input error.
fn order(config: &Config, step: &str, file: &str, sweep: &Path) -> Result<Verdict, String> {
    let cells = suite_order::parse_cells(&finished(&read(&sweep.join("order.txt"))?, "order.txt")?)?;
    let path = config.root.join(file);
    let write = env::var(DEBT_WRITE).as_deref() == Ok("1");
    let before = match read(&path) {
        Ok(text) => text,
        Err(_) if write => String::new(),
        Err(error) => return Err(format!("{error}; seed it with {DEBT_WRITE}=1 on a finished sweep")),
    };
    let recorded = suite_order::parse_debt(file, &before)?;
    let mut judged = suite_order::judge(file, &cells, &recorded);
    if write {
        let next = suite_order::shrink(&recorded, &judged);
        let head = if before.is_empty() { suite_order::default_header(step) } else { header(&before) };
        let text = suite_order::render_debt(&head, &next);
        if text != before {
            fs::write(&path, &text).map_err(|error| format!("{}: {error}", path.display()))?;
        }
        judged = suite_order::judge(file, &cells, &next);
    }
    Ok(if judged.failures.is_empty() { Ok(judged.report) } else { Err(format!("{}\n{}", judged.failures.join("\n"), judged.report)) })
}

fn budgets(config: &Config, suite: &Suite) -> Verdict {
    let (sweep, comparators) = inputs(suite, env::var_os(suite.sweep_variable), env::var_os(suite.comparators_variable))?;
    let scores = parse_sweep_scores(&read(&sweep.join("score.txt"))?)?;
    let ir = parse_ir(&read(&sweep.join("ir.txt"))?)?;
    let comparators = parse_comparators(&read(&comparators)?)?;

    let debt_path = config.root.join(suite.debt);
    let write = env::var(DEBT_WRITE).as_deref() == Ok("1");
    let before = match read(&debt_path) {
        Ok(text) => text,
        Err(_) if write => String::new(),
        Err(error) => return Err(format!("{error}; seed it with {DEBT_WRITE}=1 on a finished sweep")),
    };
    let recorded = parse_debt(suite, &before)?;
    let judgement = judge(suite, &scores, &ir, &comparators, &recorded);
    if !write {
        return verdict_of(suite, &judgement, &table(&judgement.rows), suite.title);
    }

    let next = shrink(&recorded, &judgement, &ir);
    let head = if before.is_empty() { suite.default_header() } else { header(&before) };
    let text = render_debt(&head, &next);
    if text != before {
        fs::write(&debt_path, &text).map_err(|error| format!("{}: {error}", debt_path.display()))?;
    }
    // The written list is judged too: a miss it still does not hold fails the write.
    let rewritten = judge(suite, &scores, &ir, &comparators, &next);
    let note = format!("wrote {} (shrink-only): {} misses, {} ceilings", suite.debt, next.misses.len(), next.ceilings.len());
    verdict_of(suite, &rewritten, &table(&rewritten.rows), &note)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_of(score: Option<f64>, rss: Option<f64>) -> Run {
        Run { score, rss }
    }

    fn pair(mode: &str, bench: &str) -> (String, String) {
        (mode.to_owned(), bench.to_owned())
    }

    /// One bench `b` in no-jit against node-jitless and bun-nojit; Ir 100 and its ceiling 100.
    fn one_bench(quench: Run, jitless: Run, bun: Run) -> (Scores, IrCounts, Comparators) {
        let scores = BTreeMap::from([(pair("no-jit", "b"), quench)]);
        let ir = BTreeMap::from([("b".to_owned(), Some(100.0))]);
        let comparators = BTreeMap::from([(pair("node-jitless", "b"), jitless), (pair("bun-nojit", "b"), bun)]);
        (scores, ir, comparators)
    }

    fn debt(misses: &[(&str, &str, Metric, f64)], ceilings: &[(&str, f64)]) -> Debt {
        Debt {
            misses: misses.iter().map(|(bench, mode, metric, value)| ((bench.to_string(), mode.to_string(), *metric), *value)).collect(),
            ceilings: ceilings.iter().map(|(bench, value)| (bench.to_string(), *value)).collect(),
        }
    }

    /// Quench at 20 with 10 RSS against peers at 10 score and 50 RSS: every target met.
    fn passing() -> (Scores, IrCounts, Comparators) {
        one_bench(run_of(Some(20.0), Some(10.0)), run_of(Some(10.0), Some(50.0)), run_of(Some(5.0), Some(40.0)))
    }

    #[test]
    fn the_data_structure_suite_holds_no_jit_against_the_jit_peers() {
        // No-jit Quench at 20 against node 30 and node-jitless 10: DS judges it against node (a miss),
        // JetStream against node-jitless (met).
        let scores = BTreeMap::from([(pair("no-jit", "b"), run_of(Some(20.0), Some(10.0)))]);
        let ir = BTreeMap::from([("b".to_owned(), Some(100.0))]);
        let comparators = BTreeMap::from([(pair("node", "b"), run_of(Some(30.0), Some(50.0))), (pair("node-jitless", "b"), run_of(Some(10.0), Some(50.0)))]);
        let ceilings = debt(&[], &[("b", 100.0)]);
        assert!(judge(&DS, &scores, &ir, &comparators, &ceilings).failures.iter().any(|failure| failure.contains("not listed in crates/quench-tier-corpus/ds_budgets.txt")));
        assert!(judge(&JS3, &scores, &ir, &comparators, &ceilings).failures.is_empty());
    }

    #[test]
    fn meeting_every_target_passes() {
        let (scores, ir, comparators) = passing();
        let judged = judge(&JS3, &scores, &ir, &comparators, &debt(&[], &[("b", 100.0)]));
        assert!(judged.failures.is_empty(), "{:?}", judged.failures);
        assert!(judged.misses.is_empty());
        assert_eq!(judged.rows[0].verdict, "score ok, rss ok, ir ok");
    }

    #[test]
    fn a_failed_peer_is_skipped() {
        // bun-nojit FAILed with a lower RSS (10); it must not set the RSS target, so 20 against 50 is 0.4x.
        let (scores, ir, comparators) = one_bench(run_of(Some(20.0), Some(20.0)), run_of(Some(10.0), Some(50.0)), run_of(None, Some(10.0)));
        let judged = judge(&JS3, &scores, &ir, &comparators, &debt(&[], &[("b", 100.0)]));
        assert!(judged.failures.is_empty(), "{:?}", judged.failures);
        assert_eq!(judged.rows[0].rss_ratio, Some(0.4));
    }

    #[test]
    fn an_unlisted_score_miss_fails_and_is_recorded() {
        let (scores, ir, comparators) = one_bench(run_of(Some(12.0), Some(10.0)), run_of(Some(10.0), Some(50.0)), run_of(Some(5.0), Some(40.0)));
        let judged = judge(&JS3, &scores, &ir, &comparators, &debt(&[], &[("b", 100.0)]));
        assert_eq!(judged.failures.len(), 1, "{:?}", judged.failures);
        assert!(judged.failures[0].contains("not listed"));
        assert_eq!(judged.misses.get(&("b".to_owned(), "no-jit".to_owned(), Metric::Score)), Some(&12.0));
    }

    #[test]
    fn unset_inputs_fail_instead_of_skipping() {
        let error = inputs(&JS3, None, None).unwrap_err();
        assert!(error.starts_with("js3-budgets needs a finished sweep: set QUENCH_JS3_SWEEP and QUENCH_JS3_COMPARATORS"), "{error}");
        assert!(inputs(&JS3, Some("sweep".into()), None).is_err());
        assert!(inputs(&JS3, None, Some("comparators".into())).is_err());
        assert!(inputs(&JS3, Some("sweep".into()), Some("comparators".into())).is_ok());
    }

    #[test]
    fn a_header_only_debt_list_allows_no_miss() {
        let (scores, ir, comparators) = one_bench(run_of(Some(12.0), Some(10.0)), run_of(Some(10.0), Some(50.0)), run_of(Some(5.0), Some(40.0)));
        let header_only = parse_debt(&JS3, &JS3.default_header()).unwrap();
        assert_eq!(header_only, Debt::default());
        let judged = judge(&JS3, &scores, &ir, &comparators, &header_only);
        assert!(judged.failures.iter().any(|failure| failure.contains("b no-jit score") && failure.contains("not listed")), "{:?}", judged.failures);
    }

    #[test]
    fn a_new_miss_is_not_added_to_a_non_empty_list() {
        let (scores, ir, comparators) = one_bench(run_of(Some(12.0), Some(10.0)), run_of(Some(10.0), Some(50.0)), run_of(Some(5.0), Some(40.0)));
        let recorded = debt(&[("c", "no-jit", Metric::Score, 3.0)], &[("b", 100.0)]);
        let judged = judge(&JS3, &scores, &ir, &comparators, &recorded);
        let next = shrink(&recorded, &judged, &ir);
        assert!(!next.misses.contains_key(&("b".to_owned(), "no-jit".to_owned(), Metric::Score)));
        assert_eq!(next.misses.get(&("c".to_owned(), "no-jit".to_owned(), Metric::Score)), Some(&3.0), "unmeasured entries are kept");
        assert_eq!(next.ceilings, recorded.ceilings);
        assert!(judge(&JS3, &scores, &ir, &comparators, &next).failures.iter().any(|failure| failure.contains("not listed")));
    }

    #[test]
    fn a_header_only_list_is_seeded_with_its_misses() {
        let (scores, ir, comparators) = one_bench(run_of(Some(12.0), Some(10.0)), run_of(Some(10.0), Some(50.0)), run_of(Some(5.0), Some(40.0)));
        let judged = judge(&JS3, &scores, &ir, &comparators, &Debt::default());
        let seeded = shrink(&Debt::default(), &judged, &ir);
        assert_eq!(seeded, debt(&[("b", "no-jit", Metric::Score, 12.0)], &[("b", 100.0)]));
        assert!(judge(&JS3, &scores, &ir, &comparators, &seeded).failures.is_empty());
    }

    #[test]
    fn an_improved_listed_miss_is_tightened() {
        // Score 13 against a recorded 12: still a miss (1.3x), but closer to the target.
        let (scores, ir, comparators) = one_bench(run_of(Some(13.0), Some(10.0)), run_of(Some(10.0), Some(50.0)), run_of(Some(5.0), Some(40.0)));
        let recorded = debt(&[("b", "no-jit", Metric::Score, 12.0)], &[("b", 100.0)]);
        let next = shrink(&recorded, &judge(&JS3, &scores, &ir, &comparators, &recorded), &ir);
        assert_eq!(next.misses.get(&("b".to_owned(), "no-jit".to_owned(), Metric::Score)), Some(&13.0));
        // RSS 28 against a recorded 30 (lowest peer 50, so 0.56x): tightened down to 28.
        let (scores, ir, comparators) = one_bench(run_of(Some(20.0), Some(28.0)), run_of(Some(10.0), Some(50.0)), run_of(Some(5.0), Some(70.0)));
        let recorded = debt(&[("b", "no-jit", Metric::Rss, 30.0)], &[("b", 100.0)]);
        let next = shrink(&recorded, &judge(&JS3, &scores, &ir, &comparators, &recorded), &ir);
        assert_eq!(next.misses.get(&("b".to_owned(), "no-jit".to_owned(), Metric::Rss)), Some(&28.0));
    }

    #[test]
    fn a_worse_listed_miss_keeps_its_recorded_value() {
        let (scores, ir, comparators) = one_bench(run_of(Some(11.0), Some(10.0)), run_of(Some(10.0), Some(50.0)), run_of(Some(5.0), Some(40.0)));
        let recorded = debt(&[("b", "no-jit", Metric::Score, 12.0)], &[("b", 100.0)]);
        let judged = judge(&JS3, &scores, &ir, &comparators, &recorded);
        let next = shrink(&recorded, &judged, &ir);
        assert_eq!(next.misses.get(&("b".to_owned(), "no-jit".to_owned(), Metric::Score)), Some(&12.0));
        assert!(judge(&JS3, &scores, &ir, &comparators, &next).failures.iter().any(|failure| failure.contains("worse than the recorded")));
    }

    #[test]
    fn a_listed_entry_that_meets_its_target_is_removed() {
        let (scores, ir, comparators) = passing();
        let recorded = debt(&[("b", "no-jit", Metric::Score, 12.0)], &[("b", 100.0)]);
        let next = shrink(&recorded, &judge(&JS3, &scores, &ir, &comparators, &recorded), &ir);
        assert!(next.misses.is_empty(), "{:?}", next.misses);
        assert_eq!(next.ceilings, recorded.ceilings);
    }

    #[test]
    fn a_listed_miss_with_no_peer_is_kept() {
        let (scores, ir, comparators) = one_bench(run_of(Some(1.0), Some(10.0)), run_of(None, None), run_of(None, None));
        let recorded = debt(&[("b", "no-jit", Metric::Score, 12.0)], &[("b", 100.0)]);
        let next = shrink(&recorded, &judge(&JS3, &scores, &ir, &comparators, &recorded), &ir);
        assert_eq!(next.misses, recorded.misses);
    }

    #[test]
    fn a_recorded_ceiling_drops_to_a_lower_measurement() {
        let (scores, _, comparators) = passing();
        let lower = BTreeMap::from([("b".to_owned(), Some(90.0))]);
        let recorded = debt(&[], &[("b", 100.0)]);
        let next = shrink(&recorded, &judge(&JS3, &scores, &lower, &comparators, &recorded), &lower);
        assert_eq!(next.ceilings.get("b"), Some(&90.0));
    }

    #[test]
    fn a_recorded_ceiling_is_never_raised() {
        let (scores, _, comparators) = passing();
        let higher = BTreeMap::from([("b".to_owned(), Some(105.0))]);
        let recorded = debt(&[], &[("b", 100.0)]);
        let next = shrink(&recorded, &judge(&JS3, &scores, &higher, &comparators, &recorded), &higher);
        assert_eq!(next.ceilings.get("b"), Some(&100.0));
        let missing = BTreeMap::from([("b".to_owned(), None)]);
        let next = shrink(&recorded, &judge(&JS3, &scores, &missing, &comparators, &recorded), &missing);
        assert_eq!(next.ceilings.get("b"), Some(&100.0));
    }

    #[test]
    fn a_bench_without_a_ceiling_gets_one_on_write() {
        let (scores, ir, comparators) = passing();
        // A non-empty list (an unmeasured entry for `c`) with no ceiling for `b`.
        let recorded = debt(&[("c", "no-jit", Metric::Score, 3.0)], &[]);
        let judged = judge(&JS3, &scores, &ir, &comparators, &recorded);
        assert!(judged.failures.iter().any(|failure| failure.contains("b ir")), "{:?}", judged.failures);
        let next = shrink(&recorded, &judged, &ir);
        assert_eq!(next.ceilings.get("b"), Some(&100.0));
        assert!(!judge(&JS3, &scores, &ir, &comparators, &next).failures.iter().any(|failure| failure.contains("b ir")));
    }

    #[test]
    fn a_no_peer_bench_still_passes_with_a_new_ceiling() {
        let (scores, ir, comparators) = one_bench(run_of(Some(1.0), Some(10.0)), run_of(None, None), run_of(None, None));
        let recorded = debt(&[("c", "no-jit", Metric::Score, 3.0)], &[]);
        let next = shrink(&recorded, &judge(&JS3, &scores, &ir, &comparators, &recorded), &ir);
        assert_eq!(next.ceilings.get("b"), Some(&100.0));
        assert!(!next.misses.contains_key(&("b".to_owned(), "no-jit".to_owned(), Metric::Score)));
        let judged = judge(&JS3, &scores, &ir, &comparators, &next);
        assert!(!judged.failures.iter().any(|failure| failure.contains("b no-jit") || failure.contains("b ir")), "{:?}", judged.failures);
        assert_eq!(judged.rows[0].verdict, "score no peer, rss no peer, ir ok");
    }

    #[test]
    fn a_listed_miss_within_tolerance_is_debt() {
        let (scores, ir, comparators) = one_bench(run_of(Some(11.7), Some(10.0)), run_of(Some(10.0), Some(50.0)), run_of(Some(5.0), Some(40.0)));
        let judged = judge(&JS3, &scores, &ir, &comparators, &debt(&[("b", "no-jit", Metric::Score, 12.0)], &[("b", 100.0)]));
        assert!(judged.failures.is_empty(), "{:?}", judged.failures);
        assert_eq!(judged.rows[0].verdict, "score debt, rss ok, ir ok");
    }

    #[test]
    fn a_listed_score_miss_past_tolerance_is_a_regression() {
        let (scores, ir, comparators) = one_bench(run_of(Some(11.0), Some(10.0)), run_of(Some(10.0), Some(50.0)), run_of(Some(5.0), Some(40.0)));
        let judged = judge(&JS3, &scores, &ir, &comparators, &debt(&[("b", "no-jit", Metric::Score, 12.0)], &[("b", 100.0)]));
        assert_eq!(judged.failures.len(), 1, "{:?}", judged.failures);
        assert!(judged.failures[0].contains("worse than the recorded 12"));
    }

    #[test]
    fn a_listed_entry_that_meets_its_target_says_remove_it() {
        let (scores, ir, comparators) = passing();
        let judged = judge(&JS3, &scores, &ir, &comparators, &debt(&[("b", "no-jit", Metric::Score, 12.0)], &[("b", 100.0)]));
        assert_eq!(judged.failures.len(), 1, "{:?}", judged.failures);
        assert!(judged.failures[0].contains("remove it"));
    }

    #[test]
    fn an_rss_miss_is_judged_against_the_lowest_peer() {
        // 30 KB against a lowest peer of 50 KB is 0.6x: a miss. Recorded 30, 30.5 is within 3%.
        let (scores, ir, comparators) = one_bench(run_of(Some(20.0), Some(30.0)), run_of(Some(10.0), Some(50.0)), run_of(Some(5.0), Some(70.0)));
        let unlisted = judge(&JS3, &scores, &ir, &comparators, &debt(&[], &[("b", 100.0)]));
        assert!(unlisted.failures[0].contains("rss"), "{:?}", unlisted.failures);
        let (scores, ir, comparators) = one_bench(run_of(Some(20.0), Some(30.5)), run_of(Some(10.0), Some(50.0)), run_of(Some(5.0), Some(70.0)));
        let listed = judge(&JS3, &scores, &ir, &comparators, &debt(&[("b", "no-jit", Metric::Rss, 30.0)], &[("b", 100.0)]));
        assert!(listed.failures.is_empty(), "{:?}", listed.failures);
        assert_eq!(listed.rows[0].verdict, "score ok, rss debt, ir ok");
    }

    #[test]
    fn a_quench_fail_is_a_failure_and_never_debt() {
        let (scores, ir, comparators) = one_bench(run_of(None, Some(10.0)), run_of(Some(10.0), Some(50.0)), run_of(Some(5.0), Some(40.0)));
        let judged = judge(&JS3, &scores, &ir, &comparators, &debt(&[("b", "no-jit", Metric::Score, 1.0)], &[("b", 100.0)]));
        assert!(judged.failures.iter().any(|failure| failure.contains("FAILed")), "{:?}", judged.failures);
        assert!(judged.misses.is_empty());
    }

    #[test]
    fn no_peer_is_not_judged() {
        let (scores, ir, comparators) = one_bench(run_of(Some(1.0), Some(10.0)), run_of(None, None), run_of(None, None));
        let judged = judge(&JS3, &scores, &ir, &comparators, &debt(&[], &[("b", 100.0)]));
        assert!(judged.failures.is_empty(), "{:?}", judged.failures);
        assert_eq!(judged.rows[0].verdict, "score no peer, rss no peer, ir ok");
    }

    #[test]
    fn ir_growth_past_the_ceiling_fails() {
        let (scores, _, comparators) = passing();
        let grown = BTreeMap::from([("b".to_owned(), Some(102.0))]);
        assert_eq!(judge(&JS3, &scores, &grown, &comparators, &debt(&[], &[("b", 100.0)])).failures.len(), 1);
        let within = BTreeMap::from([("b".to_owned(), Some(100.9))]);
        assert!(judge(&JS3, &scores, &within, &comparators, &debt(&[], &[("b", 100.0)])).failures.is_empty());
    }

    #[test]
    fn ir_without_a_ceiling_or_a_count_fails() {
        let (scores, ir, comparators) = passing();
        assert_eq!(judge(&JS3, &scores, &ir, &comparators, &debt(&[], &[])).failures.len(), 1);
        let missing = BTreeMap::from([("b".to_owned(), None)]);
        assert_eq!(judge(&JS3, &scores, &missing, &comparators, &debt(&[], &[("b", 100.0)])).failures.len(), 1);
    }

    #[test]
    fn stale_entries_for_unmeasured_benches_fail() {
        let (scores, ir, comparators) = passing();
        let judged = judge(&JS3, &scores, &ir, &comparators, &debt(&[("gone", "no-jit", Metric::Score, 3.0)], &[("b", 100.0), ("gone", 5.0)]));
        assert_eq!(judged.failures.len(), 2, "{:?}", judged.failures);
    }

    #[test]
    fn the_sweep_must_be_finished_and_well_formed() {
        assert!(parse_sweep_scores("no-jit b 10.5 1000\n").is_err());
        let scores = parse_sweep_scores("no-jit b 10.5 1000\nno-jit c FAIL ?\nDONE\n").unwrap();
        assert_eq!(scores[&pair("no-jit", "b")], run_of(Some(10.5), Some(1000.0)));
        assert_eq!(scores[&pair("no-jit", "c")], run_of(None, None));
        assert!(parse_sweep_scores("zz b 1 2\nDONE\n").is_err());
        assert!(parse_sweep_scores("no-jit b 1\nDONE\n").is_err());
    }

    #[test]
    fn ir_counts_may_be_empty() {
        let ir = parse_ir("b 1234\nc \nDONE\n").unwrap();
        assert_eq!(ir[&"b".to_owned()], Some(1234.0));
        assert_eq!(ir[&"c".to_owned()], None);
        assert!(parse_ir("b 1234\n").is_err());
    }

    #[test]
    fn comparators_name_known_engines() {
        let runs = parse_comparators("node b 10 50\nbun-nojit b FAIL ?\n").unwrap();
        assert_eq!(runs[&pair("bun-nojit", "b")], run_of(None, None));
        assert!(parse_comparators("deno b 10 50\n").is_err());
    }

    #[test]
    fn the_debt_list_round_trips_and_rejects_duplicates() {
        let text = "# header\nb no-jit score 12.5\nc default rss 300\nir b 100\n";
        let parsed = parse_debt(&JS3, text).unwrap();
        assert_eq!(parsed, debt(&[("b", "no-jit", Metric::Score, 12.5), ("c", "default", Metric::Rss, 300.0)], &[("b", 100.0)]));
        assert_eq!(render_debt("# header\n", &parsed), text);
        assert!(parse_debt(&JS3, "b no-jit score 1\nb no-jit score 2\n").is_err());
        assert!(parse_debt(&JS3, "b xx score 1\n").is_err());
    }

    #[test]
    fn the_report_lists_worst_first() {
        let scores = BTreeMap::from([(pair("no-jit", "good"), run_of(Some(30.0), Some(10.0))), (pair("no-jit", "bad"), run_of(Some(11.0), Some(10.0)))]);
        let ir = BTreeMap::from([("good".to_owned(), Some(1.0)), ("bad".to_owned(), Some(1.0))]);
        let comparators = BTreeMap::from([
            (pair("node-jitless", "good"), run_of(Some(10.0), Some(50.0))),
            (pair("node-jitless", "bad"), run_of(Some(10.0), Some(50.0))),
        ]);
        let judged = judge(&JS3, &scores, &ir, &comparators, &debt(&[], &[("good", 1.0), ("bad", 1.0)]));
        assert_eq!(judged.rows[0].bench, "bad");
        assert!(table(&judged.rows).lines().nth(1).unwrap().starts_with("bad"));
    }
}
