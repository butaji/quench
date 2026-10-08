mod analysis;

use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

const FIXTURES: &[&str] = &[
    "crypto.js",
    "deltablue.js",
    "earley-boyer.js",
    "navier-stokes.js",
    "raytrace.js",
    "regexp.js",
    "richards.js",
    "splay.js",
];
const MIN_QUALIFYING_ROUNDS: usize = 11;
const DEFAULT_TIMEOUT_MS: u64 = 300_000;
const MEASUREMENT_ENV: &[&str] = &["PATH", "HOME", "TMPDIR", "LANG", "LC_ALL", "TZ"];
const RUNNER: &str = r#"
let __quenchBenchSucceeded = true;
const __quenchBenchPrint = typeof console !== "undefined" && typeof console.log === "function"
  ? console.log.bind(console)
  : print;
BenchmarkSuite.RunSuites({
  NotifyResult(name, result) { __quenchBenchPrint("__quenchBenchResult: " + name + ": " + result); },
  NotifyError(name, error) { __quenchBenchSucceeded = false; __quenchBenchPrint("__quenchBenchError: " + name + ": " + error); },
  NotifyScore(score) {
    if (__quenchBenchSucceeded) {
      __quenchBenchPrint("----");
      __quenchBenchPrint("Score: " + score);
    }
  },
});
"#;
const SUITE_DIR: &str = "quench-bench/js-engine-benchmark/v8-v7";

#[derive(Clone)]
struct EngineSpec {
    name: &'static str,
    executable: PathBuf,
    argv: Vec<String>,
    env: BTreeMap<String, String>,
    version: String,
    jit_mode: &'static str,
    jit_proof_command: Vec<String>,
    jit_proof: String,
    executable_sha256: String,
}

#[derive(Serialize)]
struct EngineRecord {
    name: &'static str,
    executable: String,
    executable_sha256: String,
    version: String,
    argv: Vec<String>,
    environment: BTreeMap<String, String>,
    inherits_environment: bool,
    jit_mode: &'static str,
    jit_proof_command: Vec<String>,
    jit_proof: String,
}

#[derive(Serialize)]
struct Artifact {
    path: String,
    size_bytes: Option<u64>,
    modified_unix_ns: Option<u128>,
    sha256: Option<String>,
}

#[derive(Serialize)]
struct EngineSummary {
    median_score: Option<f64>,
    median_max_rss_bytes: Option<u64>,
    valid_samples: usize,
}

#[derive(Serialize)]
struct Sample {
    status: i32,
    timed_out: bool,
    wall_ns: u128,
    peak_rss_bytes: Option<u64>,
    score: Option<f64>,
    instructions: Option<u64>,
    cycles: Option<u64>,
    page_faults: Option<u64>,
    page_reclaims: Option<u64>,
    involuntary_context_switches: Option<u64>,
    stdout: String,
    stderr: String,
}

impl Sample {
    fn valid(&self) -> bool {
        self.status == 0
            && !self.timed_out
            && self.score.is_some_and(f64::is_finite)
            && self.peak_rss_bytes.is_some_and(|rss| rss > 0)
    }
}

#[derive(Serialize)]
struct RoundRecord {
    round: usize,
    execution_order: Vec<String>,
    samples: BTreeMap<String, Sample>,
}

#[derive(Serialize)]
struct FixtureRecord {
    source: Artifact,
    valid: bool,
    output_equal: bool,
    summaries: BTreeMap<String, EngineSummary>,
    rounds: Vec<RoundRecord>,
}

#[derive(Serialize)]
struct SuiteRecord {
    schema: u32,
    created_unix_ns: u128,
    rounds_requested: usize,
    timeout_ms: u64,
    source_revision: String,
    source_dirty: bool,
    corpus: CorpusRecord,
    host: HostRecord,
    engines: Vec<EngineRecord>,
    suite_inputs: Vec<Artifact>,
    fixtures: BTreeMap<String, FixtureRecord>,
    complete: bool,
    qualification_ready: bool,
}

