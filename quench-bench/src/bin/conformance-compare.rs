//! Runs a conformance suite case by case on Quench, Node and Bun in one execution mode and
//! compares each engine's processes: wall time, CPU time, peak RSS and outcome.
//!
//! ```sh
//! conformance-compare --suite test262|wasm --mode no-jit|jit --out DIR \
//!     --quench BINARY --node NODE --bun BUN [--wasm-json DIR] [--jobs N] [--timeout-ms MS] \
//!     [--every K]
//! ```
//!
//! `--every K` keeps every Kth case in discovery order (a sample spread across every area).
//!
//! Every case runs as its own process on every engine, as Quench's runners run them:
//!
//! - Test262: `run-all --case [--jit] PATH` against `node|bun tools/test262-engine-case.cjs`
//!   (the Node oracle's harness composition). Module cases are excluded: the JavaScript
//!   host runner cannot load them.
//! - Wasm: `run --worker FILE OUT` (`QUENCH_EXECUTION_MODE`) against
//!   `node|bun tools/wast-engine-run.cjs` on the script `wast-json` converted, so Quench's
//!   process parses `.wast` text while the others read encoded binaries.
//!
//! No-JIT is Quench's interpreter, `node --jitless` (Test262) or `node --wasm-jitless`
//! (Wasm: `--jitless` removes WebAssembly), and `BUN_JSC_useJIT=0`; JIT is Quench's
//! compiled execution and each engine's default. Bun runs without the shell's `BUN_OPTIONS`.
//!
//! Each process starts through `rss-exec` (beside this binary), whose own resident set is
//! a few MiB: Linux carries the exec'ing address space's high-water mark into the child's
//! `ru_maxrss`, so a process spawned from this driver would report at least the driver's
//! own peak. `rss-exec` reports its child's peak exactly; CPU time comes from `wait4` on
//! `rss-exec`, which includes the child it reaped.
//!
//! Workers take (case, engine) pairs in case order, so contention falls on every engine
//! alike. `DIR/samples.jsonl` holds one measured process per line; `DIR/summary.json` and
//! `DIR/summary.md` aggregate them per engine and per area (a Test262 directory two levels
//! deep, or a Wasm script) with Quench's ratios to each other engine.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::ffi::CString;
use std::process::{Command, ExitCode};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const ENGINES: [&str; 3] = ["quench", "node", "bun"];
/// Quench must beat every other engine by this fraction in an area: its value at most
/// `1 - WIN_MARGIN` of the other's.
const WIN_MARGIN: f64 = 0.2;
const DEFAULT_TIMEOUT_MS: u64 = 60_000;
const WATCHDOG_INTERVAL: Duration = Duration::from_millis(50);
/// The exit status a shell reports for a process a signal ended.
const SIGNAL_STATUS_BASE: i32 = 128;
const TIMEOUT_STATUS: i32 = -1;
const KIB: f64 = 1024.0;
const STDERR_FILE_MODE: libc::mode_t = 0o600;
/// The line `rss-exec` ends its stderr with: `rss-exec maxrss_kib=N status=S`.
const RSS_EXEC_PREFIX: &str = "rss-exec maxrss_kib=";
const MICROS_PER_MILLI: f64 = 1000.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Suite {
    Test262,
    Wasm,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    NoJit,
    Jit,
}

impl Mode {
    fn name(self) -> &'static str {
        match self {
            Mode::NoJit => "no-jit",
            Mode::Jit => "jit",
        }
    }
}

struct Options {
    suite: Suite,
    mode: Mode,
    out: PathBuf,
    quench: PathBuf,
    node: PathBuf,
    bun: PathBuf,
    wasm_json: Option<PathBuf>,
    jobs: usize,
    timeout: Duration,
    every: usize,
    rss_exec: PathBuf,
}

/// One case: its report path, its area, and per engine the command that runs it.
struct Case {
    id: String,
    area: String,
    commands: [Vec<String>; 3],
}

struct Sample {
    case: usize,
    engine: usize,
    wall_ms: f64,
    user_ms: f64,
    sys_ms: f64,
    maxrss_kib: i64,
    status: i32,
    passed: bool,
}

