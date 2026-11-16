//! Step: the interpreter's instruction count on the kernel fixtures may not grow. Each C-parity
//! kernel runs under `--no-jit` in callgrind, cold (no compile cache, no startup snapshot), and
//! its total Ir is held to `crates/quench-tier-corpus/no_jit_ir.txt`: growth past the tolerance
//! fails, a drop past it lowers the ceiling, and `--record` writes a kernel without one. A JIT
//! change that costs the interpreter (an out-of-line accessor, a per-read branch) shows here
//! before it lands (TOD-1764).

use std::{collections::BTreeMap, fs, path::Path};

use crate::{build::NODE, config::Config, cparity, process, step::Verdict, tiers::INTERPRETER};

const RATCHET: &str = "crates/quench-tier-corpus/no_jit_ir.txt";
const VALGRIND: &str = "valgrind";
const CALLGRIND: [&str; 2] = ["--tool=callgrind", "--callgrind-out-file=/dev/null"];
/// The line callgrind ends its report with, before the total.
const COLLECTED: &str = "Collected : ";
/// Cold runs: no on-disk compile cache, no startup snapshot.
const COLD: [(&str, &str); 2] = [("QUENCH_COMPILE_CACHE", ""), ("QUENCH_STARTUP_SNAPSHOT", "off")];
/// The share of a ceiling under which a change is the same count (about 0.3%): callgrind
/// repeats to about 0.01%.
const SAME_COUNT_DIVISOR: u64 = 333;

/// The total Ir of `kernel` run by the interpreter.
fn instructions(config: &Config, host: &Path, kernel: &Path) -> Result<u64, String> {
    let mut command = config.command(VALGRIND);
    command.args(CALLGRIND).arg(host).arg(INTERPRETER).arg(kernel).envs(COLD);
    let run = process::run(&mut command)?;
    run.stderr.lines().find_map(|line| line.split_once(COLLECTED)).and_then(|(_, total)| total.trim().parse().ok())
        .ok_or_else(|| format!("{}: callgrind reported no total\n{}", kernel.display(), run.excerpt()))
}

fn parse(text: &str) -> BTreeMap<String, u64> {
    text.lines().filter_map(|line| line.split_once(' ')).filter_map(|(name, count)| Some((name.to_owned(), count.trim().parse().ok()?))).collect()
}

pub fn run(config: &Config) -> Verdict {
    let host = config.binary(NODE)?;
    let path = config.root.join(RATCHET);
    let before = fs::read_to_string(&path).unwrap_or_default();
    let mut ceilings = parse(&before);
    let kernels = cparity::kernels(&config.root.join(cparity::KERNELS)).map_err(|error| format!("{}: {error}", cparity::KERNELS))?;
    let (mut grown, mut lowered) = (Vec::new(), 0);
    for kernel in &kernels {
        let name = kernel.file_stem().and_then(|stem| stem.to_str()).unwrap_or_default().to_owned();
        let count = instructions(config, &host, kernel)?;
        let slack = |ceiling: u64| ceiling / SAME_COUNT_DIVISOR;
        match ceilings.get(&name).copied() {
            Some(ceiling) if count > ceiling + slack(ceiling) => grown.push(format!("{name}: {count} Ir, ceiling {ceiling}")),
            Some(ceiling) if count + slack(ceiling) < ceiling => { ceilings.insert(name, count); lowered += 1; }
            Some(_) => {}
            None if config.record => { ceilings.insert(name, count); }
            None => grown.push(format!("{name}: {count} Ir, no ceiling recorded (--record)")),
        }
    }
    let after: String = ceilings.iter().map(|(name, count)| format!("{name} {count}\n")).collect();
    if after != before {
        fs::write(&path, &after).map_err(|error| format!("{}: {error}", path.display()))?;
    }
    if grown.is_empty() { Ok(format!("{} kernels within their no-JIT Ir ceilings; {lowered} lowered", kernels.len())) } else { Err(grown.join("\n")) }
}