#[derive(Serialize)]
struct CorpusRecord {
    pinned_revision: Option<String>,
    checkout_revision: Option<String>,
    checkout_clean: bool,
    fixture_set_matches: bool,
}

#[derive(Serialize)]
struct HostRecord {
    uname: String,
    rustc: String,
    model: Option<String>,
    memory_bytes: Option<u64>,
    memory_limit_bytes: Option<u64>,
    cpu_quota: Option<String>,
}

struct Options {
    fixture: Option<PathBuf>,
    all: bool,
    preflight_only: bool,
    node: PathBuf,
    bun: PathBuf,
    qjs: PathBuf,
    quench: PathBuf,
    rounds: usize,
    timeout_ms: u64,
    output: Option<PathBuf>,
}

fn main() {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.first().is_some_and(|arg| arg == "--analyze") {
        if let Err(error) = analysis::run(&args[1..]) {
            eprintln!("{error}");
            std::process::exit(2);
        }
        return;
    }
    let options = parse_options();
    if env::var_os("QUENCH_EXEC_TRACE").is_some() {
        fail("scored runs must not inherit QUENCH_EXEC_TRACE");
    }
    let engines = prepare_engines(&options);
    let engine_records = engines.iter().map(EngineSpec::record).collect::<Vec<_>>();
    if options.preflight_only {
        println!("{}", serde_json::to_string_pretty(&engine_records).unwrap());
        return;
    }

    let files = selected_fixtures(&options);
    let inputs = corpus_inputs();
    let pinned = corpus_record(&files);
    let mut fixture_records = BTreeMap::new();
    for file in files {
        let fixture = run_fixture(&file, &engines, options.rounds, options.timeout_ms);
        eprintln!(
            "{}: {} ({}/{} rounds)",
            file.display(),
            if fixture.valid { "valid" } else { "incomplete" },
            fixture
                .rounds
                .iter()
                .filter(|round| round.samples.values().all(Sample::valid))
                .count(),
            options.rounds
        );
        fixture_records.insert(file.display().to_string(), fixture);
    }

    let complete =
        !fixture_records.is_empty() && fixture_records.values().all(|fixture| fixture.valid);
    let (revision, source_dirty) = source_identity();
    let host = host_identity();
    let qualification_ready = complete
        && options.rounds >= MIN_QUALIFYING_ROUNDS
        && pinned.fixture_set_matches
        && pinned.checkout_clean
        && pinned.pinned_revision == pinned.checkout_revision
        && !source_dirty;
    let report = SuiteRecord {
        schema: 4,
        created_unix_ns: now_ns(),
        rounds_requested: options.rounds,
        timeout_ms: options.timeout_ms,
        source_revision: revision,
        source_dirty,
        corpus: pinned,
        host,
        engines: engine_records,
        suite_inputs: inputs,
        fixtures: fixture_records,
        complete,
        qualification_ready,
    };
    let bytes = serde_json::to_vec_pretty(&report).unwrap();
    if let Some(path) = options.output {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap_or_else(|error| fail(&format!("cannot create {}: {error}", path.display())));
        file.write_all(&bytes).expect("write benchmark report");
        println!("wrote {}", path.display());
    } else {
        println!("{}", String::from_utf8(bytes).unwrap());
    }
    if !complete {
        std::process::exit(1);
    }
}

impl EngineSpec {
    fn record(&self) -> EngineRecord {
        EngineRecord {
            name: self.name,
            executable: self.executable.display().to_string(),
            executable_sha256: self.executable_sha256.clone(),
            version: self.version.clone(),
            argv: self.argv.clone(),
            environment: self.env.clone(),
            inherits_environment: false,
            jit_mode: self.jit_mode,
            jit_proof_command: self.jit_proof_command.clone(),
            jit_proof: self.jit_proof.clone(),
        }
    }
}

