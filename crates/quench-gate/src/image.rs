//! Step `image`: what the linked executable `quench compile ENTRY -o BIN` costs, beside the C
//! analogue. It runs only when named (`quench-gate --only image`, see `ON_REQUEST` in `main.rs`),
//! never in a full or `--quick` run.
//!
//! For the empty script, `console.log("hi")`, and the V8-v7 richards, deltablue and raytrace
//! fixtures it records, per executable: the bytes of the file and of `.text`, `.rodata`, `.data`
//! and `.bss` (`size -A`); the startup instructions of the two startup cases (callgrind's total
//! for the whole run); and the peak resident set in kilobytes beyond the C empty program's
//! (`rss_delta_kb`). The C column comes from `tools/image-vs-c.sh`, the same measurements of the
//! C analogue built with `gcc -O2 -static`. Wall time (best of `RUNS`) is reported and not held:
//! it moves with the machine.
//!
//! Held to `crates/quench-tier-corpus/image_budgets.txt`, one row per `<case> <metric> <ceiling>`,
//! and shrink-only: a measurement above its ceiling fails, and a ceiling above its measurement
//! fails too, so the ceilings track the image down. A run with `QUENCH_DEBT_WRITE=1` rewrites the
//! file: each ceiling becomes the lower of its old value and the measurement, and a row with no
//! ceiling takes its measurement; an over-budget measurement fails and writes nothing.

use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Instant,
};

use crate::{
    build::NODE,
    config::{Config, PROFILE},
    process,
    startup::{self, MEASURE_RSS},
    step::Verdict,
};

const BUDGETS: &str = "crates/quench-tier-corpus/image_budgets.txt";
const SCRIPT: &str = "tools/image-vs-c.sh";
/// The scratch directory under the cargo target directory: compiled executables, callgrind output.
const SCRATCH: &str = "image";
const DEBT_WRITE: &str = "QUENCH_DEBT_WRITE";
/// Runs whose best wall time is reported.
const RUNS: usize = 5;
/// The section names `size -A` reports, which are the metrics `text`, `rodata`, `data`, `bss`.
const SECTIONS: [&str; 4] = [".text", ".rodata", ".data", ".bss"];
/// The cases: the program's name, and the C analogue it is compared with.
const CASES: [(&str, &str); 5] = [
    ("empty", "empty"),
    ("hello", "empty"),
    ("v8-richards", "richards"),
    ("v8-deltablue", "deltablue"),
    ("v8-raytrace", "raytrace"),
];
/// The cases that are pure startup, whose instructions are ratcheted.
const STARTUP: [&str; 2] = ["empty", "hello"];
/// The C analogue whose peak resident set is the baseline of every `rss_delta_kb`.
const BASELINE: &str = "empty";

const BYTES: &str = "bytes";
const IR: &str = "ir";
const RSS_DELTA: &str = "rss_delta_kb";

/// One measurement's name: case, metric.
type Key = (String, String);
type Measures = BTreeMap<Key, u64>;

fn key(case: &str, metric: &str) -> Key {
    (case.to_owned(), metric.to_owned())
}

fn label(key: &Key) -> String {
    format!("{} {}", key.0, key.1)
}

/// The budget file's rows, skipping comments and blank lines.
fn parse_budgets(text: &str) -> Result<Measures, String> {
    let mut rows = Measures::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        let &[case, metric, ceiling] = fields.as_slice() else {
            return Err(format!("{BUDGETS}:{}: expected `case metric ceiling`, found `{line}`", index + 1));
        };
        let ceiling: u64 = ceiling.parse().map_err(|_| format!("{BUDGETS}:{}: ceiling `{ceiling}` is not a whole number", index + 1))?;
        if rows.insert(key(case, metric), ceiling).is_some() {
            return Err(format!("{BUDGETS}:{}: `{line}` repeats a row", index + 1));
        }
    }
    Ok(rows)
}

/// The header of the budget file: its leading comment lines, kept verbatim on a rewrite.
fn header(text: &str) -> String {
    text.lines().take_while(|line| line.starts_with('#')).map(|line| format!("{line}\n")).collect()
}

fn render(header: &str, rows: &Measures) -> String {
    let body: String = rows.iter().map(|(key, ceiling)| format!("{} {} {ceiling}\n", key.0, key.1)).collect();
    format!("{header}{body}")
}

