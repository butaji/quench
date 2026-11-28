//! The tier invariant: on every benchmark the default mode scores at least as
//! well as each single execution tier, each single tier scores at least as well
//! as `--no-jit`, and every mode prints the same program output.
//!
//! The modes of one Quench binary run interleaved in the same rounds (the
//! shared fixture runner rotates their order each round), so host noise
//! lands on every mode alike.

use super::{
    fail, host_identity, now_ns, parse_options_from, run_fixture,
    selected_fixtures, spec, usage, write_new_report, JitMode,
};
use super::{EngineSpec, Options, Suite};
use crate::subcommand::reject_exec_trace;
use serde::Serialize;
use std::{collections::BTreeMap, path::Path};

/// Relative score shortfall that is still noise rather than a violation. Host
/// noise on the reference machine is about +-10% per sample; the median over
/// repeated rounds brings it near this value.
pub(crate) const DEFAULT_TOLERANCE: f64 = 0.05;

/// Upper bound that keeps `score >= peer * (1 - tolerance)` meaningful.
const MAX_TOLERANCE: f64 = 0.5;

/// Selects this mode; the remaining arguments are the ordinary suite options.
pub(crate) const FLAG: &str = "--tier-invariant";
const TOLERANCE_FLAG: &str = "--tier-tolerance";

const BYTES_PER_MIB: u64 = 1 << 20;

const REPORT_SCHEMA: u32 = 1;

/// One way to launch the binary under test.
struct Mode {
    /// Engine name in the run records; Quench engines are named `quench*`.
    engine: &'static str,
    label: &'static str,
    flag: Option<&'static str>,
}

const MODES: &[Mode] = &[
    Mode { engine: "quench-default", label: "default", flag: None },
    Mode { engine: "quench-no-jit", label: "no-jit", flag: Some("--no-jit") },
];

/// `upper` must score at least `lower * (1 - tolerance)`: default must not trail the interpreter.
const RELATIONS: &[(&str, &str)] = &[("default", "no-jit")];

/// Command-line choices of `--tier-invariant`.
pub(crate) struct Config {
    pub(crate) tolerance: f64,
}

impl Config {
    /// Splits this mode's own flags from the arguments the suite parser takes.
    fn extract(args: &[String]) -> (Self, Vec<String>) {
        let mut config = Self { tolerance: DEFAULT_TOLERANCE };
        let mut rest = Vec::new();
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                FLAG => {}
                TOLERANCE_FLAG => {
                    config.tolerance = args
                        .next()
                        .and_then(|value| value.parse().ok())
                        .filter(|tolerance| Self::tolerance_is_valid(*tolerance))
                        .unwrap_or_else(|| usage("--tier-tolerance requires a fraction in [0, 0.5)"));
                }
                _ => rest.push(arg.clone()),
            }
        }
        (config, rest)
    }

    fn tolerance_is_valid(tolerance: f64) -> bool {
        (0.0..MAX_TOLERANCE).contains(&tolerance)
    }

}

#[derive(Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub(crate) enum Violation {
    /// `upper` scored below `lower` by more than the tolerance.
    Slower { upper: String, lower: String, upper_score: f64, lower_score: f64 },
    OutputMismatch,
    /// A mode failed, timed out or reported no score.
    Invalid,
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Slower { upper, lower, upper_score, lower_score } => write!(
                f,
                "{upper} {upper_score:.1} < {lower} {lower_score:.1}"
            ),
            Self::OutputMismatch => f.write_str("output differs between modes"),
            Self::Invalid => f.write_str("a mode failed or scored nothing"),
        }
    }
}

/// The violations of one fixture. `scores` holds the measured modes only; a
/// relation that names an unmeasured mode does not apply.
pub(crate) fn violations(
    scores: &BTreeMap<&str, f64>,
    valid: bool,
    output_equal: bool,
    tolerance: f64,
) -> Vec<Violation> {
    let slower = RELATIONS.iter().filter_map(|&(upper, lower)| {
        let (upper_score, lower_score) = (*scores.get(upper)?, *scores.get(lower)?);
        (upper_score < lower_score * (1.0 - tolerance)).then(|| Violation::Slower {
            upper: upper.into(),
            lower: lower.into(),
            upper_score,
            lower_score,
        })
    });
    let invalid = (!valid).then_some(Violation::Invalid);
    let mismatch = (!output_equal).then_some(Violation::OutputMismatch);
    invalid.into_iter().chain(mismatch).chain(slower).collect()
}

#[derive(Serialize)]
struct FixtureReport {
    scores: BTreeMap<&'static str, Option<f64>>,
    max_rss_bytes: BTreeMap<&'static str, Option<u64>>,
    output_equal: bool,
    valid: bool,
    violations: Vec<Violation>,
}

#[derive(Serialize)]
struct Report {
    schema: u32,
    suite: Suite,
    created_unix_ns: u128,
    rounds: usize,
    tolerance: f64,
    modes: Vec<&'static str>,
    executable: String,
    host: super::HostRecord,
    fixtures: BTreeMap<String, FixtureReport>,
    violation_count: usize,
}

fn mode_engines(options: &Options) -> Vec<EngineSpec> {
    let mut engines: Vec<EngineSpec> = MODES
        .iter()
        .map(|mode| {
            let argv = mode.flag.map(String::from).into_iter().collect();
            spec(mode.engine, &options.quench, argv, BTreeMap::new())
        })
        .collect();
    let webassembly = super::probe_webassembly(&engines[0]);
    engines.iter_mut().for_each(|engine| engine.webassembly = webassembly);
    engines
}