fn main() -> ExitCode {
    let options = match parse(std::env::args().skip(1)) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("conformance-compare: {error}");
            return ExitCode::from(2);
        }
    };
    match run(&options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("conformance-compare: {error}");
            ExitCode::from(1)
        }
    }
}

fn parse(mut arguments: impl Iterator<Item = String>) -> Result<Options, String> {
    let mut suite = None;
    let mut mode = None;
    let mut out = None;
    let mut quench = None;
    let mut node = None;
    let mut bun = None;
    let mut wasm_json = None;
    let mut jobs = thread::available_parallelism().map_or(1, |jobs| jobs.get());
    let mut timeout = Duration::from_millis(DEFAULT_TIMEOUT_MS);
    let mut every = 1;
    while let Some(argument) = arguments.next() {
        let mut value = || arguments.next().ok_or(format!("{argument} needs a value"));
        match argument.as_str() {
            "--suite" => {
                suite = Some(match value()?.as_str() {
                    "test262" => Suite::Test262,
                    "wasm" => Suite::Wasm,
                    other => return Err(format!("unknown suite {other}")),
                })
            }
            "--mode" => {
                mode = Some(match value()?.as_str() {
                    "no-jit" => Mode::NoJit,
                    "jit" => Mode::Jit,
                    other => return Err(format!("unknown mode {other}")),
                })
            }
            "--out" => out = Some(PathBuf::from(value()?)),
            "--quench" => quench = Some(PathBuf::from(value()?)),
            "--node" => node = Some(PathBuf::from(value()?)),
            "--bun" => bun = Some(PathBuf::from(value()?)),
            "--wasm-json" => wasm_json = Some(PathBuf::from(value()?)),
            "--every" => every = value()?.parse().map_err(|_| "--every needs a count")?,
            "--jobs" => jobs = value()?.parse().map_err(|_| "--jobs needs a count")?,
            "--timeout-ms" => {
                timeout = Duration::from_millis(value()?.parse().map_err(|_| "--timeout-ms needs milliseconds")?)
            }
            other => return Err(format!("unexpected argument {other}")),
        }
    }
    let suite = suite.ok_or("--suite is required")?;
    if suite == Suite::Wasm && wasm_json.is_none() {
        return Err("--suite wasm needs --wasm-json (the wast-json output)".into());
    }
    Ok(Options {
        suite,
        mode: mode.ok_or("--mode is required")?,
        out: out.ok_or("--out is required")?,
        quench: quench.ok_or("--quench is required")?,
        node: node.ok_or("--node is required")?,
        bun: bun.ok_or("--bun is required")?,
        wasm_json,
        jobs: jobs.max(1),
        timeout,
        every: every.max(1),
        rss_exec: std::env::current_exe()
            .map_err(|error| error.to_string())?
            .with_file_name("rss-exec"),
    })
}

fn run(options: &Options) -> Result<(), String> {
    if options.out.exists() {
        return Err(format!("{} exists; use a new output directory", options.out.display()));
    }
    fs::create_dir_all(options.out.join("scratch")).map_err(|error| error.to_string())?;
    let (cases, excluded) = match options.suite {
        Suite::Test262 => test262_cases(options)?,
        Suite::Wasm => wasm_cases(options)?,
    };
    let cases = cases.into_iter().step_by(options.every).collect::<Vec<_>>();
    eprintln!("cases={} excluded={excluded} jobs={}", cases.len(), options.jobs);
    let started = Instant::now();
    let samples = measure(options, &cases)?;
    let elapsed = started.elapsed();
    let mut lines = fs::File::create(options.out.join("samples.jsonl")).map_err(|error| error.to_string())?;
    for sample in &samples {
        let line = json!({
            "case": cases[sample.case].id, "area": cases[sample.case].area,
            "engine": ENGINES[sample.engine], "wall_ms": sample.wall_ms, "user_ms": sample.user_ms,
            "sys_ms": sample.sys_ms, "maxrss_kib": sample.maxrss_kib, "status": sample.status,
            "passed": sample.passed,
        });
        writeln!(lines, "{line}").map_err(|error| error.to_string())?;
    }
    let summary = summarize(options, &cases, &samples, excluded, elapsed);
    fs::write(options.out.join("summary.json"), serde_json::to_string_pretty(&summary).unwrap())
        .map_err(|error| error.to_string())?;
    fs::write(options.out.join("summary.md"), markdown(&summary)).map_err(|error| error.to_string())?;
    let _ = fs::remove_dir_all(options.out.join("scratch"));
    print!("{}", markdown(&summary));
    Ok(())
}

