//! `--residuals`: how far each tier's hot loops are from the C a programmer would write
//! (execution law 4). Each kernel of `quench-tier-corpus` runs under each forced tier with
//! `QUENCH_TRACE=residuals`; the engine reports the counts of the loops it compiles (CLIF
//! instructions for u-jit, stencil rows for b-jit) and this mode compares the hottest loop's
//! counts with the kernel's spec (the target, reported) and the shrink-only ratchet (the gate:
//! only a count above its recorded entry fails).

use super::{fail, usage};
use quench_tier_corpus::residuals::{
    Counts, Kernel, Ratchet, Tier, CATEGORIES, CATEGORY_COUNT, KERNELS,
};
use std::{
    collections::HashMap,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

/// Selects this mode.
pub(crate) const FLAG: &str = "--residuals";
const QUENCH_FLAG: &str = "--quench";
const KERNELS_FLAG: &str = "--kernels";
const RATCHET_FLAG: &str = "--ratchet";
const UPDATE_FLAG: &str = "--update-ratchet";
const TIMEOUT_FLAG: &str = "--timeout-ms";

const DEFAULT_QUENCH: &str = "target/release/quench-node";
const DEFAULT_TIMEOUT_MS: u64 = 60_000;
const POLL_INTERVAL: Duration = Duration::from_millis(5);

/// The engine's report line (`vm/compiled/residuals.rs`) and its fields.
const LINE_PREFIX: &str = "QUENCH_RESIDUALS ";
const TIER_FIELD: &str = "tier";
const BYTES_FIELD: &str = "bytes";
/// Why a kernel has no row of counts when the engine compiled no loop of it.
const NO_LOOP: &str = "no compiled loop";
/// The category the hottest loop is chosen by.
const SIZE_CATEGORY: &str = "instructions";

struct Config {
    quench: PathBuf,
    kernels: Option<Vec<String>>,
    ratchet: PathBuf,
    update: bool,
    timeout: Duration,
}

impl Config {
    fn parse(args: &[String]) -> Self {
        let mut config = Self {
            quench: DEFAULT_QUENCH.into(),
            kernels: None,
            ratchet: Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../tests/fixtures/residuals/ratchet.txt"),
            update: false,
            timeout: Duration::from_millis(DEFAULT_TIMEOUT_MS),
        };
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                FLAG => {}
                UPDATE_FLAG => config.update = true,
                flag => config.set(
                    flag,
                    args.next()
                        .unwrap_or_else(|| usage(&format!("{flag} requires a value"))),
                ),
            }
        }
        config
    }

    /// Applies the option `flag` that takes `value`.
    fn set(&mut self, flag: &str, value: &str) {
        match flag {
            QUENCH_FLAG => self.quench = value.into(),
            RATCHET_FLAG => self.ratchet = value.into(),
            KERNELS_FLAG => self.kernels = Some(value.split(',').map(String::from).collect()),
            TIMEOUT_FLAG => {
                let millis = value
                    .parse()
                    .unwrap_or_else(|_| usage("--timeout-ms requires milliseconds"));
                self.timeout = Duration::from_millis(millis);
            }
            other => usage(&format!("--residuals does not take {other}")),
        }
    }

    fn selected(&self) -> impl Iterator<Item = &'static Kernel> + '_ {
        KERNELS.iter().filter(|kernel| {
            self.kernels
                .as_ref()
                .is_none_or(|names| names.iter().any(|name| name == kernel.name))
        })
    }
}

/// One loop's report.
struct Record {
    counts: Counts,
    bytes: Option<u64>,
}

fn parse_line(line: &str, tier: Tier) -> Option<Record> {
    let fields: HashMap<&str, &str> = line
        .strip_prefix(LINE_PREFIX)?
        .split_whitespace()
        .filter_map(|field| field.split_once('='))
        .collect();
    (*fields.get(TIER_FIELD)? == tier.name()).then_some(())?;
    let mut counts = [0; CATEGORY_COUNT];
    for (slot, name) in counts.iter_mut().zip(CATEGORIES) {
        *slot = fields.get(name)?.parse().ok()?;
    }
    Some(Record {
        counts: Counts(counts),
        bytes: fields.get(BYTES_FIELD).and_then(|bytes| bytes.parse().ok()),
    })
}

/// The hottest loop the engine reported for `tier`: the one with the most instructions.
fn hottest(report: &str, tier: Tier) -> Option<Record> {
    report
        .lines()
        .filter_map(|line| parse_line(line, tier))
        .max_by_key(|record| record.counts.get(SIZE_CATEGORY))
}

