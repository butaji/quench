//! Step: AOT. Each C analogue in `quench-bench/c-analogues` names its JetStream row (`analogue`).
//! The row's files run as one program (`program`), which the AOT-enabled `quench-node` compiles
//! ahead of time; the AOT binary must print what the interpreter prints and exit as it does, and
//! both are timed against the C reference (`row`). The ratios are held to `aot-ratios.txt`
//! (`ratios`). The generic-row budget (`exact`) must be zero under `--strict`, and `--suites`
//! adds test262 and the Wasm spec suite in AOT mode (`suites`).

mod analogue;
mod driver;
mod exact;
mod program;
mod ratios;
mod row;
mod suites;

use std::{
    fs, io,
    path::Path,
};

use crate::{build::NODE, config::Config, step::Verdict};
use ratios::{PRECISION, Settled, Table};

const RATIOS: &str = "crates/quench-tier-corpus/aot-ratios.txt";
/// A row whose AOT binary takes more than this many times C's wall-clock time is over target.
const TARGET: f64 = 5.0;

/// Empties `path` and creates it, so a run's outputs are its own.
fn fresh(path: &Path) -> Result<(), String> {
    match fs::remove_dir_all(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(format!("{}: {error}", path.display())),
        _ => {}
    }
    fs::create_dir_all(path).map_err(|error| format!("{}: {error}", path.display()))
}

/// What the rows settled to: the failures, the report lines and the counts of ceilings moved.
#[derive(Default)]
struct Report {
    failures: Vec<String>,
    lines: Vec<String>,
    over: Vec<String>,
    lowered: usize,
    recorded: usize,
    unrecorded: usize,
}

impl Report {
    /// Settles one row's ratio against its ceiling and notes whether it is over target.
    fn settle(&mut self, table: &mut Table, row: &str, ratio: f64, record: bool) {
        self.lines.push(format!("{row} {ratio:.PRECISION$}x"));
        if ratio > TARGET {
            self.over.push(row.to_owned());
        }
        match table.settle(row, ratio, record) {
            Settled::Held => {}
            Settled::Grew { limit } => self.failures.push(format!("{row}: ratio {ratio:.PRECISION$} exceeds its ceiling {limit:.PRECISION$}")),
            Settled::Lowered => self.lowered += 1,
            Settled::Recorded => self.recorded += 1,
            Settled::Unrecorded => self.unrecorded += 1,
        }
    }

    /// The step's verdict: failures, then over-target rows under `--strict`.
    fn verdict(mut self, strict: bool, rows: usize) -> Verdict {
        if strict && !self.over.is_empty() {
            self.failures.push(format!("over target {TARGET:.1}: {}", self.over.join(", ")));
        }
        if !self.failures.is_empty() {
            return Err(self.failures.join("\n"));
        }
        let summary = format!(
            "{rows} rows: {} ceilings lowered, {} recorded, {} not recorded yet (--record); {} over target {TARGET:.1}",
            self.lowered,
            self.recorded,
            self.unrecorded,
            self.over.len()
        );
        Ok(std::iter::once(summary).chain(self.lines).collect::<Vec<_>>().join("\n"))
    }
}

/// The generic-row budget's report line, or its failure under `--strict`.
fn budget(config: &Config, report: &mut Report) {
    match exact::check(config) {
        Ok(line) => report.lines.push(line),
        Err(error) => report.failures.push(error),
    }
}

pub fn run(config: &Config) -> Verdict {
    let analogues = analogue::discover(&config.root.join(analogue::DIRECTORY))?;
    if analogues.is_empty() {
        return Err(format!("no C analogues in {}", analogue::DIRECTORY));
    }
    let host = config.binary(NODE)?;
    let path = config.root.join(RATIOS);
    let before = fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut table = Table::parse(&before)?;
    let mut report = Report::default();
    for analogue in &analogues {
        match row::measure(config, analogue, &host) {
            Ok(ratio) => report.settle(&mut table, &analogue.row, ratio, config.record),
            Err(error) => report.failures.push(format!("{}: {error}", analogue.row)),
        }
    }
    let after = table.render();
    if after != before {
        fs::write(&path, after).map_err(|error| format!("{}: {error}", path.display()))?;
    }
    budget(config, &mut report);
    if config.suites {
        match suites::run(config) {
            Ok(text) => report.lines.push(text),
            Err(error) => report.failures.push(error),
        }
    }
    report.verdict(config.strict, analogues.len())
}
