//! Step `tier-order`: the modes are ordered by their score, interpreter < default < AOT, so in
//! instructions a faster mode never runs a program in more than the one under it: the default
//! (every function in its predicted-cheapest tier) no more than the interpreter, and AOT (the
//! `quench aot` build) no more than the default.
//! Each C-parity kernel, and every `.js` file in the directories `QUENCH_TIER_ORDER_CORPUS` names
//! (colon-separated, such as the V8-v7 files), runs cold in callgrind under each mode; AOT runs
//! the executable its compile makes of the program. A pair whose faster mode counts more than the
//! slack over the slower one is a violation, one `program higher>lower` line (the faster mode
//! first), held to `crates/quench-tier-corpus/tier_order.txt`: a violation the file does not list fails the step,
//! and a listed one that no longer occurs leaves it, so the list only shrinks. `quench-lint
//! --census` counts its lines. Alone: `quench-gate --only tier-order`. The same ladder per op is
//! `ir-budget`'s (`tier_inversions.txt`, `tier_inversions_aot.txt`); this step holds whole programs.

use std::{collections::{BTreeMap, BTreeSet}, env, ffi::OsString, fs, path::{Path, PathBuf}, sync::{atomic::{AtomicUsize, Ordering}, Mutex, PoisonError}, thread};

use crate::{build::NODE, config::Config, cparity, process, step::Verdict, tiers::{AOT, DEFAULT, INTERPRETER, launch}};

/// The violations, one `program higher>lower` per line, sorted.
pub const LIST: &str = "crates/quench-tier-corpus/tier_order.txt";
const EXTRA_CORPUS: &str = "QUENCH_TIER_ORDER_CORPUS";
const VALGRIND: &str = "valgrind";
/// Where each program's launch is built (AOT's executable): one directory per program.
const SCRATCH: &str = "tier-order";
/// The JIT tiers patch the code they emit (call links, stubs): valgrind must see each write, or it runs
/// a stale translation and the program crashes partway with a total that counts only that part.
const CALLGRIND: [&str; 4] = ["--tool=callgrind", "--fair-sched=yes", "--smc-check=all", "--callgrind-out-file=/dev/null"];
/// The line callgrind ends its report with, before the total.
const COLLECTED: &str = "Collected : ";
/// Cold runs, with no caches.
const ENVIRONMENT: [(&str, &str); 2] = [("QUENCH_COMPILE_CACHE", ""), ("QUENCH_STARTUP_SNAPSHOT", "off")];
/// Each mode by its name in the list and by `tiers` (the interpreter's flag, `default`, `aot`).
const MODES: [(&str, &str); 3] = [("no-jit", INTERPRETER), ("default", DEFAULT), ("aot", AOT)];
/// The pairs held in order: the faster mode (first) never counts more than the slower one (second).
const ORDER: [(&str, &str); 2] = [("default", "no-jit"), ("aot", "default")];
/// The share of the lower count under which the higher one is the same count (about 0.3%).
const SAME_COUNT_DIVISOR: u64 = 333;
const HEADER: &str = "# Programs a higher tier runs in more instructions than the tier under it (quench-gate step `tier-order`).\n\
# One `program higher>lower` per line. The list may only shrink; quench-lint --census counts it.\n";

fn corpus(config: &Config) -> Result<Vec<PathBuf>, String> {
    let mut files = cparity::kernels(&config.root.join(cparity::KERNELS)).map_err(|error| format!("{}: {error}", cparity::KERNELS))?;
    for directory in env::var_os(EXTRA_CORPUS).map(|extra| env::split_paths(&extra).collect::<Vec<_>>()).unwrap_or_default() {
        let entries = fs::read_dir(&directory).map_err(|error| format!("{}: {error}", directory.display()))?;
        files.extend(entries.flatten().map(|entry| entry.path()).filter(|path| path.extension().is_some_and(|extension| extension == "js")));
    }
    files.sort();
    Ok(files)
}

/// The total Ir of the run `argv` (a program's launch in one mode) under callgrind.
fn instructions(config: &Config, argv: &[OsString], program: &Path, mode: &str) -> Result<u64, String> {
    let mut command = config.command(VALGRIND);
    command.args(CALLGRIND).args(argv).envs(ENVIRONMENT);
    let run = process::run(&mut command)?;
    if !run.success { return Err(format!("{} {mode}: failed under callgrind\n{}", program.display(), run.excerpt())); }
    run.stderr.lines().find_map(|line| line.split_once(COLLECTED)).and_then(|(_, total)| total.trim().parse().ok())
        .ok_or_else(|| format!("{} {mode}: callgrind reported no total\n{}", program.display(), run.excerpt()))
}