fn parse_options() -> Options {
    let mut args = env::args().skip(1);
    let first = args
        .next()
        .unwrap_or_else(|| usage("missing fixture or --all"));
    let mut options = Options {
        fixture: None,
        all: first == "--all",
        preflight_only: first == "--preflight-only",
        node: "node".into(),
        bun: "bun".into(),
        qjs: "qjs".into(),
        quench: "target/production/quench-node".into(),
        rounds: MIN_QUALIFYING_ROUNDS,
        timeout_ms: DEFAULT_TIMEOUT_MS,
        output: None,
    };
    if !options.all && !options.preflight_only {
        options.fixture = Some(PathBuf::from(first));
    }
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--node" => options.node = required_path(&mut args, "--node"),
            "--bun" => options.bun = required_path(&mut args, "--bun"),
            "--qjs" => options.qjs = required_path(&mut args, "--qjs"),
            "--quench" => options.quench = required_path(&mut args, "--quench"),
            "--runs" => {
                options.rounds = usize::try_from(required_number(&mut args, "--runs"))
                    .unwrap_or_else(|_| usage("--runs is too large"));
                if options.rounds == 0 {
                    usage("--runs must be positive");
                }
            }
            "--timeout-ms" => {
                options.timeout_ms = required_number(&mut args, "--timeout-ms");
                if options.timeout_ms == 0 {
                    usage("--timeout-ms must be positive");
                }
            }
            "--out" => options.output = Some(required_path(&mut args, "--out")),
            "--help" | "-h" => usage(""),
            _ => usage(&format!("unknown argument: {arg}")),
        }
    }
    if options.preflight_only && options.fixture.is_some() {
        usage("--preflight-only does not take a fixture");
    }
    options
}

fn required_path(args: &mut impl Iterator<Item = String>, option: &str) -> PathBuf {
    args.next()
        .map(PathBuf::from)
        .unwrap_or_else(|| usage(&format!("missing value for {option}")))
}

fn required_number(args: &mut impl Iterator<Item = String>, option: &str) -> u64 {
    args.next()
        .and_then(|value| value.parse().ok())
        .unwrap_or_else(|| usage(&format!("invalid value for {option}")))
}

fn prepare_engines(options: &Options) -> Vec<EngineSpec> {
    let mut node = spec(
        "node",
        &options.node,
        vec!["--jitless".into()],
        BTreeMap::new(),
    );
    node.version = version(&node.executable, &["--version"], &node.env);
    node.jit_proof_command = vec![
        node.executable.display().to_string(),
        "--jitless".into(),
        "-e".into(),
        "console.log(process.execArgv.includes('--jitless'))".into(),
    ];
    let proof = capture(
        &node.executable,
        &[
            "--jitless",
            "-e",
            "console.log(process.execArgv.includes('--jitless'))",
        ],
        &node.env,
    );
    node.jit_proof = require_proof("node --jitless", &proof, "true");

    let mut bun_env = BTreeMap::new();
    bun_env.insert("BUN_JSC_useJIT".into(), "0".into());
    let mut bun = spec("bun", &options.bun, vec![], bun_env);
    bun.version = version(&bun.executable, &["--version"], &bun.env);
    let mut proof_env = bun.env.clone();
    proof_env.insert("BUN_JSC_dumpOptions".into(), "1".into());
    let proof = capture(
        &bun.executable,
        &["-e", "console.log(Bun.version)"],
        &proof_env,
    );
    bun.jit_proof_command = vec![
        "BUN_JSC_useJIT=0".into(),
        "BUN_JSC_dumpOptions=1".into(),
        bun.executable.display().to_string(),
        "-e".into(),
        "console.log(Bun.version)".into(),
    ];
    bun.jit_proof = require_proof("BUN_JSC_useJIT=0 + JSC option dump", &proof, "useJIT=false");

    let mut qjs = spec("quickjs", &options.qjs, vec![], BTreeMap::new());
    qjs.version = version(&qjs.executable, &["--version"], &qjs.env);
    qjs.jit_proof_command = vec![qjs.executable.display().to_string(), "--version".into()];
    qjs.jit_proof =
        "QuickJS qjs executes bytecode in its interpreter; binary version and SHA-256 are recorded"
            .into();

    let mut quench = spec("quench", &options.quench, vec![], BTreeMap::new());
    quench.version = format!(
        "Quench shared-VM interpreter build; package {}",
        env!("CARGO_PKG_VERSION")
    );
    quench.jit_proof_command = vec![quench.executable.display().to_string()];
    quench.jit_proof =
        "the measured shared VM executes interpreter bytecode; no guest JIT path is enabled".into();

    vec![quench, qjs, bun, node]
}