fn fixture_report(
    config: &Config,
    summaries: &BTreeMap<String, super::EngineSummary>,
    valid: bool,
    output_equal: bool,
) -> FixtureReport {
    let per_mode = |read: fn(&super::EngineSummary) -> Option<f64>| {
        MODES
            .iter()
            .map(|mode| (mode.label, summaries.get(mode.engine).and_then(read)))
            .collect::<BTreeMap<_, _>>()
    };
    let scores = per_mode(|summary| summary.median_score);
    let max_rss_bytes = MODES
        .iter()
        .map(|mode| (mode.label, summaries.get(mode.engine).and_then(|s| s.median_max_rss_bytes)))
        .collect();
    let measured = scores
        .iter()
        .filter_map(|(label, score)| Some((*label, (*score)?)))
        .collect();
    let all_scored = scores.values().all(Option::is_some);
    FixtureReport {
        violations: violations(&measured, valid && all_scored, output_equal, config.tolerance),
        scores,
        max_rss_bytes,
        output_equal,
        valid,
    }
}

fn print_table(name: &str, report: &FixtureReport) {
    let cells = MODES.iter().map(|mode| {
        let score = report.scores[mode.label];
        let rss = report.max_rss_bytes[mode.label];
        format!(
            "{}={} ({} MiB)",
            mode.label,
            score.map_or_else(|| "-".into(), |score| format!("{score:.1}")),
            rss.map_or_else(|| "-".into(), |rss| (rss / BYTES_PER_MIB).to_string()),
        )
    });
    println!(
        "{name}: {} | output-equal={} | {}",
        cells.collect::<Vec<_>>().join(" "),
        report.output_equal,
        if report.violations.is_empty() { "ok" } else { "FAIL" }
    );
    for violation in &report.violations {
        println!("  violation: {violation}");
    }
}

/// The tier options and the suite options they run under.
fn parse(args: &[String]) -> (Config, Options) {
    let (config, rest) = Config::extract(args);
    let options = parse_options_from(rest);
    if options.fixed_work
        || options.preflight_only
        || options.checkpoint.is_some()
        || options.resume.is_some()
        || options.jit_mode != JitMode::Jit
    {
        usage("--tier-invariant selects its own modes; it cannot combine with --fixed-work, --preflight-only, --no-jit or checkpoints");
    }
    reject_exec_trace();
    (config, options)
}

/// Measures every selected fixture in every mode; the exit status is non-zero on any violation.
pub(crate) fn run(args: &[String]) -> i32 {
    let (config, options) = parse(args);
    let (config, options) = (&config, &options);
    let engines = mode_engines(options);
    let mut fixtures = BTreeMap::new();
    for file in selected_fixtures(options) {
        let record = run_fixture(options.suite, &file, &engines, options.rounds, options.timeout_ms);
        let report = fixture_report(config, &record.summaries, record.valid, record.output_equal);
        print_table(&fixture_name(&file), &report);
        fixtures.insert(fixture_name(&file), report);
    }
    let violation_count = fixtures.values().map(|fixture| fixture.violations.len()).sum();
    let report = Report {
        schema: REPORT_SCHEMA,
        suite: options.suite,
        created_unix_ns: now_ns(),
        rounds: options.rounds,
        tolerance: config.tolerance,
        modes: MODES.iter().map(|mode| mode.label).collect(),
        executable: engines[0].executable.display().to_string(),
        host: host_identity(),
        fixtures,
        violation_count,
    };
    if let Some(path) = &options.output {
        write_new_report(path, &serde_json::to_vec_pretty(&report).unwrap());
        println!("wrote {}", path.display());
    }
    println!("tier invariant: {violation_count} violation(s)");
    i32::from(violation_count > 0)
}

fn fixture_name(file: &Path) -> String {
    file.file_name().map_or_else(
        || fail("fixture has no file name"),
        |name| name.to_string_lossy().into_owned(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scores(entries: &[(&'static str, f64)]) -> BTreeMap<&'static str, f64> {
        entries.iter().copied().collect()
    }

    const ORDERED: &[(&str, f64)] = &[("default", 100.0), ("no-jit", 50.0)];

    #[test]
    fn ordered_scores_pass() {
        assert!(violations(&scores(ORDERED), true, true, DEFAULT_TOLERANCE).is_empty());
    }

    #[test]
    fn default_trailing_the_interpreter_beyond_tolerance_fails() {
        let found = violations(&scores(&[("default", 40.0), ("no-jit", 50.0)]), true, true, DEFAULT_TOLERANCE);
        assert!(matches!(&found[..], [Violation::Slower { upper, lower, .. }] if upper == "default" && lower == "no-jit"));
    }

    #[test]
    fn shortfall_within_tolerance_passes() {
        let within = scores(&[("default", 96.0), ("no-jit", 100.0)]);
        assert!(violations(&within, true, true, DEFAULT_TOLERANCE).is_empty());
        assert_eq!(violations(&within, true, true, 0.0).len(), 1);
    }

    #[test]
    fn output_mismatch_and_invalid_runs_fail() {
        let found = violations(&scores(ORDERED), false, false, DEFAULT_TOLERANCE);
        assert_eq!(found, [Violation::Invalid, Violation::OutputMismatch]);
    }

    #[test]
    fn own_flags_are_split_from_suite_options() {
        let args = ["--fixtures", "a,b", FLAG, TOLERANCE_FLAG, "0.1", "--runs", "2"].map(String::from);
        let (config, rest) = Config::extract(&args);
        assert!(config.tolerance == 0.1);
        assert_eq!(rest, ["--fixtures", "a,b", "--runs", "2"]);
    }

    #[test]
    fn tolerance_range() {
        assert!(Config::tolerance_is_valid(DEFAULT_TOLERANCE));
        assert!(!Config::tolerance_is_valid(-0.1));
        assert!(!Config::tolerance_is_valid(MAX_TOLERANCE));
    }
}