fn repository() -> PathBuf {
    std::env::current_dir().expect("working directory")
}

fn test262_cases(options: &Options) -> Result<(Vec<Case>, usize), String> {
    let root = repository().join("tests/test262/test");
    let mut files = Vec::new();
    collect(&root, "js", &mut files)?;
    files.retain(|path| !path.to_string_lossy().ends_with("_FIXTURE.js"));
    files.sort();
    let jit = options.mode == Mode::Jit;
    let runner = repository().join("tools/test262-engine-case.cjs");
    let mut cases = Vec::new();
    let mut excluded = 0;
    for file in files {
        let source = fs::read_to_string(&file).map_err(|error| format!("{}: {error}", file.display()))?;
        if is_module_case(&source) {
            excluded += 1;
            continue;
        }
        let relative = file.strip_prefix(&root).unwrap().to_string_lossy().into_owned();
        let mut quench = vec![options.quench.display().to_string(), "--case".into()];
        if jit {
            quench.push("--jit".into());
        }
        quench.push(file.display().to_string());
        let mut node = vec![options.node.display().to_string()];
        if !jit {
            node.push("--jitless".into());
        }
        node.extend([runner.display().to_string(), relative.clone()]);
        let bun = vec![options.bun.display().to_string(), runner.display().to_string(), relative.clone()];
        cases.push(Case { area: area_of(&relative), id: relative, commands: [quench, node, bun] });
    }
    Ok((cases, excluded))
}

/// Whether the case's frontmatter flags it as a module.
fn is_module_case(source: &str) -> bool {
    let Some(start) = source.find("/*---") else { return false };
    let Some(end) = source[start..].find("---*/") else { return false };
    source[start..start + end]
        .lines()
        .any(|line| line.trim_start().starts_with("flags:") && line.contains("module"))
}

/// A Test262 path's first two directories (`built-ins/Array`).
fn area_of(relative: &str) -> String {
    relative.split('/').take(2).collect::<Vec<_>>().join("/")
}

fn wasm_cases(options: &Options) -> Result<(Vec<Case>, usize), String> {
    let testsuite = repository().join("crates/quench-wasm-test/testsuite");
    let json_root = options.wasm_json.as_ref().unwrap();
    let mut files = Vec::new();
    collect(&testsuite, "wast", &mut files)?;
    files.sort();
    let runner = repository().join("tools/wast-engine-run.cjs");
    let jit = options.mode == Mode::Jit;
    let mut cases = Vec::new();
    let mut excluded = 0;
    for (index, file) in files.into_iter().enumerate() {
        let relative = file.strip_prefix(&testsuite).unwrap().to_string_lossy().into_owned();
        let script = json_root.join(Path::new(&relative).with_extension("json"));
        if !script.exists() {
            excluded += 1;
            continue;
        }
        let report = options.out.join("scratch").join(format!("{index}.json"));
        let quench = vec![
            options.quench.display().to_string(),
            "--worker".into(),
            file.display().to_string(),
            report.display().to_string(),
        ];
        let mut node = vec![options.node.display().to_string()];
        if !jit {
            node.push("--wasm-jitless".into());
        }
        node.extend([runner.display().to_string(), script.display().to_string()]);
        let bun = vec![options.bun.display().to_string(), runner.display().to_string(), script.display().to_string()];
        cases.push(Case { area: relative.clone(), id: relative, commands: [quench, node, bun] });
    }
    Ok((cases, excluded))
}

fn collect(directory: &Path, extension: &str, files: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(directory).map_err(|error| format!("{}: {error}", directory.display()))? {
        let path = entry.map_err(|error| error.to_string())?.path();
        if path.is_dir() {
            collect(&path, extension, files)?;
        } else if path.extension().is_some_and(|found| found == extension) {
            files.push(path);
        }
    }
    Ok(())
}