fn spec(
    name: &'static str,
    path: &Path,
    argv: Vec<String>,
    env: BTreeMap<String, String>,
) -> EngineSpec {
    let executable = resolve_executable(path).unwrap_or_else(|error| fail(&error));
    let sha = sha256(&executable)
        .unwrap_or_else(|| fail(&format!("cannot hash {}", executable.display())));
    EngineSpec {
        name,
        executable,
        argv,
        env: measurement_environment(env),
        version: String::new(),
        jit_mode: match name {
            "node" | "bun" => "disabled-and-probed",
            _ => "interpreter",
        },
        jit_proof_command: Vec::new(),
        jit_proof: String::new(),
        executable_sha256: sha,
    }
}

fn measurement_environment(overrides: BTreeMap<String, String>) -> BTreeMap<String, String> {
    let inherited = MEASUREMENT_ENV
        .iter()
        .filter_map(|key| env::var(key).ok().map(|value| ((*key).to_string(), value)))
        .collect::<BTreeMap<_, _>>();
    inherited.into_iter().chain(overrides).collect()
}

fn resolve_executable(path: &Path) -> Result<PathBuf, String> {
    if path.components().count() > 1 {
        return path
            .canonicalize()
            .map_err(|error| format!("cannot resolve {}: {error}", path.display()));
    }
    env::split_paths(&env::var_os("PATH").unwrap_or_default())
        .map(|dir| dir.join(path))
        .find(|candidate| candidate.is_file())
        .and_then(|candidate| candidate.canonicalize().ok())
        .ok_or_else(|| format!("executable not found: {}", path.display()))
}

fn version(executable: &Path, args: &[&str], envs: &BTreeMap<String, String>) -> String {
    let output = capture(executable, args, envs);
    let transcript = format!("{}\n{}", output.stdout, output.stderr);
    let quickjs_version = transcript
        .lines()
        .find(|line| line.contains("QuickJS version"));
    if output.timed_out || (output.status != 0 && quickjs_version.is_none()) {
        fail(&format!(
            "version probe failed for {}",
            executable.display()
        ));
    }
    transcript
        .lines()
        .find(|line| line.contains("QuickJS version"))
        .or_else(|| transcript.lines().find(|line| !line.trim().is_empty()))
        .unwrap_or("version unavailable")
        .trim()
        .to_string()
}

fn require_proof(name: &str, output: &Capture, expected: &str) -> String {
    let transcript = format!("{}\n{}", output.stdout, output.stderr);
    if output.status != 0 || output.timed_out || !transcript.contains(expected) {
        fail(&format!("JIT-off proof failed for {name}: {transcript}"));
    }
    format!(
        "command succeeded and reported {expected:?}: {}",
        transcript.trim()
    )
}

struct Capture {
    status: i32,
    timed_out: bool,
    stdout: String,
    stderr: String,
}

fn capture(executable: &Path, args: &[&str], envs: &BTreeMap<String, String>) -> Capture {
    let output = Command::new(executable)
        .args(args)
        .env_clear()
        .envs(envs)
        .output();
    capture_output(output)
}

fn capture_output(output: std::io::Result<Output>) -> Capture {
    match output {
        Ok(output) => Capture {
            status: output.status.code().unwrap_or(-1),
            timed_out: false,
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        },
        Err(error) => Capture {
            status: -1,
            timed_out: false,
            stdout: String::new(),
            stderr: error.to_string(),
        },
    }
}