/// Where the measurements stand against their ceilings.
#[derive(Debug, PartialEq)]
struct Settled {
    /// Why the step fails; empty when it passes.
    failures: Vec<String>,
    /// The ceilings the file holds after this run, were it written.
    next: Measures,
}

/// Holds `measured` to `ceilings`, shrink-only. Without `write`, every measurement that is not
/// exactly its ceiling fails, every row with no measurement fails, and every measurement with no
/// ceiling fails. With `write`, a measurement above its ceiling fails; each ceiling becomes the
/// lower of its old value and the measurement, and stale rows are dropped.
fn settle(measured: &Measures, ceilings: &Measures, write: bool) -> Settled {
    let mut failures = Vec::new();
    let mut next = Measures::new();
    for (key, &value) in measured {
        let ceiling = ceilings.get(key).copied();
        next.insert(key.clone(), ceiling.map_or(value, |ceiling| ceiling.min(value)));
        let name = label(key);
        match ceiling {
            Some(ceiling) if value > ceiling => failures.push(format!("{name}: {value} is over its ceiling {ceiling}")),
            Some(ceiling) if value < ceiling && !write => failures.push(format!("{name}: {value} is under its ceiling {ceiling}; lower the ceiling to {value}")),
            None if !write => failures.push(format!("{name}: {value} with no ceiling in {BUDGETS} (record it with {DEBT_WRITE}=1)")),
            _ => {}
        }
    }
    if !write {
        for key in ceilings.keys().filter(|key| !measured.contains_key(*key)) {
            failures.push(format!("{}: not measured by this step; remove it from {BUDGETS}", label(key)));
        }
    }
    Settled { failures, next }
}

/// The section sizes of `binary` in bytes, by the names of `SECTIONS` without their dot.
fn sections(config: &Config, binary: &Path) -> Result<BTreeMap<String, u64>, String> {
    let mut command = config.command("size");
    command.arg("-A").arg(binary);
    let listing = process::run(&mut command)?;
    if !listing.success {
        return Err(listing.excerpt());
    }
    let mut sizes = BTreeMap::new();
    for line in listing.stdout.lines() {
        if let [name, size, ..] = line.split_whitespace().collect::<Vec<_>>()[..]
            && SECTIONS.contains(&name)
            && let Ok(size) = size.parse::<u64>()
        {
            sizes.insert(name[1..].to_owned(), size);
        }
    }
    Ok(sizes)
}

/// The instructions of one whole run of `binary`, under callgrind.
fn instructions(config: &Config, binary: &Path, scratch: &Path) -> Result<u64, String> {
    let out = scratch.join("callgrind.out");
    let mut command = config.command("valgrind");
    command.arg("--tool=callgrind").arg(format!("--callgrind-out-file={}", out.display())).arg(binary);
    let run = process::run(&mut command)?;
    if !run.success {
        return Err(format!("{} failed under callgrind\n{}", binary.display(), run.excerpt()));
    }
    let text = fs::read_to_string(&out).map_err(|error| format!("{}: {error}", out.display()))?;
    let _ = fs::remove_file(&out);
    startup::parse_totals(&text).ok_or_else(|| format!("{}: no totals line", out.display()))
}

/// The peak resident set of one run of `binary`, in kilobytes: a fresh copy of this gate spawns
/// it (`startup::measure_child`), so the gate's own high-water mark does not count.
fn peak_rss_kb(config: &Config, binary: &Path, scratch: &Path) -> Result<u64, String> {
    let gate = env::current_exe().map_err(|error| format!("current_exe: {error}"))?;
    let out = scratch.join("peak_rss_kb");
    let status = Command::new(gate)
        .current_dir(&config.root)
        .arg(MEASURE_RSS)
        .arg(&out)
        .arg(binary)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|error| format!("cannot run {}: {error}", binary.display()))?;
    if !status.success() {
        return Err(format!("{} exited with {status}", binary.display()));
    }
    let text = fs::read_to_string(&out).map_err(|error| format!("{}: {error}", out.display()))?;
    text.trim().parse().map_err(|error| format!("{}: {error}", out.display()))
}

/// The best wall time of `RUNS` runs of `binary`, in microseconds.
fn best_wall_us(config: &Config, binary: &Path) -> Result<u64, String> {
    let mut best = u64::MAX;
    for _ in 0..RUNS {
        let started = Instant::now();
        let run = process::run(&mut config.command(binary))?;
        if !run.success {
            return Err(format!("{} failed\n{}", binary.display(), run.excerpt()));
        }
        best = best.min(started.elapsed().as_micros() as u64);
    }
    Ok(best)
}

