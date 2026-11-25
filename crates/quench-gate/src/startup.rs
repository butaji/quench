//! Step `startup`: what starting the JavaScript host costs, in each execution mode (`--no-jit`,
//! `--b-jit`, and the default with no flag). For each mode it measures the instructions of three runs under callgrind
//! (an empty script, `console.log("hi")`, and a CommonJS `require` of a generated 200-function
//! module, written into the scratch directory by this step), and the peak resident set of the
//! first two: `wait4`'s `ru_maxrss` of the child (Linux, in kilobytes).
//!
//! Each measurement is held to `crates/quench-tier-corpus/startup_budgets.txt`, one row per
//! `<case> <mode> <metric> <ceiling>`. More than 2% over its ceiling fails. More than 10% under it
//! fails too, with the value to lower the ceiling to, so the ceilings only shrink. A run with
//! `QUENCH_DEBT_WRITE=1` rewrites the file: each ceiling becomes the lower of its old value and the
//! measurement, and a row with no ceiling takes its measurement; an over-budget measurement fails
//! and writes nothing. The runs use the caches' environment of the ir-budget step (no compile cache,
//! no startup snapshot, a fresh cache directory), so each run starts cold.
//!
//! Alone: `quench-gate --only startup`, on the host the build step built.

use std::{
    collections::BTreeMap,
    env,
    ffi::{c_int, c_long},
    fmt::Write as _,
    fs::{self, File},
    io,
    os::unix::process::ExitStatusExt,
    path::{Path, PathBuf},
    process::{Command, ExitStatus},
};

use crate::{build::NODE, config::Config, process, step::Verdict, tiers::INTERPRETER};

const BUDGETS: &str = "crates/quench-tier-corpus/startup_budgets.txt";
/// The scratch directory under the cargo target directory: generated scripts, callgrind output.
const SCRATCH: &str = "startup";
/// The three shipped modes (user, 2026-11-22): interpreter only, b-jit only, and the default
/// (no flag: every tier on), keyed in the budget file by flag, `default` for the default.
const MODES: [&str; 3] = [INTERPRETER, "--b-jit", DEFAULT];
const DEFAULT: &str = "default";
/// The cases measured under callgrind, and the cases whose peak resident set is measured.
const CASES: [&str; 3] = ["empty", "hello", "module"];
const RSS_CASES: [&str; 2] = ["empty", "hello"];
const IR: &str = "ir";
const RSS: &str = "rss_kb";
/// Functions in the generated module the `module` case requires.
const FUNCTIONS: usize = 200;
/// A measurement more than this many percent over its ceiling fails.
const OVER_PERCENT: u64 = 2;
/// A measurement more than this many percent under its ceiling fails, so the ceiling is lowered.
const UNDER_PERCENT: u64 = 10;
/// `1` rewrites the budget file from the measurements instead of failing on them.
const DEBT_WRITE: &str = "QUENCH_DEBT_WRITE";

/// One measurement's name: case, mode, metric.
type Key = (String, String, String);
type Ceilings = BTreeMap<Key, u64>;

fn key(case: &str, mode: &str, metric: &str) -> Key {
    (case.to_owned(), mode.to_owned(), metric.to_owned())
}

fn label(key: &Key) -> String {
    format!("{} {} {}", key.0, key.1, key.2)
}

/// Where one measurement stands against its ceiling.
#[derive(Debug, PartialEq)]
enum Judgement {
    Within,
    Over,
    Under,
    Unrecorded,
}

/// Judges `measured` against `ceiling` with integer arithmetic, so the limits are exact.
fn judge(measured: u64, ceiling: Option<u64>) -> Judgement {
    let Some(ceiling) = ceiling else { return Judgement::Unrecorded };
    if measured * 100 > ceiling * (100 + OVER_PERCENT) {
        Judgement::Over
    } else if measured * 100 < ceiling * (100 - UNDER_PERCENT) {
        Judgement::Under
    } else {
        Judgement::Within
    }
}

/// The result of holding every measurement to its ceiling.
#[derive(Debug, PartialEq)]
struct Settled {
    /// Why the step fails; empty when it passes.
    failures: Vec<String>,
    /// The ceilings the file holds after this run, were it written.
    next: Ceilings,
}