/// Runs `source` forced to `tier` and returns what the engine reported on stderr.
fn run_kernel(config: &Config, tier: Tier, source: &str) -> Result<String, String> {
    let mut child = Command::new(&config.quench)
        .arg(format!("--{}", tier.name()))
        .args(["-e", source])
        .env("QUENCH_TRACE", "residuals")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot run {}: {error}", config.quench.display()))?;
    let mut stderr = child.stderr.take().ok_or("no stderr")?;
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text);
        text
    });
    let deadline = Instant::now() + config.timeout;
    let status = loop {
        match child.try_wait().map_err(|error| error.to_string())? {
            Some(status) => break status,
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("timed out".into());
            }
            None => std::thread::sleep(POLL_INTERVAL),
        }
    };
    let report = reader.join().map_err(|_| "stderr reader panicked")?;
    status
        .success()
        .then_some(report)
        .ok_or_else(|| format!("exited with {status}"))
}

/// One kernel on one tier against its spec and recorded ratchet entry.
struct Row {
    kernel: &'static Kernel,
    tier: Tier,
    /// The hottest loop, or why there is none (no compiled loop, or the run failed).
    outcome: Result<Record, String>,
    over: Vec<&'static str>,
}

fn measure(config: &Config, ratchet: &Ratchet, kernel: &'static Kernel, tier: Tier) -> Row {
    let outcome = run_kernel(config, tier, kernel.source)
        .and_then(|report| hottest(&report, tier).ok_or_else(|| NO_LOOP.to_string()));
    let over = outcome.as_ref().map_or_else(
        |_| Vec::new(),
        |record| ratchet.regressions(kernel, tier, record.counts),
    );
    Row {
        kernel,
        tier,
        outcome,
        over,
    }
}

/// Whether the row breaks the judge: a loop above its recorded entry, a failed run, or a loop that
/// compiled before (a ratchet entry) and no longer does.
fn is_violation(row: &Row, ratchet: &Ratchet) -> bool {
    match &row.outcome {
        Ok(_) => !row.over.is_empty(),
        Err(reason) => reason != NO_LOOP || ratchet.has(row.kernel, row.tier),
    }
}

fn print_row(row: &Row, ratchet: &Ratchet) {
    let flag = if is_violation(row, ratchet) {
        "FAIL"
    } else {
        "ok"
    };
    let spec = row.kernel.expected(row.tier);
    let cells = match &row.outcome {
        Ok(record) => CATEGORIES
            .iter()
            .zip(record.counts.0.iter().zip(spec.0))
            .map(|(name, (have, want))| format!("{name}={have}({want})"))
            .collect::<Vec<_>>()
            .join(" "),
        Err(reason) => reason.clone(),
    };
    let bytes = row
        .outcome
        .as_ref()
        .ok()
        .and_then(|record| record.bytes)
        .map_or_else(String::new, |bytes| format!(" bytes={bytes}"));
    println!(
        "{:<18} {} {cells}{bytes} | {flag}",
        row.kernel.name,
        row.tier.name()
    );
    for category in &row.over {
        println!("  regressed past the ratchet: {category}");
    }
}

/// Measures the selected kernels on both tiers; the exit status is non-zero on any violation.
pub(crate) fn run(args: &[String]) -> i32 {
    let config = Config::parse(args);
    let text = std::fs::read_to_string(&config.ratchet).unwrap_or_default();
    let ratchet = Ratchet::parse(&text).unwrap_or_else(|error| fail(&error));
    let rows: Vec<Row> = config
        .selected()
        .flat_map(|kernel| Tier::ALL.map(|tier| measure(&config, &ratchet, kernel, tier)))
        .collect();
    rows.iter().for_each(|row| print_row(row, &ratchet));
    let violations = rows
        .iter()
        .filter(|row| is_violation(row, &ratchet))
        .count();
    if config.update {
        let measured = rows
            .iter()
            .filter_map(|row| Some((row.kernel.name, row.tier, row.outcome.as_ref().ok()?.counts)));
        let rendered = ratchet.tightened(measured).render();
        std::fs::write(&config.ratchet, rendered)
            .unwrap_or_else(|error| fail(&format!("cannot write ratchet: {error}")));
        println!("wrote {}", config.ratchet.display());
    }
    println!(
        "residuals: {violations} violation(s) over {} kernel-tier pair(s)",
        rows.len()
    );
    i32::from(violations > 0)
}