fn selected_fixtures(options: &Options) -> Vec<PathBuf> {
    if options.all {
        return FIXTURES
            .iter()
            .map(|name| Path::new(SUITE_DIR).join(name))
            .collect();
    }
    vec![options.fixture.clone().unwrap()]
}

fn run_fixture(
    file: &Path,
    engines: &[EngineSpec],
    rounds: usize,
    timeout_ms: u64,
) -> FixtureRecord {
    let temporary = materialize(file);
    let mut results = Vec::with_capacity(rounds);
    for round in 0..rounds {
        let order = rotated_order(engines, round);
        let mut samples = BTreeMap::new();
        for engine in &order {
            samples.insert(engine.name.to_string(), run(engine, &temporary, timeout_ms));
        }
        results.push(RoundRecord {
            round,
            execution_order: order.iter().map(|engine| engine.name.to_string()).collect(),
            samples,
        });
        if !results.last().unwrap().samples.values().all(Sample::valid) {
            break;
        }
    }
    let _ = fs::remove_file(temporary);
    let valid = results.len() == rounds
        && results
            .iter()
            .all(|round| round.samples.values().all(Sample::valid));
    let output_equal = outputs_equal(&results, engines);
    let summaries = engines
        .iter()
        .map(|engine| (engine.name.to_string(), summary(&results, engine.name)))
        .collect();
    FixtureRecord {
        source: artifact(file),
        valid,
        output_equal,
        summaries,
        rounds: results,
    }
}

fn rotated_order(engines: &[EngineSpec], round: usize) -> Vec<EngineSpec> {
    (0..engines.len())
        .map(|offset| engines[(offset + round) % engines.len()].clone())
        .collect()
}

fn outputs_equal(rounds: &[RoundRecord], engines: &[EngineSpec]) -> bool {
    rounds.iter().all(|round| {
        let Some(first) = engines
            .first()
            .and_then(|engine| round.samples.get(engine.name))
        else {
            return false;
        };
        engines.iter().all(|engine| {
            round.samples.get(engine.name).is_some_and(|sample| {
                sample.status == first.status
                    && semantic_output(&sample.stdout) == semantic_output(&first.stdout)
            })
        })
    })
}

fn run(engine: &EngineSpec, source: &Path, timeout_ms: u64) -> Sample {
    let started = Instant::now();
    let mut command = Command::new(&engine.executable);
    command
        .args(&engine.argv)
        .arg(source)
        .env_clear()
        .envs(&engine.env)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => return invalid_sample(started, error.to_string()),
    };
    let stdout_reader = child.stdout.take().expect("piped stdout");
    let stderr_reader = child.stderr.take().expect("piped stderr");
    let stdout_thread = std::thread::spawn(move || read_pipe(stdout_reader));
    let stderr_thread = std::thread::spawn(move || read_pipe(stderr_reader));
    let waited = wait_child(child.id(), timeout_ms);
    // `wait4` reaps the process directly to return its resource counters.
    // Child has no Drop behavior that waits or kills a reaped process.
    drop(child);
    let stdout_result = stdout_thread.join();
    let stderr_result = stderr_thread.join();
    let (status, timed_out, usage) = match waited {
        Ok(result) => result,
        Err(error) => {
            return invalid_sample(started, format!("waiting for engine failed: {error}"));
        }
    };
    let (stdout, stdout_error) = match stdout_result {
        Ok(result) => result,
        Err(_) => return invalid_sample(started, "stdout reader panicked".into()),
    };
    let (stderr, stderr_error) = match stderr_result {
        Ok(result) => result,
        Err(_) => return invalid_sample(started, "stderr reader panicked".into()),
    };
    let mut stderr = String::from_utf8_lossy(&stderr).into_owned();
    if let Some(error) = stdout_error {
        stderr.push_str(&format!("\nstdout read failed: {error}"));
    }
    if let Some(error) = stderr_error {
        stderr.push_str(&format!("\nstderr read failed: {error}"));
    }
    let stdout = String::from_utf8_lossy(&stdout).into_owned();
    let score = stdout
        .lines()
        .find_map(|line| line.strip_prefix("Score: "))
        .and_then(|value| value.parse().ok());
    Sample {
        status,
        timed_out,
        wall_ns: started.elapsed().as_nanos(),
        peak_rss_bytes: usage.peak_rss_bytes,
        score,
        instructions: None,
        cycles: None,
        page_faults: Some(usage.page_faults),
        page_reclaims: Some(usage.page_reclaims),
        involuntary_context_switches: Some(usage.involuntary_context_switches),
        stdout,
        stderr,
    }
}

