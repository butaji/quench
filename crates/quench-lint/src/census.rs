//! The JIT completeness census: counts, taken from the source tree, of what stands between the
//! tiers and "every opcode is the interpreter specialized". Each metric moves toward done in one
//! direction; `census.txt` holds the current values and no metric may move away from done.
//!
//! usage: quench-lint --census [--update-census]

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

use crate::census_metrics as extract;

const CENSUS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/census.txt");
pub const CENSUS_FLAG: &str = "--census";
const UPDATE_FLAG: &str = "--update-census";

/// Which way a metric moves as the JIT nears completeness.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Direction {
    Informational,
    Up,
    Down,
}

/// Where a metric's counts are read.
enum Source {
    /// A file that must exist.
    File(&'static str),
    /// A file counted when present.
    Optional(&'static str),
    /// Every Rust file under a directory whose name contains one of the fragments (any when empty).
    Tree(&'static str, &'static [&'static str]),
}

/// How a source file yields its count.
#[derive(Clone, Copy)]
enum Extract {
    Syntax(fn(&syn::File) -> usize),
    Text(fn(&str) -> usize),
}

struct Metric {
    name: &'static str,
    direction: Direction,
    sources: &'static [Source],
    extract: Extract,
}

const COMPILED: &str = "crates/quench-runtime/src/vm/compiled";
const LOWERING_FRAGMENTS: &[&str] = &["lower", "region"];

const METRICS: &[Metric] = &[
    Metric {
        name: "opcodes",
        direction: Direction::Informational,
        sources: &[Source::File("crates/quench-runtime/src/bytecode.rs")],
        extract: Extract::Syntax(extract::opcode_variants),
    },
    Metric {
        name: "opcodes_with_staged_operation",
        direction: Direction::Up,
        sources: &[Source::File("crates/quench-runtime/src/staged.rs")],
        extract: Extract::Syntax(extract::staged_opcodes),
    },
    Metric {
        name: "hand_semantics_lines",
        direction: Direction::Down,
        sources: &[Source::File("crates/quench-runtime/src/staged/hand_semantics.txt")],
        extract: Extract::Text(extract::text_lines),
    },
    Metric {
        name: "tier_opcode_arms",
        direction: Direction::Down,
        sources: &[
            Source::Optional("crates/quench-runtime/src/vm/compiled/lane_region.rs"),
            Source::Optional("crates/quench-runtime/src/vm/compiled/lane_unit.rs"),
            Source::Optional("crates/quench-runtime/src/vm/compiled/osr_loop.rs"),
            Source::Optional("crates/quench-runtime/src/vm/compiled.rs"),
            Source::Optional("crates/quench-runtime/src/vm/js_compiled.rs"),
            Source::Tree(COMPILED, LOWERING_FRAGMENTS),
        ],
        extract: Extract::Syntax(extract::opcode_arms),
    },
    Metric {
        name: "frame_structs",
        direction: Direction::Down,
        sources: &[Source::Tree("crates/quench-runtime/src/vm", &[])],
        extract: Extract::Syntax(extract::frame_structs),
    },
    Metric {
        name: "exit_mechanisms",
        direction: Direction::Down,
        sources: &[Source::File("crates/quench-runtime/src/vm/compiled.rs"), Source::Tree(COMPILED, &[])],
        extract: Extract::Syntax(extract::exit_mechanisms),
    },
    Metric {
        name: "tier_policy_constants",
        direction: Direction::Down,
        sources: &[Source::File("crates/quench-runtime/src/vm/compiled/tier_policy.rs")],
        extract: Extract::Syntax(extract::tier_policy_literals),
    },
    Metric {
        name: "entry_kinds",
        direction: Direction::Down,
        sources: &[Source::File("crates/quench-runtime/src/vm/compiled.rs"), Source::Tree(COMPILED, &[])],
        extract: Extract::Syntax(extract::entry_uses),
    },
    Metric {
        name: "stencil_opcodes",
        direction: Direction::Up,
        sources: &[Source::File("crates/quench-runtime/src/vm/compiled/base_region/decode.rs")],
        extract: Extract::Syntax(extract::decoder_classes),
    },
];

/// One metric's total and its per-file counts (files that count zero are left out).
struct Measurement {
    total: usize,
    files: BTreeMap<String, usize>,
}

fn rust_files_under(directory: &Path, fragments: &[&str], found: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(directory).map_err(|error| format!("{}: {error}", directory.display()))?;
    for path in entries.flatten().map(|entry| entry.path()) {
        let stem = path.file_stem().and_then(|stem| stem.to_str()).unwrap_or_default();
        if path.is_dir() {
            rust_files_under(&path, fragments, found)?;
        } else if path.extension().is_some_and(|extension| extension == "rs")
            && (fragments.is_empty() || fragments.iter().any(|fragment| stem.contains(fragment)))
        {
            found.push(path);
        }
    }
    Ok(())
}

fn source_files(sources: &[Source]) -> Result<Vec<PathBuf>, String> {
    let mut found = Vec::new();
    for source in sources {
        match source {
            Source::File(path) => found.push(PathBuf::from(path)),
            Source::Optional(path) => found.extend(Some(PathBuf::from(path)).filter(|path| path.exists())),
            Source::Tree(directory, fragments) => rust_files_under(Path::new(directory), fragments, &mut found)?,
        }
    }
    found.sort();
    found.dedup();
    Ok(found)
}

fn count_file(path: &Path, how: Extract) -> Result<usize, String> {
    let name = path.display();
    let text = fs::read_to_string(path).map_err(|error| format!("{name}: {error}"))?;
    match how {
        Extract::Text(count) => Ok(count(&text)),
        Extract::Syntax(count) => syn::parse_file(&text)
            .map(|syntax| count(&syntax))
            .map_err(|error| format!("{name}:{}: {error}", error.span().start().line)),
    }
}

fn measure(metric: &Metric) -> Result<Measurement, String> {
    let mut files = BTreeMap::new();
    for path in source_files(metric.sources)? {
        let count = count_file(&path, metric.extract)?;
        if count > 0 {
            files.insert(path.display().to_string(), count);
        }
    }
    Ok(Measurement { total: files.values().sum(), files })
}

fn measure_all() -> Result<Vec<(&'static Metric, Measurement)>, String> {
    METRICS.iter().map(|metric| Ok((metric, measure(metric)?))).collect()
}

/// A metric's JSON value: its total, or its total with the per-file counts of a multi-file metric.
fn json_value(metric: &Metric, found: &Measurement) -> String {
    if metric.sources.len() == 1 && !matches!(metric.sources[0], Source::Tree(..)) {
        return found.total.to_string();
    }
    let files: Vec<String> = found.files.iter().map(|(file, count)| format!("\"{file}\":{count}")).collect();
    format!("{{\"total\":{},\"files\":{{{}}}}}", found.total, files.join(","))
}

fn render_json(measured: &[(&Metric, Measurement)]) -> String {
    let fields: Vec<String> =
        measured.iter().map(|(metric, found)| format!("\"{}\":{}", metric.name, json_value(metric, found))).collect();
    format!("{{{}}}", fields.join(","))
}

fn render_baseline(measured: &[(&Metric, Measurement)]) -> String {
    measured.iter().map(|(metric, found)| format!("{} {}\n", metric.name, found.total)).collect()
}

fn parse_baseline(text: &str) -> BTreeMap<String, usize> {
    text.lines()
        .filter_map(|line| {
            let (name, value) = line.split_once(' ')?;
            Some((name.to_string(), value.parse().ok()?))
        })
        .collect()
}

/// Whether `now` is farther from done than `recorded`.
fn regressed(direction: Direction, recorded: usize, now: usize) -> bool {
    match direction {
        Direction::Informational => false,
        Direction::Up => now < recorded,
        Direction::Down => now > recorded,
    }
}

/// The recorded metrics that moved away from done, as messages; a metric with no record is new.
fn regressions(recorded: &BTreeMap<String, usize>, measured: &[(&Metric, Measurement)]) -> Vec<String> {
    measured
        .iter()
        .filter_map(|(metric, found)| {
            let before = *recorded.get(metric.name)?;
            regressed(metric.direction, before, found.total)
                .then(|| format!("census: {} moved away from done: {before} -> {}", metric.name, found.total))
        })
        .collect()
}

pub fn run(args: &[String]) -> ExitCode {
    let measured = match measure_all() {
        Ok(measured) => measured,
        Err(error) => {
            eprintln!("quench-lint: census cannot read {error}");
            return ExitCode::FAILURE;
        }
    };
    println!("{}", render_json(&measured));
    let recorded = parse_baseline(&fs::read_to_string(CENSUS).unwrap_or_default());
    let failures = regressions(&recorded, &measured);
    failures.iter().for_each(|message| eprintln!("{message}"));
    if !failures.is_empty() {
        return ExitCode::FAILURE;
    }
    if args.iter().any(|arg| arg == UPDATE_FLAG)
        && let Err(error) = fs::write(CENSUS, render_baseline(&measured))
    {
        eprintln!("quench-lint: cannot write {CENSUS}: {error}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directions_decide_regression() {
        assert!(regressed(Direction::Up, 3, 2));
        assert!(!regressed(Direction::Up, 3, 4));
        assert!(regressed(Direction::Down, 3, 4));
        assert!(!regressed(Direction::Down, 3, 3));
        assert!(!regressed(Direction::Informational, 3, 9));
    }

    #[test]
    fn baseline_round_trips_and_names_the_metric() {
        let recorded = parse_baseline("frame_structs 2\nopcodes 10\n");
        let measured = [(&METRICS[4], Measurement { total: 3, files: BTreeMap::new() })];
        let failures = regressions(&recorded, &measured);
        assert_eq!(failures.len(), 1);
        assert!(failures[0].contains("frame_structs"));
        assert_eq!(parse_baseline(&render_baseline(&measured)).get("frame_structs"), Some(&3));
    }

    #[test]
    fn json_nests_per_file_counts_of_multi_file_metrics() {
        let files = BTreeMap::from([("a.rs".to_string(), 2)]);
        let found = Measurement { total: 2, files };
        assert_eq!(json_value(&METRICS[3], &found), r#"{"total":2,"files":{"a.rs":2}}"#);
        assert_eq!(json_value(&METRICS[0], &found), "2");
    }
}