/// The C analogue `name` by `tools/image-vs-c.sh`: its metrics as the script prints them, by name.
fn c_measures(config: &Config, name: &str, scratch: &Path) -> Result<BTreeMap<String, u64>, String> {
    let mut command = config.command("bash");
    command.arg(config.root.join(SCRIPT)).arg(name).arg(scratch.join("c").join(name));
    let run = process::run(&mut command)?;
    if !run.success {
        return Err(format!("{SCRIPT} {name} failed\n{}", run.excerpt()));
    }
    run.stdout
        .lines()
        .map(|line| -> Result<(String, u64), String> {
            let mut fields = line.split_whitespace();
            let metric = fields.next().ok_or_else(|| format!("{SCRIPT} {name}: empty line"))?;
            let value = fields.next().and_then(|value| value.parse().ok()).ok_or_else(|| format!("{SCRIPT} {name}: `{line}` is not `metric value`"))?;
            Ok((metric.to_owned(), value))
        })
        .collect()
}

/// The C analogue's measurements under the quench metric names, for one case: `rss_delta_kb` is
/// its peak beyond the baseline's.
fn c_column(case: &str, c: &BTreeMap<String, u64>, baseline_rss: u64) -> Measures {
    let mut column = Measures::new();
    for (metric, value) in c {
        match metric.as_str() {
            "rss_kb" => column.insert(key(case, RSS_DELTA), value.saturating_sub(baseline_rss)),
            "ir" => column.insert(key(case, IR), *value),
            "bytes" => column.insert(key(case, BYTES), *value),
            section => column.insert(key(case, section), *value),
        };
    }
    column
}

pub fn run(config: &Config) -> Verdict {
    let compiler = config.aot_binary(NODE)?;
    let scratch = config.target.join(SCRATCH);
    let _ = fs::remove_dir_all(&scratch);
    let binaries = scratch.join("bin");
    fs::create_dir_all(&binaries).map_err(|error| format!("{}: {error}", binaries.display()))?;

    let mut scripts: BTreeMap<String, PathBuf> = startup::write_scripts(&scratch)?.into_iter().map(|(name, path)| (name.to_owned(), path)).collect();
    scripts.extend(startup::write_v8_scripts(&config.root, &scratch)?);

    let baseline_rss = c_measures(config, BASELINE, &scratch)?.get("rss_kb").copied().ok_or("the C baseline reports no rss_kb")?;
    let mut measured = Measures::new();
    let mut column = Measures::new();
    let mut walls: Vec<(&str, u64)> = Vec::new();
    for (case, analogue) in CASES {
        let script = scripts.get(case).ok_or_else(|| format!("no script for `{case}`: git submodule update --init quench-bench/js-engine-benchmark"))?;
        let binary = binaries.join(case);
        let mut command = config.command(&compiler);
        command.arg("compile").arg(script).arg("-o").arg(&binary).args(["--profile", PROFILE]);
        let compiled = process::run(&mut command)?;
        if !compiled.success {
            return Err(format!("{case}: `quench compile` failed\n{}", compiled.excerpt()));
        }

        let sizes = sections(config, &binary)?;
        let bytes = fs::metadata(&binary).map_err(|error| format!("{}: {error}", binary.display()))?.len();
        measured.insert(key(case, BYTES), bytes);
        for (section, size) in sizes {
            measured.insert(key(case, &section), size);
        }
        if STARTUP.contains(&case) {
            measured.insert(key(case, IR), instructions(config, &binary, &scratch)?);
        }
        let rss = peak_rss_kb(config, &binary, &scratch)?;
        measured.insert(key(case, RSS_DELTA), rss.saturating_sub(baseline_rss));
        walls.push((case, best_wall_us(config, &binary)?));

        column.extend(c_column(case, &c_measures(config, analogue, &scratch)?, baseline_rss));
    }

    let path = config.root.join(BUDGETS);
    let before = fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    let ceilings = parse_budgets(&before)?;
    let write = env::var(DEBT_WRITE).as_deref() == Ok("1");
    let settled = settle(&measured, &ceilings, write);
    let table = table(&measured, &ceilings, &column, &walls);
    let _ = fs::remove_dir_all(&scratch);
    if !settled.failures.is_empty() {
        return Err(format!("{}\n\n{table}", settled.failures.join("\n")));
    }
    if write {
        let after = render(&header(&before), &settled.next);
        if after != before {
            fs::write(&path, after).map_err(|error| format!("{}: {error}", path.display()))?;
        }
        return Ok(format!("{} image ceilings recorded in {BUDGETS}\n{table}", settled.next.len()));
    }
    Ok(format!("{} image measurements within their ceilings\n{table}", measured.len()))
}