/// The total Ir of `program` in the mode `MODES[mode]`, its launch built in the program's own
/// scratch directory (`slot`), so an AOT compile never shares an output with another program.
fn measure(config: &Config, host: &Path, program: &Path, slot: usize, mode: usize) -> Result<u64, String> {
    let (name, flag) = MODES[mode];
    let scratch = config.target.join(SCRATCH).join(slot.to_string());
    fs::create_dir_all(&scratch).map_err(|error| format!("{}: {error}", scratch.display()))?;
    let argv = launch(config, host, flag, program, &scratch, &[])?;
    instructions(config, &argv, program, name)
}

/// The violations of `counts` (Ir by mode) for the program `name`.
pub fn violations(name: &str, counts: &BTreeMap<&str, u64>) -> Vec<String> {
    ORDER.iter().filter_map(|(higher, lower)| {
        let (high, low) = (*counts.get(higher)?, *counts.get(lower)?);
        (high > low + low / SAME_COUNT_DIVISOR).then(|| format!("{name} {higher}>{lower}"))
    }).collect()
}

fn listed(text: &str) -> BTreeSet<String> {
    text.lines().filter(|line| !line.is_empty() && !line.starts_with('#')).map(str::to_owned).collect()
}

pub fn run(config: &Config) -> Verdict {
    let host = config.binary(NODE)?;
    let programs = corpus(config)?;
    // Each (program, mode) run is its own callgrind process (AOT's compile first): `config.jobs` of them at once.
    let runs: Vec<(usize, usize)> = (0..programs.len()).flat_map(|program| (0..MODES.len()).map(move |mode| (program, mode))).collect();
    let next = AtomicUsize::new(0);
    let measured: Mutex<BTreeMap<(usize, usize), Result<u64, String>>> = Mutex::default();
    thread::scope(|scope| {
        for _ in 0..config.jobs.min(runs.len()) {
            scope.spawn(|| while let Some(&(program, mode)) = runs.get(next.fetch_add(1, Ordering::Relaxed)) {
                let count = measure(config, &host, &programs[program], program, mode);
                measured.lock().unwrap_or_else(PoisonError::into_inner).insert((program, mode), count);
            });
        }
    });
    let mut measured = measured.into_inner().unwrap_or_else(PoisonError::into_inner);
    let mut found = BTreeSet::new();
    let mut table = Vec::new();
    for (index, program) in programs.iter().enumerate() {
        let name = program.file_stem().and_then(|stem| stem.to_str()).unwrap_or_default().to_owned();
        let mut counts = BTreeMap::new();
        for (mode_index, (mode, _)) in MODES.iter().enumerate() {
            counts.insert(*mode, measured.remove(&(index, mode_index)).unwrap_or_else(|| Err(format!("{name}: not measured")))?);
        }
        table.push(format!("{name}: {}", MODES.iter().map(|(mode, _)| format!("{mode} {}", counts[mode])).collect::<Vec<_>>().join(", ")));
        found.extend(violations(&name, &counts));
    }
    let path = config.root.join(LIST);
    let recorded = listed(&fs::read_to_string(&path).unwrap_or_default());
    let new: Vec<&String> = found.difference(&recorded).collect();
    let gone = recorded.difference(&found).count();
    // A program outside the corpus this run measured keeps its line: the list shrinks by what the run saw.
    let kept: BTreeSet<&String> = recorded.iter().filter(|line| found.contains(*line) || !table.iter().any(|row| row.starts_with(&format!("{}:", line.split(' ').next().unwrap_or_default())))).collect();
    if new.is_empty() || config.record {
        let lines: BTreeSet<&String> = if config.record { found.iter().chain(kept.iter().copied()).collect() } else { kept };
        let text = format!("{HEADER}{}", lines.iter().map(|line| format!("{line}\n")).collect::<String>());
        fs::write(&path, text).map_err(|error| format!("{}: {error}", path.display()))?;
    }
    if new.is_empty() || config.record {
        Ok(format!("{} programs in tier order but the listed {}; {gone} fixed and removed\n{}", table.len(), found.len(), table.join("\n")))
    } else {
        Err(format!("{} new tier-order violations (fix them; `--record` lists them only by the owner's decision):\n{}\n{}",
            new.len(), new.iter().map(|line| format!("  {line}")).collect::<Vec<_>>().join("\n"), table.join("\n")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_faster_mode_over_the_slower_one_is_a_violation() {
        let counts = BTreeMap::from([("no-jit", 1000), ("default", 1100), ("aot", 1000)]);
        assert_eq!(violations("k", &counts), vec!["k default>no-jit".to_owned()]);
        let close = BTreeMap::from([("no-jit", 1000), ("default", 1003)]);
        assert!(violations("k", &close).is_empty());
    }

    #[test]
    fn aot_counts_no_more_than_the_default() {
        let counts = BTreeMap::from([("no-jit", 1000), ("default", 900), ("aot", 910)]);
        assert_eq!(violations("k", &counts), vec!["k aot>default".to_owned()]);
        let in_order = BTreeMap::from([("no-jit", 1000), ("default", 900), ("aot", 600)]);
        assert!(violations("k", &in_order).is_empty());
    }
}