/// Per-engine environment: the mode's switch for engines that take it from the
/// environment, and Bun without the shell's `BUN_OPTIONS`.
fn environment(engine: usize, options: &Options) -> Vec<CString> {
    let mut variables = std::env::vars().collect::<BTreeMap<_, _>>();
    match ENGINES[engine] {
        "quench" => {
            variables.insert("TEST262_DIR".into(), repository().join("tests/test262").display().to_string());
            variables.insert("TEST262_TEST_TIMEOUT_MS".into(), options.timeout.as_millis().to_string());
            variables.insert("QUENCH_EXECUTION_MODE".into(), options.mode.name().into());
        }
        "bun" => {
            variables.remove("BUN_OPTIONS");
            let jit = if options.mode == Mode::Jit { "1" } else { "0" };
            variables.insert("BUN_JSC_useJIT".into(), jit.into());
        }
        _ => {}
    }
    variables
        .into_iter()
        .filter_map(|(key, value)| CString::new(format!("{key}={value}")).ok())
        .collect()
}

/// Starts `argv` with `posix_spawn`; stdin and stdout are `/dev/null`, stderr is `stderr`.
fn spawn(argv: &[String], environment: &[CString], stderr: &Path) -> Result<libc::pid_t, String> {
    let arguments = argv
        .iter()
        .map(|argument| CString::new(argument.as_str()).map_err(|_| format!("{argument:?} contains NUL")))
        .collect::<Result<Vec<_>, _>>()?;
    let mut argv = arguments.iter().map(|argument| argument.as_ptr().cast_mut()).collect::<Vec<_>>();
    argv.push(std::ptr::null_mut());
    let mut envp = environment.iter().map(|variable| variable.as_ptr().cast_mut()).collect::<Vec<_>>();
    envp.push(std::ptr::null_mut());
    let null = CString::new("/dev/null").unwrap();
    let stderr = CString::new(stderr.as_os_str().as_encoded_bytes()).map_err(|_| "stderr path contains NUL")?;
    // SAFETY: the file actions are initialized before use and destroyed after the spawn;
    // argv and envp are null-terminated arrays of C strings that outlive the call.
    unsafe {
        let mut actions: libc::posix_spawn_file_actions_t = std::mem::zeroed();
        libc::posix_spawn_file_actions_init(&mut actions);
        libc::posix_spawn_file_actions_addopen(&mut actions, libc::STDIN_FILENO, null.as_ptr(), libc::O_RDONLY, 0);
        libc::posix_spawn_file_actions_addopen(&mut actions, libc::STDOUT_FILENO, null.as_ptr(), libc::O_WRONLY, 0);
        libc::posix_spawn_file_actions_addopen(
            &mut actions,
            libc::STDERR_FILENO,
            stderr.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC,
            STDERR_FILE_MODE,
        );
        let mut pid = 0;
        let spawned = libc::posix_spawn(&mut pid, arguments[0].as_ptr(), &actions, std::ptr::null(), argv.as_ptr(), envp.as_ptr());
        libc::posix_spawn_file_actions_destroy(&mut actions);
        if spawned != 0 {
            return Err(format!("{}: {}", argv_name(&arguments), std::io::Error::from_raw_os_error(spawned)));
        }
        Ok(pid)
    }
}

fn argv_name(arguments: &[CString]) -> String {
    arguments[0].to_string_lossy().into_owned()
}

