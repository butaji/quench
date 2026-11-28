//! Step `tier-order`: a higher tier never runs a program in more instructions than the tier under it
//! (interpreter > b-jit > u-jit, and the default, which tiers b-jit up to u-jit, no more than b-jit).
//! Each C-parity kernel, and every `.js` file in the directories `QUENCH_TIER_ORDER_CORPUS` names
//! (colon-separated, such as the V8-v7 files), runs cold in callgrind under each mode. A pair whose higher tier counts
//! more than the slack over the lower one is a violation, one `program higher>lower` line, held to
//! `crates/quench-tier-corpus/tier_order.txt`: a violation the file does not list fails the step,
//! and a listed one that no longer occurs leaves it, so the list only shrinks. `quench-lint
//! --census` counts its lines. Alone: `quench-gate --only tier-order`. The same ladder per op is
//! `ir-budget`'s (`tier_inversions.txt`, `tier_inversions_default.txt`); this step holds whole programs.

use std::{collections::{BTreeMap, BTreeSet}, env, fs, path::{Path, PathBuf}, sync::{atomic::{AtomicUsize, Ordering}, Mutex, PoisonError}, thread};

use crate::{build::NODE, config::Config, cparity, process, step::Verdict, tiers::INTERPRETER};

/// The violations, one `program higher>lower` per line, sorted.
pub const LIST: &str = "crates/quench-tier-corpus/tier_order.txt";
const EXTRA_CORPUS: &str = "QUENCH_TIER_ORDER_CORPUS";
const VALGRIND: &str = "valgrind";
/// The JIT tiers patch the code they emit (call links, stubs): valgrind must see each write, or it runs
/// a stale translation and the program crashes partway with a total that counts only that part.
const CALLGRIND: [&str; 4] = ["--tool=callgrind", "--fair-sched=yes", "--smc-check=all", "--callgrind-out-file=/dev/null"];
/// The line callgrind ends its report with, before the total.
const COLLECTED: &str = "Collected : ";
/// Cold runs, with no caches.
const ENVIRONMENT: [(&str, &str); 2] = [("QUENCH_COMPILE_CACHE", ""), ("QUENCH_STARTUP_SNAPSHOT", "off")];
/// Each mode by name and runner flag (none for the default).
const MODES: [(&str, Option<&str>); 4] = [("no-jit", Some(INTERPRETER)), ("b-jit", Some("--b-jit")), ("u-jit", Some("--u-jit")), ("default", None)];
/// The pairs held in order: the first never counts more than the second.
const ORDER: [(&str, &str); 3] = [("b-jit", "no-jit"), ("u-jit", "b-jit"), ("default", "b-jit")];
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

/// The total Ir of `program` run in the mode `flag`.
fn instructions(config: &Config, host: &Path, flag: Option<&str>, program: &Path) -> Result<u64, String> {
    let mut command = config.command(VALGRIND);
    command.args(CALLGRIND).arg(host).args(flag).arg(program).envs(ENVIRONMENT);
    let run = process::run(&mut command)?;
    if !run.success { return Err(format!("{} {}: failed under callgrind\n{}", program.display(), flag.unwrap_or("default"), run.excerpt())); }
    run.stderr.lines().find_map(|line| line.split_once(COLLECTED)).and_then(|(_, total)| total.trim().parse().ok())
        .ok_or_else(|| format!("{} {}: callgrind reported no total\n{}", program.display(), flag.unwrap_or("default"), run.excerpt()))
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
    // Each (program, mode) run is its own callgrind process: `config.jobs` of them at once.
    let runs: Vec<(usize, usize)> = (0..programs.len()).flat_map(|program| (0..MODES.len()).map(move |mode| (program, mode))).collect();
    let next = AtomicUsize::new(0);
    let measured: Mutex<BTreeMap<(usize, usize), Result<u64, String>>> = Mutex::default();
    thread::scope(|scope| {
        for _ in 0..config.jobs.min(runs.len()) {
            scope.spawn(|| while let Some(&(program, mode)) = runs.get(next.fetch_add(1, Ordering::Relaxed)) {
                let count = instructions(config, &host, MODES[mode].1, &programs[program]);
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
    fn a_higher_tier_over_the_lower_one_is_a_violation() {
        let counts = BTreeMap::from([("no-jit", 1000), ("b-jit", 1100), ("u-jit", 1000), ("default", 1101)]);
        assert_eq!(violations("k", &counts), vec!["k b-jit>no-jit".to_owned()]);
        let close = BTreeMap::from([("no-jit", 1000), ("b-jit", 1003)]);
        assert!(violations("k", &close).is_empty());
    }
}
