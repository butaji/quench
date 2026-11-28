//! `--residuals`: how far each mode's hot loops are from the C a programmer would write
//! (execution law 4). Each kernel of `quench-tier-corpus` runs in each mode (`--no-jit`, the
//! default, and the AOT executable compiled from the kernel) with `QUENCH_TRACE=residuals`; the
//! engine reports the counts of the loops it compiles (CLIF instructions for the Cranelift tier,
//! stencil rows for the b-jit tier) and this mode compares the hottest loop's counts with the
//! kernel's spec (the target, reported) and the shrink-only ratchet (the gate: only a count above
//! its recorded entry fails).

use super::{fail, usage, TIER_RUN_TIMEOUT_MS};
use quench_tier_corpus::residuals::{
    Counts, Kernel, Mode, Ratchet, CATEGORIES, CATEGORY_COUNT, KERNELS,
};
use std::{
    collections::HashMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

/// Selects this mode.
pub(crate) const FLAG: &str = "--residuals";
const QUENCH_FLAG: &str = "--quench";
const AOT_QUENCH_FLAG: &str = "--aot-quench";
const KERNELS_FLAG: &str = "--kernels";
const RATCHET_FLAG: &str = "--ratchet";
const UPDATE_FLAG: &str = "--update-ratchet";
const TIMEOUT_FLAG: &str = "--timeout-ms";

const DEFAULT_QUENCH: &str = "target/release/quench";
/// The AOT-enabled `quench` (`cargo build --features aot --target-dir target/aot`), which compiles the
/// kernels for the AOT mode.
const DEFAULT_AOT_QUENCH: &str = "target/aot/release/quench";
/// The interpreter's flag, and the eval flag that takes a kernel's source.
const INTERPRETER_FLAG: &str = "--no-jit";
const EVAL_FLAG: &str = "-e";
const POLL_INTERVAL: Duration = Duration::from_millis(5);

/// The engine's report line (`vm/compiled/residuals.rs`) and its fields. The engine names the tier
/// of each loop it reports: `u-jit` (Cranelift) or `b-jit` (stencils).
const LINE_PREFIX: &str = "QUENCH_RESIDUALS ";
const TIER_FIELD: &str = "tier";
const CRANELIFT_TIER: &str = "u-jit";
const STENCIL_TIER: &str = "b-jit";
const BYTES_FIELD: &str = "bytes";
/// Why a kernel has no row of counts when the engine compiled no loop of it.
const NO_LOOP: &str = "no compiled loop";
/// The category the hottest loop is chosen by.
const SIZE_CATEGORY: &str = "instructions";

struct Config {
    quench: PathBuf,
    aot_quench: PathBuf,
    kernels: Option<Vec<String>>,
    ratchet: PathBuf,
    update: bool,
    timeout: Duration,
}

impl Config {
    fn parse(args: &[String]) -> Self {
        let mut config = Self {
            quench: DEFAULT_QUENCH.into(),
            aot_quench: DEFAULT_AOT_QUENCH.into(),
            kernels: None,
            ratchet: Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../tests/fixtures/residuals/ratchet.txt"),
            update: false,
            timeout: Duration::from_millis(TIER_RUN_TIMEOUT_MS),
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
            AOT_QUENCH_FLAG => self.aot_quench = value.into(),
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

/// The engine's tier names (`tier=`) that a mode's loops may carry: the interpreter compiles none,
/// AOT compiles everything with the Cranelift tier, and the default mode may tier a function either way.
fn accepts(mode: Mode, tier: &str) -> bool {
    match mode {
        Mode::NoJit => false,
        Mode::Default => tier == CRANELIFT_TIER || tier == STENCIL_TIER,
        Mode::Aot => tier == CRANELIFT_TIER,
    }
}

fn parse_line(line: &str, mode: Mode) -> Option<Record> {
    let fields: HashMap<&str, &str> = line
        .strip_prefix(LINE_PREFIX)?
        .split_whitespace()
        .filter_map(|field| field.split_once('='))
        .collect();
    accepts(mode, fields.get(TIER_FIELD)?).then_some(())?;
    let mut counts = [0; CATEGORY_COUNT];
    for (slot, name) in counts.iter_mut().zip(CATEGORIES) {
        *slot = fields.get(name)?.parse().ok()?;
    }
    Some(Record {
        counts: Counts(counts),
        bytes: fields.get(BYTES_FIELD).and_then(|bytes| bytes.parse().ok()),
    })
}

/// The hottest loop the engine reported in `mode`: the one with the most instructions.
fn hottest(report: &str, mode: Mode) -> Option<Record> {
    report
        .lines()
        .filter_map(|line| parse_line(line, mode))
        .max_by_key(|record| record.counts.get(SIZE_CATEGORY))
}

/// The AOT executable of `kernel`: the kernel's source written beside the AOT-enabled `quench`, and
/// compiled ahead of time there.
fn compile_aot(config: &Config, kernel: &Kernel) -> Result<PathBuf, String> {
    let directory = config.aot_quench.parent().unwrap_or(Path::new(".")).join("residuals");
    fs::create_dir_all(&directory).map_err(|error| format!("{}: {error}", directory.display()))?;
    let source = directory.join(format!("{}.js", kernel.name));
    fs::write(&source, kernel.source).map_err(|error| format!("{}: {error}", source.display()))?;
    let program = directory.join(kernel.name);
    let compiled = Command::new(&config.aot_quench)
        .arg("--build-aot")
        .arg(&source)
        .arg("-o")
        .arg(&program)
        .output()
        .map_err(|error| format!("cannot run {}: {error}", config.aot_quench.display()))?;
    if compiled.status.success() {
        Ok(program)
    } else {
        Err(format!("{}: the AOT compile failed: {}", kernel.name, String::from_utf8_lossy(&compiled.stderr).trim()))
    }
}

/// The command that runs `kernel` in `mode` with the residual trace on.
fn kernel_command(config: &Config, mode: Mode, kernel: &Kernel) -> Result<Command, String> {
    let mut command = match mode {
        Mode::Aot => Command::new(compile_aot(config, kernel)?),
        Mode::NoJit => {
            let mut quench = Command::new(&config.quench);
            quench.arg(INTERPRETER_FLAG).args([EVAL_FLAG, kernel.source]);
            quench
        }
        Mode::Default => {
            let mut quench = Command::new(&config.quench);
            quench.args([EVAL_FLAG, kernel.source]);
            quench
        }
    };
    command.env("QUENCH_TRACE", "residuals");
    Ok(command)
}

/// Runs `kernel` in `mode` and returns what the engine reported on stderr.
fn run_kernel(config: &Config, mode: Mode, kernel: &Kernel) -> Result<String, String> {
    let mut child = kernel_command(config, mode, kernel)?
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot run {} for {}: {error}", mode.name(), kernel.name))?;
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

/// One kernel in one mode against its spec and recorded ratchet entry.
struct Row {
    kernel: &'static Kernel,
    mode: Mode,
    /// The hottest loop, or why there is none (no compiled loop, or the run failed).
    outcome: Result<Record, String>,
    over: Vec<&'static str>,
}

fn measure(config: &Config, ratchet: &Ratchet, kernel: &'static Kernel, mode: Mode) -> Row {
    let outcome = run_kernel(config, mode, kernel)
        .and_then(|report| hottest(&report, mode).ok_or_else(|| NO_LOOP.to_string()));
    let over = outcome.as_ref().map_or_else(
        |_| Vec::new(),
        |record| ratchet.regressions(kernel, mode, record.counts),
    );
    Row {
        kernel,
        mode,
        outcome,
        over,
    }
}

/// Whether the row breaks the judge: a loop above its recorded entry, a failed run, or a loop that
/// compiled before (a ratchet entry) and no longer does.
fn is_violation(row: &Row, ratchet: &Ratchet) -> bool {
    match &row.outcome {
        Ok(_) => !row.over.is_empty(),
        Err(reason) => reason != NO_LOOP || ratchet.has(row.kernel, row.mode),
    }
}

fn print_row(row: &Row, ratchet: &Ratchet) {
    let flag = if is_violation(row, ratchet) {
        "FAIL"
    } else {
        "ok"
    };
    let spec = row.kernel.expected(row.mode);
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
        row.mode.name()
    );
    for category in &row.over {
        println!("  regressed past the ratchet: {category}");
    }
}

/// Measures the selected kernels in every mode; the exit status is non-zero on any violation.
pub(crate) fn run(args: &[String]) -> i32 {
    let config = Config::parse(args);
    let text = std::fs::read_to_string(&config.ratchet).unwrap_or_default();
    let ratchet = Ratchet::parse(&text).unwrap_or_else(|error| fail(&error));
    let rows: Vec<Row> = config
        .selected()
        .flat_map(|kernel| Mode::ALL.map(|mode| measure(&config, &ratchet, kernel, mode)))
        .collect();
    rows.iter().for_each(|row| print_row(row, &ratchet));
    let violations = rows
        .iter()
        .filter(|row| is_violation(row, &ratchet))
        .count();
    if config.update {
        let measured = rows
            .iter()
            .filter_map(|row| Some((row.kernel.name, row.mode, row.outcome.as_ref().ok()?.counts)));
        let rendered = ratchet.tightened(measured).render();
        std::fs::write(&config.ratchet, rendered)
            .unwrap_or_else(|error| fail(&format!("cannot write ratchet: {error}")));
        println!("wrote {}", config.ratchet.display());
    }
    println!(
        "residuals: {violations} violation(s) over {} kernel-mode pair(s)",
        rows.len()
    );
    i32::from(violations > 0)
}