fn read_pipe(mut reader: impl Read) -> (Vec<u8>, Option<String>) {
    let mut output = Vec::new();
    let error = reader
        .read_to_end(&mut output)
        .err()
        .map(|error| error.to_string());
    (output, error)
}

struct ProcessUsage {
    peak_rss_bytes: Option<u64>,
    page_faults: u64,
    page_reclaims: u64,
    involuntary_context_switches: u64,
}

fn wait_child(child_id: u32, timeout_ms: u64) -> std::io::Result<(i32, bool, ProcessUsage)> {
    use std::{os::unix::process::ExitStatusExt, time::Duration};

    let pid = libc::pid_t::try_from(child_id).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "child process id is out of range",
        )
    })?;
    let timeout = Duration::from_millis(timeout_ms);
    let started = Instant::now();
    let mut timed_out = false;
    let mut sent_term_at = None;
    loop {
        let mut status = 0;
        // SAFETY: wait4 writes its status and usage outputs to these live values.
        let mut raw_usage: libc::rusage = unsafe { std::mem::zeroed() };
        let result = unsafe { libc::wait4(pid, &mut status, libc::WNOHANG, &mut raw_usage) };
        if result == pid {
            let peak_rss_bytes = peak_rss_bytes(raw_usage.ru_maxrss);
            let usage = ProcessUsage {
                peak_rss_bytes,
                page_faults: nonnegative(raw_usage.ru_majflt)
                    .saturating_add(nonnegative(raw_usage.ru_minflt)),
                page_reclaims: nonnegative(raw_usage.ru_minflt),
                involuntary_context_switches: nonnegative(raw_usage.ru_nivcsw),
            };
            let exit_status = std::process::ExitStatus::from_raw(status);
            return Ok((exit_status.code().unwrap_or(-1), timed_out, usage));
        }
        if result == -1 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }

        if !timed_out && started.elapsed() >= timeout {
            timed_out = true;
            sent_term_at = Some(Instant::now());
            // SAFETY: `pid` is the process-group ID established before spawn.
            unsafe { libc::kill(-pid, libc::SIGTERM) };
        } else if let Some(term_at) = sent_term_at {
            if term_at.elapsed() >= Duration::from_secs(1) {
                // SAFETY: terminate any process in this benchmark's process group.
                unsafe { libc::kill(-pid, libc::SIGKILL) };
                sent_term_at = None;
            }
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn nonnegative(value: libc::c_long) -> u64 {
    u64::try_from(value).unwrap_or(0)
}

fn peak_rss_bytes(raw_rss: libc::c_long) -> Option<u64> {
    let rss = u64::try_from(raw_rss).ok()?;
    if cfg!(target_os = "linux") {
        rss.checked_mul(1024)
    } else {
        Some(rss)
    }
}

fn invalid_sample(started: Instant, error: String) -> Sample {
    Sample {
        status: -1,
        timed_out: false,
        wall_ns: started.elapsed().as_nanos(),
        peak_rss_bytes: None,
        score: None,
        instructions: None,
        cycles: None,
        page_faults: None,
        page_reclaims: None,
        involuntary_context_switches: None,
        stdout: String::new(),
        stderr: error,
    }
}

fn summary(rounds: &[RoundRecord], engine: &str) -> EngineSummary {
    let samples = rounds
        .iter()
        .filter_map(|round| round.samples.get(engine))
        .collect::<Vec<_>>();
    let scores = samples
        .iter()
        .filter_map(|sample| sample.score)
        .filter(|score| score.is_finite())
        .collect();
    let rss = samples
        .iter()
        .filter_map(|sample| sample.peak_rss_bytes)
        .collect();
    EngineSummary {
        median_score: median_f64(scores),
        median_max_rss_bytes: median_u64(rss),
        valid_samples: samples.into_iter().filter(|sample| sample.valid()).count(),
    }
}

fn median_f64(mut values: Vec<f64>) -> Option<f64> {
    values.sort_by(f64::total_cmp);
    values.get(values.len() / 2).copied()
}

fn median_u64(mut values: Vec<u64>) -> Option<u64> {
    values.sort_unstable();
    values.get(values.len() / 2).copied()
}

fn materialize(file: &Path) -> PathBuf {
    let nonce = now_ns();
    let name = file.file_name().unwrap_or_default().to_string_lossy();
    let path = Path::new("/tmp").join(format!(
        "quench-v8-v7-{}-{nonce}-{name}",
        std::process::id()
    ));
    let base = fs::read(Path::new(SUITE_DIR).join("base.js"))
        .unwrap_or_else(|error| fail(&error.to_string()));
    let fixture = fs::read(file).unwrap_or_else(|error| fail(&error.to_string()));
    let mut source = Vec::with_capacity(base.len() + fixture.len() + RUNNER.len() + 2);
    source.extend_from_slice(&base);
    source.push(b'\n');
    source.extend_from_slice(&fixture);
    source.push(b'\n');
    source.extend_from_slice(RUNNER.as_bytes());
    fs::write(&path, source).unwrap_or_else(|error| fail(&error.to_string()));
    path
}

fn semantic_output(stdout: &str) -> String {
    stdout
        .lines()
        .filter(|line| {
            !line.starts_with("Score: ")
                && *line != "----"
                && !line.starts_with("__quenchBenchResult: ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn corpus_inputs() -> Vec<Artifact> {
    std::iter::once(Path::new(SUITE_DIR).join("base.js"))
        .chain(FIXTURES.iter().map(|name| Path::new(SUITE_DIR).join(name)))
        .map(|path| artifact(&path))
        .collect()
}

fn corpus_record(selected: &[PathBuf]) -> CorpusRecord {
    let pinned_revision = command_output(
        "git",
        &["ls-tree", "HEAD", "--", "quench-bench/js-engine-benchmark"],
    )
    .split_whitespace()
    .nth(2)
    .map(str::to_string);
    let checkout_revision = command_output(
        "git",
        &[
            "-C",
            "quench-bench/js-engine-benchmark",
            "rev-parse",
            "HEAD",
        ],
    )
    .lines()
    .next()
    .map(str::to_string);
    let checkout_clean = command_output(
        "git",
        &[
            "-C",
            "quench-bench/js-engine-benchmark",
            "status",
            "--porcelain",
        ],
    )
    .is_empty();
    let expected = FIXTURES
        .iter()
        .map(|name| Path::new(SUITE_DIR).join(name))
        .collect::<BTreeSet<_>>();
    let actual = selected.iter().cloned().collect::<BTreeSet<_>>();
    CorpusRecord {
        pinned_revision,
        checkout_revision,
        checkout_clean,
        fixture_set_matches: expected == actual,
    }
}

fn source_identity() -> (String, bool) {
    let revision = command_output("git", &["rev-parse", "HEAD"])
        .lines()
        .next()
        .unwrap_or("")
        .to_string();
    let dirty =
        !command_output("git", &["status", "--porcelain", "--untracked-files=all"]).is_empty();
    (revision, dirty)
}

fn host_identity() -> HostRecord {
    let model = fs::read_to_string("/sys/devices/virtual/dmi/id/product_name")
        .ok()
        .map(|value| value.trim().to_string())
        .or_else(|| {
            command_output("sysctl", &["-n", "hw.model"])
                .lines()
                .next()
                .map(str::to_string)
        });
    let memory_bytes = host_memory_bytes();
    HostRecord {
        uname: command_output("uname", &["-a"]).trim().to_string(),
        rustc: command_output("rustc", &["-Vv"]).trim().to_string(),
        model,
        memory_bytes,
        memory_limit_bytes: cgroup_memory_limit_bytes(),
        cpu_quota: fs::read_to_string("/sys/fs/cgroup/cpu.max")
            .ok()
            .map(|value| value.trim().to_string()),
    }
}

fn host_memory_bytes() -> Option<u64> {
    if cfg!(target_os = "linux") {
        return fs::read_to_string("/proc/meminfo")
            .ok()?
            .lines()
            .find_map(|line| line.strip_prefix("MemTotal:"))?
            .split_whitespace()
            .next()?
            .parse::<u64>()
            .ok()?
            .checked_mul(1024);
    }
    command_output("sysctl", &["-n", "hw.memsize"])
        .trim()
        .parse()
        .ok()
}

fn cgroup_memory_limit_bytes() -> Option<u64> {
    fs::read_to_string("/sys/fs/cgroup/memory.max")
        .ok()?
        .trim()
        .parse()
        .ok()
}

fn artifact(path: &Path) -> Artifact {
    let metadata = fs::metadata(path).ok();
    let modified_unix_ns = metadata
        .as_ref()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos());
    Artifact {
        path: path.display().to_string(),
        size_bytes: metadata.map(|metadata| metadata.len()),
        modified_unix_ns,
        sha256: sha256(path),
    }
}

pub(crate) fn sha256(path: &Path) -> Option<String> {
    let output = Command::new("shasum")
        .args(["-a", "256"])
        .arg(path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .next()
        .map(str::to_string)
}

fn command_output(program: &str, args: &[&str]) -> String {
    Command::new(program)
        .args(args)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .unwrap_or_default()
}

pub(crate) fn now_ns() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos())
}

fn usage(message: &str) -> ! {
    eprintln!(
        "{message}\nusage: quench-bench <fixture.js>|--all [--quench PATH] [--qjs PATH] [--bun PATH] [--node PATH] [--runs N] [--timeout-ms N] [--out PATH]\n       quench-bench --preflight-only [engine options]\n       quench-bench --analyze REPORT [--out JSON]"
    );
    std::process::exit(2)
}

fn fail(message: &str) -> ! {
    eprintln!("{message}");
    std::process::exit(2)
}

#[cfg(test)]
mod tests {
    use super::{peak_rss_bytes, wait_child};
    use std::process::Command;

    #[test]
    fn converts_wait4_peak_rss_to_bytes() {
        let raw_rss = 1234;
        let expected: u64 = if cfg!(target_os = "linux") {
            1_263_616
        } else {
            raw_rss as u64
        };
        assert_eq!(peak_rss_bytes(raw_rss), Some(expected));
    }

    #[test]
    fn wait4_reports_exit_status_and_resource_usage() {
        let mut command = Command::new("sh");
        command.arg("-c").arg("exit 7");
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut command, 0);
        let child = command.spawn().expect("spawn shell");
        let (status, timed_out, usage) = wait_child(child.id(), 5000).expect("wait4 child");
        assert_eq!(status, 7);
        assert!(!timed_out);
        assert!(usage.peak_rss_bytes.is_some_and(|rss| rss > 0));
    }

    #[test]
    fn timeout_terminates_the_process_group() {
        let mut command = Command::new("sh");
        command.arg("-c").arg("sleep 5");
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut command, 0);
        let child = command.spawn().expect("spawn shell");
        let (status, timed_out, _) = wait_child(child.id(), 5).expect("wait4 child");
        assert_eq!(status, -1);
        assert!(timed_out);
    }
}