fn measure(options: &Options, cases: &[Case]) -> Result<Vec<Sample>, String> {
    let next = AtomicUsize::new(0);
    let total = cases.len() * ENGINES.len();
    let samples = Mutex::new(Vec::with_capacity(total));
    let watchdog = Arc::new(Mutex::new(HashMap::<i32, Instant>::new()));
    let done = Arc::new(AtomicUsize::new(0));
    let watch = {
        let watchdog = watchdog.clone();
        let done = done.clone();
        thread::spawn(move || {
            while done.load(Ordering::Relaxed) == 0 {
                thread::sleep(WATCHDOG_INTERVAL);
                let now = Instant::now();
                for (pid, deadline) in watchdog.lock().unwrap().iter() {
                    if now > *deadline {
                        // SAFETY: the pid is unreaped while registered (`wait_measured`).
                        unsafe { libc::kill(*pid, libc::SIGKILL) };
                    }
                }
            }
        })
    };
    thread::scope(|scope| {
        for _ in 0..options.jobs {
            scope.spawn(|| loop {
                let index = next.fetch_add(1, Ordering::Relaxed);
                if index >= total {
                    break;
                }
                let (case, engine) = (index / ENGINES.len(), index % ENGINES.len());
                let sample = run_one(options, &cases[case], case, engine, &watchdog);
                let mut samples = samples.lock().unwrap();
                samples.push(sample);
                if samples.len() % 1000 == 0 {
                    eprintln!("progress={}/{total}", samples.len());
                }
            });
        }
    });
    done.store(1, Ordering::Relaxed);
    watch.join().map_err(|_| "watchdog panicked")?;
    let mut samples = samples.into_inner().unwrap();
    samples.sort_by_key(|sample| (sample.case, sample.engine));
    Ok(samples)
}

fn run_one(
    options: &Options,
    case: &Case,
    index: usize,
    engine: usize,
    watchdog: &Mutex<HashMap<i32, Instant>>,
) -> Sample {
    let mut argv = vec![options.rss_exec.display().to_string(), "--".into()];
    argv.extend(case.commands[engine].iter().cloned());
    let environment = environment(engine, options);
    let stderr = options.out.join("scratch").join(format!("{index}-{engine}.stderr"));
    let started = Instant::now();
    let measured = spawn(&argv, &environment, &stderr).map(|pid| {
        watchdog.lock().unwrap().insert(pid, started + options.timeout);
        wait_measured(pid, watchdog)
    });
    let wall_ms = started.elapsed().as_secs_f64() * MICROS_PER_MILLI;
    let (status, usage) = measured.unwrap_or((SIGNAL_STATUS_BASE, zeroed_usage()));
    let timed_out = wall_ms >= options.timeout.as_secs_f64() * MICROS_PER_MILLI;
    let status = if timed_out { TIMEOUT_STATUS } else { status };
    let passed = status == 0 && outcome_passed(options, case, engine);
    let maxrss_kib = fs::read_to_string(&stderr)
        .ok()
        .and_then(|text| {
            text.lines().rev().find_map(|line| {
                line.strip_prefix(RSS_EXEC_PREFIX)?.split_whitespace().next()?.parse::<i64>().ok()
            })
        })
        .unwrap_or(usage.ru_maxrss);
    let _ = fs::remove_file(&stderr);
    Sample {
        case: index,
        engine,
        wall_ms,
        user_ms: timeval_ms(usage.ru_utime),
        sys_ms: timeval_ms(usage.ru_stime),
        maxrss_kib,
        status,
        passed,
    }
}

/// A Quench Wasm worker exits 0 after scoring; its report (one outcome per directive)
/// says whether every directive passed. The others' exit status is their verdict.
fn outcome_passed(options: &Options, case: &Case, engine: usize) -> bool {
    if options.suite != Suite::Wasm || ENGINES[engine] != "quench" {
        return true;
    }
    let report = PathBuf::from(&case.commands[engine][3]);
    let passed = fs::read_to_string(&report)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|report| report.as_array().cloned())
        .is_some_and(|outcomes| outcomes.iter().all(|outcome| outcome["outcome"] == "pass"));
    let _ = fs::remove_file(&report);
    passed
}

fn zeroed_usage() -> libc::rusage {
    // SAFETY: rusage is plain integers; all-zero is a valid value.
    unsafe { std::mem::zeroed() }
}

fn timeval_ms(time: libc::timeval) -> f64 {
    time.tv_sec as f64 * MICROS_PER_MILLI + time.tv_usec as f64 / MICROS_PER_MILLI
}