/// Holds `measured` to `ceilings`. Without `write`, every measurement outside its band and every
/// row with no measurement fails. With `write`, only an over-budget measurement fails; each
/// ceiling becomes the lower of its old value and the measurement, and stale rows are dropped.
fn settle(measured: &Ceilings, ceilings: &Ceilings, write: bool) -> Settled {
    let mut failures = Vec::new();
    let mut next = Ceilings::new();
    for (key, &value) in measured {
        let ceiling = ceilings.get(key).copied();
        next.insert(key.clone(), ceiling.map_or(value, |ceiling| ceiling.min(value)));
        let name = label(key);
        match (judge(value, ceiling), ceiling) {
            (Judgement::Over, Some(ceiling)) => {
                let percent = (value as f64 - ceiling as f64) / ceiling as f64 * 100.0;
                failures.push(format!("{name}: {value} is {percent:.1}% over its ceiling {ceiling} (limit {OVER_PERCENT}%)"));
            }
            (Judgement::Under, Some(_)) if !write => failures.push(format!("{name}: {value} is more than {UNDER_PERCENT}% under its ceiling; lower the ceiling to {value}")),
            (Judgement::Unrecorded, _) if !write => failures.push(format!("{name}: {value} with no ceiling in {BUDGETS} (record it with {DEBT_WRITE}=1)")),
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

/// The total instructions of a callgrind output file: its last `totals:` line, else `summary:`.
fn parse_totals(text: &str) -> Option<u64> {
    let total = |prefix: &str| text.lines().rev().find_map(|line| line.strip_prefix(prefix)).and_then(|rest| rest.split_whitespace().next()?.parse().ok());
    total("totals: ").or_else(|| total("summary: "))
}

/// The budget file's rows, skipping comments and blank lines.
fn parse_budgets(text: &str) -> Result<Ceilings, String> {
    let mut rows = Ceilings::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        let &[case, mode, metric, ceiling] = fields.as_slice() else {
            return Err(format!("{BUDGETS}:{}: expected `case mode metric ceiling`, found `{line}`", index + 1));
        };
        let ceiling: u64 = ceiling.parse().map_err(|_| format!("{BUDGETS}:{}: ceiling `{ceiling}` is not a whole number", index + 1))?;
        if rows.insert(key(case, mode, metric), ceiling).is_some() {
            return Err(format!("{BUDGETS}:{}: `{line}` repeats a row", index + 1));
        }
    }
    Ok(rows)
}

/// The header of the budget file: its leading comment lines, kept verbatim on a rewrite.
fn header(text: &str) -> String {
    text.lines().take_while(|line| line.starts_with('#')).map(|line| format!("{line}\n")).collect()
}

fn render(header: &str, rows: &Ceilings) -> String {
    let body: String = rows.iter().map(|(key, ceiling)| format!("{} {} {} {ceiling}\n", key.0, key.1, key.2)).collect();
    format!("{header}{body}")
}

/// The source of the generated module: `FUNCTIONS` small functions and an export of all of them.
fn module_source() -> String {
    let mut text = String::new();
    for index in 0..FUNCTIONS {
        let _ = writeln!(text, "function f{index}(x) {{ return x * {index} + {}; }}", index % 7);
    }
    let names: Vec<String> = (0..FUNCTIONS).map(|index| format!("f{index}")).collect();
    let _ = writeln!(text, "module.exports = {{ {} }};", names.join(", "));
    text
}

fn write_file(path: &Path, text: &str) -> Result<(), String> {
    fs::write(path, text).map_err(|error| format!("{}: {error}", path.display()))
}

/// Writes the scripts into `scratch`: the case name to its entry script.
fn write_scripts(scratch: &Path) -> Result<BTreeMap<&'static str, PathBuf>, String> {
    let dir = scratch.join("scripts");
    let module = dir.join("module");
    fs::create_dir_all(&module).map_err(|error| format!("{}: {error}", module.display()))?;
    // The scratch sits inside the repo, whose package.json says "type": "module": the scripts are
    // CommonJS, so their own package.json says so (otherwise `require` is undefined in module).
    write_file(&dir.join("package.json"), "{\"type\": \"commonjs\"}\n")?;
    let empty = dir.join("empty.js");
    write_file(&empty, "")?;
    let hello = dir.join("hello.js");
    write_file(&hello, "console.log(\"hi\")\n")?;
    write_file(&module.join("lib.js"), &module_source())?;
    let main = module.join("main.js");
    write_file(&main, "const lib = require(\"./lib.js\");\nconsole.log(Object.keys(lib).length);\n")?;
    Ok(BTreeMap::from([("empty", empty), ("hello", hello), ("module", main)]))
}

/// The environment every measured run starts with: the mode's JIT flags, no caches, a fresh
/// cache directory.
fn environment(command: &mut Command, mode: &str, scratch: &Path) {
    // The forced tier compiles synchronously; the default mode runs as users run it.
    if mode != INTERPRETER && mode != DEFAULT {
        command.env("QUENCH_DEV", "sync-compile");
    }
    command.env("QUENCH_COMPILE_CACHE", "").env("QUENCH_STARTUP_SNAPSHOT", "off").env("XDG_CACHE_HOME", scratch);
}

/// The instructions of one run of `host mode script`, under callgrind.
fn instructions(config: &Config, host: &Path, mode: &str, script: &Path, scratch: &Path) -> Result<u64, String> {
    let out = scratch.join("callgrind.out");
    let mut command = config.command("valgrind");
    command
        .arg("--tool=callgrind")
        .arg("--dump-instr=no")
        .arg(format!("--callgrind-out-file={}", out.display()))
        // JIT tiers patch the code they emit: valgrind must see each write.
        .args((mode != INTERPRETER).then_some("--smc-check=all"))
        .arg(host)
        .args((mode != DEFAULT).then_some(mode))
        .arg(script);
    environment(&mut command, mode, scratch);
    let run = process::run(&mut command)?;
    if !run.success {
        return Err(format!("{} {mode} {} failed under callgrind\n{}", host.display(), script.display(), run.excerpt()));
    }
    let text = fs::read_to_string(&out).map_err(|error| format!("{}: {error}", out.display()))?;
    let _ = fs::remove_file(&out);
    parse_totals(&text).ok_or_else(|| format!("{}: no totals line", out.display()))
}

/// The `rusage` of a finished child, laid out as glibc's `struct rusage` on 64-bit Linux.
#[repr(C)]
#[derive(Default)]
struct Rusage {
    utime: [c_long; 2],
    stime: [c_long; 2],
    maxrss: c_long,
    _rest: [c_long; 13],
}

unsafe extern "C" {
    fn wait4(pid: c_int, status: *mut c_int, options: c_int, rusage: *mut Rusage) -> c_int;
}

/// The peak resident set of one run of `host mode script`, in kilobytes: the child's `ru_maxrss`
/// from `wait4`. The child's output goes to files in `scratch`, so nothing blocks on a full pipe.
fn peak_rss_kb(config: &Config, host: &Path, mode: &str, script: &Path, scratch: &Path) -> Result<u64, String> {
    let open = |name: &str| File::create(scratch.join(name)).map_err(|error| format!("{}: {error}", scratch.join(name).display()));
    let mut command = config.command(host);
    command.args((mode != DEFAULT).then_some(mode)).arg(script).stdout(open("stdout")?).stderr(open("stderr")?);
    environment(&mut command, mode, scratch);
    let child = command.spawn().map_err(|error| format!("cannot run {}: {error}", host.display()))?;
    let (mut status, mut usage): (c_int, Rusage) = (0, Rusage::default());
    // SAFETY: `child` is an unwaited child of this process, and `status` and `usage` are valid
    // for writes for the duration of the call.
    let reaped = unsafe { wait4(child.id() as c_int, &mut status, 0, &mut usage) };
    if reaped < 0 {
        return Err(format!("wait4: {}", io::Error::last_os_error()));
    }
    let exit = ExitStatus::from_raw(status);
    if !exit.success() {
        return Err(format!("{} {mode} {} exited with {exit}", host.display(), script.display()));
    }
    Ok(usage.maxrss as u64)
}

/// The measurements as a table: name, measured value, ceiling.
fn table(measured: &Ceilings, ceilings: &Ceilings) -> String {
    let head = format!("{:<8}{:<9}{:<8}{:>12}{:>12}", "case", "mode", "metric", "measured", "ceiling");
    let lines = measured.iter().map(|(key, value)| {
        let ceiling = ceilings.get(key).map_or_else(|| "-".to_owned(), u64::to_string);
        format!("{:<8}{:<9}{:<8}{value:>12}{ceiling:>12}", key.0, key.1, key.2)
    });
    std::iter::once(head).chain(lines).collect::<Vec<_>>().join("\n")
}

pub fn run(config: &Config) -> Verdict {
    let host = config.binary(NODE)?;
    let scratch = config.target.join(SCRATCH);
    let _ = fs::remove_dir_all(&scratch);
    fs::create_dir_all(&scratch).map_err(|error| format!("{}: {error}", scratch.display()))?;
    let scripts = write_scripts(&scratch)?;
    let mut measured = Ceilings::new();
    for mode in MODES {
        for case in CASES {
            let instructions = instructions(config, &host, mode, &scripts[case], &scratch)?;
            measured.insert(key(case, mode, IR), instructions);
        }
        for case in RSS_CASES {
            let kilobytes = peak_rss_kb(config, &host, mode, &scripts[case], &scratch)?;
            measured.insert(key(case, mode, RSS), kilobytes);
        }
    }
    let _ = fs::remove_dir_all(&scratch);

    let path = config.root.join(BUDGETS);
    let before = fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    let ceilings = parse_budgets(&before)?;
    let write = env::var(DEBT_WRITE).as_deref() == Ok("1");
    let settled = settle(&measured, &ceilings, write);
    let table = table(&measured, &ceilings);
    if !settled.failures.is_empty() {
        return Err(format!("{}\n\n{table}", settled.failures.join("\n")));
    }
    if write {
        let after = render(&header(&before), &settled.next);
        if after != before {
            write_file(&path, &after)?;
        }
        return Ok(format!("{} startup ceilings recorded in {BUDGETS}\n{table}", settled.next.len()));
    }
    Ok(format!("{} startup measurements within their ceilings\n{table}", measured.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(entries: &[(&str, &str, &str, u64)]) -> Ceilings {
        entries.iter().map(|&(case, mode, metric, value)| (key(case, mode, metric), value)).collect()
    }

    #[test]
    fn totals_come_from_the_last_totals_line() {
        let text = "summary: 5\nfn=(1) main\n1 3\ntotals: 42\n";
        assert_eq!(parse_totals(text), Some(42));
    }

    #[test]
    fn totals_fall_back_to_the_summary_line() {
        assert_eq!(parse_totals("summary: 7\n"), Some(7));
        assert_eq!(parse_totals("events: Ir\n"), None);
        assert_eq!(parse_totals("totals: many\n"), None);
    }

    #[test]
    fn budget_rows_parse_and_render_back() {
        let text = "# header line\n\nempty --no-jit ir 1000\nhello --b-jit rss_kb 2048\n";
        let parsed = parse_budgets(text).unwrap();
        assert_eq!(parsed, rows(&[("empty", "--no-jit", "ir", 1000), ("hello", "--b-jit", "rss_kb", 2048)]));
        assert_eq!(render(&header(text), &parsed), "# header line\nempty --no-jit ir 1000\nhello --b-jit rss_kb 2048\n");
    }

    #[test]
    fn malformed_budget_rows_are_errors() {
        assert!(parse_budgets("empty --no-jit ir\n").is_err());
        assert!(parse_budgets("empty --no-jit ir many\n").is_err());
        assert!(parse_budgets("empty --no-jit ir 1\nempty --no-jit ir 2\n").is_err());
    }

    #[test]
    fn the_judge_draws_its_bands_exactly() {
        assert_eq!(judge(1000, None), Judgement::Unrecorded);
        assert_eq!(judge(1000, Some(1000)), Judgement::Within);
        assert_eq!(judge(1020, Some(1000)), Judgement::Within);
        assert_eq!(judge(1021, Some(1000)), Judgement::Over);
        assert_eq!(judge(900, Some(1000)), Judgement::Within);
        assert_eq!(judge(899, Some(1000)), Judgement::Under);
    }

    #[test]
    fn a_run_without_debt_write_fails_on_slack_and_on_missing_rows() {
        let measured = rows(&[("empty", "--no-jit", "ir", 850), ("hello", "--no-jit", "ir", 5)]);
        let ceilings = rows(&[("empty", "--no-jit", "ir", 1000), ("gone", "--no-jit", "ir", 7)]);
        let settled = settle(&measured, &ceilings, false);
        assert_eq!(settled.failures.len(), 3);
        assert!(settled.failures.iter().any(|line| line.contains("empty --no-jit ir") && line.contains("lower the ceiling to 850")));
        assert!(settled.failures.iter().any(|line| line.contains("hello --no-jit ir") && line.contains("no ceiling")));
        assert!(settled.failures.iter().any(|line| line.contains("gone --no-jit ir") && line.contains("remove it")));
    }

    #[test]
    fn debt_write_only_lowers_ceilings_and_fails_only_when_over() {
        let measured = rows(&[("empty", "--no-jit", "ir", 900), ("hello", "--no-jit", "ir", 1010), ("module", "--no-jit", "ir", 5)]);
        let ceilings = rows(&[("empty", "--no-jit", "ir", 1000), ("hello", "--no-jit", "ir", 1000), ("gone", "--no-jit", "ir", 7)]);
        let settled = settle(&measured, &ceilings, true);
        assert!(settled.failures.is_empty());
        assert_eq!(settled.next, rows(&[("empty", "--no-jit", "ir", 900), ("hello", "--no-jit", "ir", 1000), ("module", "--no-jit", "ir", 5)]));

        let over = rows(&[("hello", "--no-jit", "ir", 1100)]);
        let settled = settle(&over, &ceilings, true);
        assert_eq!(settled.failures.len(), 1);
        assert_eq!(settled.next[&key("hello", "--no-jit", "ir")], 1000);
    }

    #[test]
    fn the_generated_module_is_deterministic_and_has_its_functions() {
        let text = module_source();
        assert_eq!(text, module_source());
        assert_eq!(text.matches("function f").count(), FUNCTIONS);
        assert!(text.contains(&format!("f{}", FUNCTIONS - 1)));
    }
}