/// The table: each measurement beside its ceiling and the C analogue, then the wall times.
fn table(measured: &Measures, ceilings: &Measures, column: &Measures, walls: &[(&str, u64)]) -> String {
    let mut lines = vec![format!("{:<14}{:<14}{:>14}{:>14}{:>14}", "case", "metric", "measured", "ceiling", "C")];
    for (key, value) in measured {
        let ceiling = ceilings.get(key).map_or_else(|| "-".to_owned(), u64::to_string);
        let c = column.get(key).map_or_else(|| "-".to_owned(), u64::to_string);
        lines.push(format!("{:<14}{:<14}{value:>14}{ceiling:>14}{c:>14}", key.0, key.1));
    }
    lines.push("wall time, best of runs (reported, not held):".to_owned());
    lines.extend(walls.iter().map(|(case, us)| format!("  {case:<14}{us:>10} us")));
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(entries: &[(&str, &str, u64)]) -> Measures {
        entries.iter().map(|&(case, metric, value)| (key(case, metric), value)).collect()
    }

    #[test]
    fn budget_rows_parse_and_render_back() {
        let text = "# header line\n\nempty bytes 1000\nhello ir 2048\n";
        let parsed = parse_budgets(text).unwrap();
        assert_eq!(parsed, rows(&[("empty", "bytes", 1000), ("hello", "ir", 2048)]));
        assert_eq!(render(&header(text), &parsed), "# header line\nempty bytes 1000\nhello ir 2048\n");
    }

    #[test]
    fn malformed_budget_rows_are_errors() {
        assert!(parse_budgets("empty bytes\n").is_err());
        assert!(parse_budgets("empty bytes many\n").is_err());
        assert!(parse_budgets("empty bytes 1\nempty bytes 2\n").is_err());
    }

    #[test]
    fn without_write_every_difference_from_a_ceiling_fails() {
        let measured = rows(&[("empty", "bytes", 900), ("hello", "bytes", 5), ("empty", "text", 1100)]);
        let ceilings = rows(&[("empty", "bytes", 1000), ("empty", "text", 1000), ("gone", "bytes", 7)]);
        let settled = settle(&measured, &ceilings, false);
        assert_eq!(settled.failures.len(), 4);
        assert!(settled.failures.iter().any(|line| line.contains("empty bytes") && line.contains("lower the ceiling to 900")));
        assert!(settled.failures.iter().any(|line| line.contains("empty text") && line.contains("over its ceiling")));
        assert!(settled.failures.iter().any(|line| line.contains("hello bytes") && line.contains("no ceiling")));
        assert!(settled.failures.iter().any(|line| line.contains("gone bytes") && line.contains("remove it")));
    }

    #[test]
    fn with_write_the_ceilings_only_shrink() {
        let measured = rows(&[("empty", "bytes", 900), ("hello", "bytes", 1010), ("module", "bytes", 5)]);
        let ceilings = rows(&[("empty", "bytes", 1000), ("hello", "bytes", 1000), ("gone", "bytes", 7)]);
        let settled = settle(&measured, &ceilings, true);
        assert_eq!(settled.failures.len(), 1);
        assert!(settled.failures[0].starts_with("hello bytes"));
        assert_eq!(settled.next, rows(&[("empty", "bytes", 900), ("hello", "bytes", 1000), ("module", "bytes", 5)]));
    }

    #[test]
    fn the_c_column_is_relative_to_the_baseline_rss() {
        let c = BTreeMap::from([("bytes".to_owned(), 1600), ("text".to_owned(), 900), ("rss_kb".to_owned(), 3000)]);
        let column = c_column("hello", &c, 2900);
        assert_eq!(column[&key("hello", BYTES)], 1600);
        assert_eq!(column[&key("hello", "text")], 900);
        assert_eq!(column[&key("hello", RSS_DELTA)], 100);
    }
}