/// Waits for `pid` without reaping it, takes it off the watchdog, then reaps it with its
/// resource usage, so the watchdog never signals a reused pid.
fn wait_measured(pid: i32, watchdog: &Mutex<HashMap<i32, Instant>>) -> (i32, libc::rusage) {
    // SAFETY: siginfo is plain data; waitid fills it.
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    // SAFETY: pid names this process's child; WNOWAIT leaves it reapable.
    unsafe { libc::waitid(libc::P_PID, pid as libc::id_t, &mut info, libc::WEXITED | libc::WNOWAIT) };
    watchdog.lock().unwrap().remove(&pid);
    let mut status = 0;
    let mut usage = zeroed_usage();
    // SAFETY: reaps the exited child and writes its status and usage to live values.
    unsafe { libc::wait4(pid, &mut status, 0, &mut usage) };
    let code = if libc::WIFEXITED(status) {
        libc::WEXITSTATUS(status)
    } else {
        SIGNAL_STATUS_BASE + libc::WTERMSIG(status)
    };
    (code, usage)
}

#[derive(Default, Clone)]
struct Totals {
    cases: usize,
    passed: usize,
    timeouts: usize,
    wall_ms: f64,
    cpu_ms: f64,
    rss: Vec<i64>,
}

impl Totals {
    fn add(&mut self, sample: &Sample) {
        self.cases += 1;
        self.passed += usize::from(sample.passed);
        self.timeouts += usize::from(sample.status == TIMEOUT_STATUS);
        self.wall_ms += sample.wall_ms;
        self.cpu_ms += sample.user_ms + sample.sys_ms;
        self.rss.push(sample.maxrss_kib);
    }

    fn json(&self) -> Value {
        let mut rss = self.rss.clone();
        rss.sort_unstable();
        let percentile = |fraction: f64| {
            rss.get(((rss.len() as f64 - 1.0) * fraction).round() as usize).copied().unwrap_or(0)
        };
        json!({
            "cases": self.cases, "passed": self.passed, "timeouts": self.timeouts,
            "wall_s": self.wall_ms / MICROS_PER_MILLI, "cpu_s": self.cpu_ms / MICROS_PER_MILLI,
            "rss_median_mib": percentile(0.5) as f64 / KIB, "rss_p95_mib": percentile(0.95) as f64 / KIB,
            "rss_max_mib": percentile(1.0) as f64 / KIB,
            "rss_mean_mib": rss.iter().sum::<i64>() as f64 / rss.len().max(1) as f64 / KIB,
        })
    }
}

/// The metrics compared per area, each lower-is-better.
const METRICS: [&str; 4] = ["cpu_s", "wall_s", "rss_median_mib", "rss_max_mib"];

fn summarize(options: &Options, cases: &[Case], samples: &[Sample], excluded: usize, elapsed: Duration) -> Value {
    let mut overall = vec![Totals::default(); ENGINES.len()];
    let mut areas = BTreeMap::<&str, Vec<Totals>>::new();
    for sample in samples {
        overall[sample.engine].add(sample);
        areas.entry(&cases[sample.case].area).or_insert_with(|| vec![Totals::default(); ENGINES.len()])[sample.engine]
            .add(sample);
    }
    let compare = |totals: &[Totals]| {
        let values = totals.iter().map(Totals::json).collect::<Vec<_>>();
        let mut ratios = serde_json::Map::new();
        let mut losses = Vec::new();
        for metric in METRICS {
            let quench = values[0][metric].as_f64().unwrap_or(0.0);
            for (engine, other) in values.iter().enumerate().skip(1) {
                let other = other[metric].as_f64().unwrap_or(0.0);
                let ratio = if other > 0.0 { quench / other } else { f64::INFINITY };
                ratios.insert(format!("{metric}/{}", ENGINES[engine]), json!(ratio));
                if ratio > 1.0 - WIN_MARGIN {
                    losses.push(format!("{metric} vs {} {:.2}x", ENGINES[engine], ratio));
                }
            }
        }
        json!({
            "engines": ENGINES.iter().zip(&values).map(|(name, value)| (name.to_string(), value.clone()))
                .collect::<serde_json::Map<_, _>>(),
            "quench_ratio": ratios,
            "below_margin": losses,
        })
    };
    let host = json!({
        "cpus": thread::available_parallelism().map_or(0, |jobs| jobs.get()),
        "cpu_model": fs::read_to_string("/proc/cpuinfo").ok().and_then(|info| info.lines()
            .find(|line| line.starts_with("model name")).map(|line| line.split(':').nth(1).unwrap_or("").trim().to_owned())),
        "memory": fs::read_to_string("/proc/meminfo").ok().and_then(|info| info.lines().next().map(str::to_owned)),
    });
    json!({
        "suite": match options.suite { Suite::Test262 => "test262", Suite::Wasm => "wasm" },
        "mode": options.mode.name(),
        "jobs": options.jobs,
        "timeout_ms": options.timeout.as_millis(),
        "every": options.every,
        "elapsed_s": elapsed.as_secs_f64(),
        "excluded_cases": excluded,
        "host": host,
        "versions": {
            "quench": git_head(), "quench_binary": options.quench,
            "node": version(&options.node), "bun": version(&options.bun),
        },
        "win_margin": WIN_MARGIN,
        "overall": compare(&overall),
        "areas": areas.iter().map(|(area, totals)| (area.to_string(), compare(totals))).collect::<serde_json::Map<_, _>>(),
    })
}

fn version(binary: &Path) -> String {
    Command::new(binary)
        .arg("--version")
        .env_remove("BUN_OPTIONS")
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_default()
}

fn git_head() -> String {
    Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_default()
}

fn markdown(summary: &Value) -> String {
    let mut text = format!(
        "# {} {} — Quench vs Node vs Bun\n\nhost: {} CPUs, {}; jobs {}; elapsed {:.0}s; excluded {} cases\n\
         versions: quench {} · node {} · bun {}\n\n",
        summary["suite"].as_str().unwrap_or(""),
        summary["mode"].as_str().unwrap_or(""),
        summary["host"]["cpus"],
        summary["host"]["cpu_model"].as_str().unwrap_or(""),
        summary["jobs"],
        summary["elapsed_s"].as_f64().unwrap_or(0.0),
        summary["excluded_cases"],
        summary["versions"]["quench"].as_str().unwrap_or(""),
        summary["versions"]["node"].as_str().unwrap_or(""),
        summary["versions"]["bun"].as_str().unwrap_or(""),
    );
    text.push_str("| engine | cases | passed | timeouts | CPU s | wall s | RSS median MiB | RSS p95 MiB | RSS max MiB |\n|---|---|---|---|---|---|---|---|---|\n");
    for engine in ENGINES {
        let totals = &summary["overall"]["engines"][engine];
        text.push_str(&format!(
            "| {engine} | {} | {} | {} | {:.1} | {:.1} | {:.1} | {:.1} | {:.1} |\n",
            totals["cases"], totals["passed"], totals["timeouts"],
            totals["cpu_s"].as_f64().unwrap_or(0.0), totals["wall_s"].as_f64().unwrap_or(0.0),
            totals["rss_median_mib"].as_f64().unwrap_or(0.0), totals["rss_p95_mib"].as_f64().unwrap_or(0.0),
            totals["rss_max_mib"].as_f64().unwrap_or(0.0),
        ));
    }
    text.push_str("\nQuench / other (lower is better; a win needs <= 0.80):\n\n| metric | vs node | vs bun |\n|---|---|---|\n");
    for metric in METRICS {
        let ratio = |engine: &str| summary["overall"]["quench_ratio"][format!("{metric}/{engine}")].as_f64().unwrap_or(0.0);
        text.push_str(&format!("| {metric} | {:.2} | {:.2} |\n", ratio("node"), ratio("bun")));
    }
    let losing = summary["areas"]
        .as_object()
        .map(|areas| areas.iter().filter(|(_, area)| area["below_margin"].as_array().is_some_and(|l| !l.is_empty())).collect::<Vec<_>>())
        .unwrap_or_default();
    text.push_str(&format!(
        "\nAreas where Quench is not 20% better than every engine on every metric: {} of {}\n\n",
        losing.len(),
        summary["areas"].as_object().map_or(0, |areas| areas.len())
    ));
    text.push_str("| area | cases | short of the margin |\n|---|---|---|\n");
    for (area, value) in losing {
        let reasons = value["below_margin"].as_array().unwrap().iter().filter_map(Value::as_str).collect::<Vec<_>>().join("; ");
        text.push_str(&format!("| {area} | {} | {reasons} |\n", value["engines"]["quench"]["cases"]));
    }
    text
}
