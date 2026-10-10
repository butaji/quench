mod analysis;
mod contention;

use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{Instant, SystemTime, UNIX_EPOCH},
};
#[cfg(all(unix, not(target_os = "macos")))]
use std::{io::Read, process::Stdio};

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
const FIXED_WORK_REPORT_SCHEMA: u32 = 4;
const FIXED_WORK_MEASUREMENT_MODE: &str = "fixed_work_diagnostic";
const FIXED_WORK_MARKER_PREFIX: &str = "__quenchFixedWork:";
const FIXED_WORK_SUITE_COUNT: usize = 1;
const FIXED_WORK_MARKER_COUNT: usize = 1;
const FIXED_WORK_SAMPLE_ORDER_PERIOD: usize = 2;
const HOST_CPU_CONSUMER_LIMIT: usize = 5;
const FIXED_WORK_PLANS: &[FixedWorkPlan] = &[
    FixedWorkPlan {
        fixture: "crypto.js",
        suite: "Crypto",
        benchmark_count: 2,
        iterations_per_benchmark: 4,
    },
    FixedWorkPlan {
        fixture: "deltablue.js",
        suite: "DeltaBlue",
        benchmark_count: 1,
        iterations_per_benchmark: 40,
    },
    FixedWorkPlan {
        fixture: "earley-boyer.js",
        suite: "EarleyBoyer",
        benchmark_count: 2,
        iterations_per_benchmark: 3,
    },
    FixedWorkPlan {
        fixture: "navier-stokes.js",
        suite: "NavierStokes",
        benchmark_count: 1,
        iterations_per_benchmark: 5,
    },
    FixedWorkPlan {
        fixture: "raytrace.js",
        suite: "RayTrace",
        benchmark_count: 1,
        iterations_per_benchmark: 10,
    },
    FixedWorkPlan {
        fixture: "regexp.js",
        suite: "RegExp",
        benchmark_count: 1,
        iterations_per_benchmark: 1,
    },
    FixedWorkPlan {
        fixture: "richards.js",
        suite: "Richards",
        benchmark_count: 1,
        iterations_per_benchmark: 100,
    },
    FixedWorkPlan {
        fixture: "splay.js",
        suite: "Splay",
        benchmark_count: 1,
        iterations_per_benchmark: 300,
    },
];
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

#[derive(Clone, Deserialize, Serialize)]
struct EngineRecord {
    name: String,
    executable: String,
    executable_sha256: String,
    version: String,
    argv: Vec<String>,
    environment: BTreeMap<String, String>,
    inherits_environment: bool,
    jit_mode: String,
    jit_proof_command: Vec<String>,
    jit_proof: String,
}

#[derive(Clone, Deserialize, Serialize)]
struct Artifact {
    path: String,
    size_bytes: Option<u64>,
    modified_unix_ns: Option<u128>,
    sha256: Option<String>,
}

#[derive(Clone, Deserialize, Serialize)]
struct EngineSummary {
    median_score: Option<f64>,
    median_max_rss_bytes: Option<u64>,
    valid_samples: usize,
}

#[derive(Clone, Deserialize, Serialize)]
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
    #[serde(default)]
    host_before: Option<HostSnapshot>,
    #[serde(default)]
    host_after: Option<HostSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    contention: Option<contention::Assessment>,
    stdout: String,
    stderr: String,
}

#[derive(Clone, Deserialize, Serialize)]
struct HostSnapshot {
    captured_unix_ns: u128,
    load_average: Option<LoadAverage>,
    load_average_error: Option<String>,
    top_cpu_consumers: Vec<CpuConsumer>,
    process_list_error: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
struct LoadAverage {
    one_minute: f64,
    five_minutes: f64,
    fifteen_minutes: f64,
}

#[derive(Clone, Deserialize, Serialize)]
struct CpuConsumer {
    pid: u32,
    cpu_percent: f64,
    command: String,
}

impl Sample {
    fn valid(&self) -> bool {
        self.status == 0
            && !self.timed_out
            && self.score.is_some_and(f64::is_finite)
            && self.peak_rss_bytes.is_some_and(|rss| rss > 0)
    }

    fn valid_fixed_work(&self, plan: FixedWorkPlan, iterations: usize) -> bool {
        let marker = fixed_work_marker(plan, iterations);
        let marker_count = self
            .stdout
            .lines()
            .filter(|line| line.starts_with(FIXED_WORK_MARKER_PREFIX))
            .count();
        self.status == 0
            && !self.timed_out
            && self.score.is_none()
            && self.peak_rss_bytes.is_some_and(|rss| rss > 0)
            && marker_count == FIXED_WORK_MARKER_COUNT
            && self.stdout.lines().any(|line| line == marker)
    }
}

#[derive(Clone, Deserialize, Serialize)]
struct RoundRecord {
    round: usize,
    execution_order: Vec<String>,
    samples: BTreeMap<String, Sample>,
}

#[derive(Clone, Deserialize, Serialize)]
struct FixtureRecord {
    source: Artifact,
    valid: bool,
    output_equal: bool,
    summaries: BTreeMap<String, EngineSummary>,
    rounds: Vec<RoundRecord>,
}

#[derive(Clone, Deserialize, Serialize)]
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

#[derive(Clone, Deserialize, Serialize)]
struct CorpusRecord {
    pinned_revision: Option<String>,
    checkout_revision: Option<String>,
    checkout_clean: bool,
    fixture_set_matches: bool,
}

#[derive(Clone, Deserialize, Serialize)]
struct HostRecord {
    uname: String,
    rustc: String,
    model: Option<String>,
    memory_bytes: Option<u64>,
    memory_limit_bytes: Option<u64>,
    cpu_quota: Option<String>,
    #[serde(default)]
    logical_cpus: Option<usize>,
    process_metrics_backend: String,
}

#[derive(Clone, Copy, Serialize)]
struct FixedWorkPlan {
    fixture: &'static str,
    suite: &'static str,
    benchmark_count: usize,
    iterations_per_benchmark: usize,
}

impl FixedWorkPlan {
    fn total_run_calls(self) -> usize {
        self.benchmark_count * self.iterations_per_benchmark
    }
}

#[derive(Clone, Serialize)]
struct FixedWorkSummary {
    work: FixedWorkProcessSummary,
    setup_only: FixedWorkProcessSummary,
    marginal_per_run: FixedWorkPerRunSummary,
}

#[derive(Clone, Serialize)]
struct FixedWorkProcessSummary {
    median_wall_ns: Option<u128>,
    median_cycles: Option<u64>,
    median_instructions: Option<u64>,
    median_max_rss_bytes: Option<u64>,
    valid_samples: usize,
    clean_samples: usize,
}

#[derive(Clone, Serialize)]
struct FixedWorkPerRunSummary {
    median_wall_ns: Option<f64>,
    median_cycles: Option<f64>,
    median_instructions: Option<f64>,
    paired_samples: usize,
    clean_paired_samples: usize,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
enum FixedWorkSampleKind {
    Work,
    SetupOnly,
}

#[derive(Clone, Serialize)]
struct FixedWorkExecution {
    engine: String,
    sample: FixedWorkSampleKind,
}

#[derive(Clone, Serialize)]
struct FixedWorkRound {
    round: usize,
    execution_order: Vec<FixedWorkExecution>,
    #[serde(default)]
    rejected_attempts: Vec<FixedWorkRoundAttempt>,
    selected_attempt: usize,
    samples: BTreeMap<String, Sample>,
    setup_only_samples: BTreeMap<String, Sample>,
}

#[derive(Clone, Serialize)]
struct FixedWorkRoundAttempt {
    attempt: usize,
    execution_order: Vec<FixedWorkExecution>,
    samples: BTreeMap<String, Sample>,
    setup_only_samples: BTreeMap<String, Sample>,
}

#[derive(Clone, Serialize)]
struct FixedWorkFixtureRecord {
    source: Artifact,
    materialized_source: Artifact,
    setup_only_source: Artifact,
    plan: FixedWorkPlan,
    valid: bool,
    output_equal: bool,
    clean_rounds: usize,
    summaries: BTreeMap<String, FixedWorkSummary>,
    rounds: Vec<FixedWorkRound>,
}

#[derive(Clone, Serialize)]
struct FixedWorkReport {
    schema: u32,
    measurement_mode: String,
    created_unix_ns: u128,
    rounds_requested: usize,
    timeout_ms: u64,
    source_revision: String,
    source_dirty: bool,
    corpus: CorpusRecord,
    host: HostRecord,
    contention_policy: contention::Policy,
    measurement_runner: Artifact,
    engines: Vec<EngineRecord>,
    suite_inputs: Vec<Artifact>,
    fixtures: BTreeMap<String, FixedWorkFixtureRecord>,
    complete: bool,
    qualification_ready: bool,
}

struct Options {
    fixture: Option<PathBuf>,
    all: bool,
    preflight_only: bool,
    node: PathBuf,
    bun: PathBuf,
    qjs: PathBuf,
    quench: PathBuf,
    quench_peer: Option<PathBuf>,
    rounds: usize,
    fixed_work: bool,
    timeout_ms: u64,
    output: Option<PathBuf>,
    checkpoint: Option<PathBuf>,
    resume: Option<PathBuf>,
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
    let engines = if options.fixed_work && options.quench_peer.is_some() {
        prepare_quench_pair(&options)
    } else {
        prepare_engines(&options)
    };
    let engine_records = engines.iter().map(EngineSpec::record).collect::<Vec<_>>();
    if options.preflight_only {
        println!("{}", serde_json::to_string_pretty(&engine_records).unwrap());
        return;
    }

    let files = selected_fixtures(&options);
    let (source_revision, source_dirty) = source_identity();
    if options.fixed_work {
        let report = run_fixed_work_report(
            &options,
            &files,
            &engines,
            engine_records,
            source_revision,
            source_dirty,
        );
        let bytes = serde_json::to_vec_pretty(&report).unwrap();
        if let Some(path) = options.output {
            write_new_report(&path, &bytes);
            println!("wrote {}", path.display());
        } else {
            println!("{}", String::from_utf8(bytes).unwrap());
        }
        if !report.complete {
            std::process::exit(1);
        }
        return;
    }

    let mut report = SuiteRecord {
        schema: 5,
        created_unix_ns: now_ns(),
        rounds_requested: options.rounds,
        timeout_ms: options.timeout_ms,
        source_revision,
        source_dirty,
        corpus: corpus_record(&files),
        host: host_identity(),
        engines: engine_records,
        suite_inputs: corpus_inputs(),
        fixtures: BTreeMap::new(),
        complete: false,
        qualification_ready: false,
    };
    if let Some(path) = &options.resume {
        let saved = read_checkpoint(path);
        validate_resume(&saved, &report, &files);
        report = saved;
        eprintln!(
            "resuming {} completed fixture records from {}",
            report.fixtures.len(),
            path.display()
        );
    } else if let Some(path) = &options.checkpoint {
        if path.exists() {
            fail(&format!(
                "checkpoint already exists: {} (use --resume)",
                path.display()
            ));
        }
        write_checkpoint(path, &report);
    }
    let checkpoint_path = options.resume.as_ref().or(options.checkpoint.as_ref());
    for file in files {
        let key = file.display().to_string();
        if report.fixtures.get(&key).is_some_and(|fixture| {
            fixture.valid && fixture.output_equal && fixture.rounds.len() == options.rounds
        }) {
            eprintln!(
                "{}: resumed valid fixture ({} rounds)",
                file.display(),
                options.rounds
            );
            continue;
        }
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
        report.fixtures.insert(key, fixture);
        if let Some(path) = checkpoint_path {
            write_checkpoint(path, &report);
            eprintln!(
                "checkpointed {} fixtures to {}",
                report.fixtures.len(),
                path.display()
            );
        }
    }

    let complete =
        !report.fixtures.is_empty() && report.fixtures.values().all(|fixture| fixture.valid);
    report.complete = complete;
    report.qualification_ready = complete
        && options.rounds >= MIN_QUALIFYING_ROUNDS
        && report.corpus.fixture_set_matches
        && report.corpus.checkout_clean
        && report.corpus.pinned_revision == report.corpus.checkout_revision
        && !report.source_dirty;
    if let Some(path) = checkpoint_path {
        write_checkpoint(path, &report);
    }
    let bytes = serde_json::to_vec_pretty(&report).unwrap();
    if let Some(path) = options.output {
        write_new_report(&path, &bytes);
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
            name: self.name.to_string(),
            executable: self.executable.display().to_string(),
            executable_sha256: self.executable_sha256.clone(),
            version: self.version.clone(),
            argv: self.argv.clone(),
            environment: self.env.clone(),
            inherits_environment: false,
            jit_mode: self.jit_mode.to_string(),
            jit_proof_command: self.jit_proof_command.clone(),
            jit_proof: self.jit_proof.clone(),
        }
    }
}

fn read_checkpoint(path: &Path) -> SuiteRecord {
    let bytes = fs::read(path).unwrap_or_else(|error| {
        fail(&format!(
            "cannot read checkpoint {}: {error}",
            path.display()
        ))
    });
    serde_json::from_slice(&bytes)
        .unwrap_or_else(|error| fail(&format!("invalid checkpoint {}: {error}", path.display())))
}

fn write_checkpoint(path: &Path, report: &SuiteRecord) {
    let bytes = serde_json::to_vec_pretty(report).expect("serialize benchmark checkpoint");
    let mut temporary_name = path.as_os_str().to_os_string();
    temporary_name.push(format!(".tmp-{}-{}", std::process::id(), now_ns()));
    let temporary = PathBuf::from(temporary_name);
    let result = (|| -> std::io::Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)
    })();
    if let Err(error) = result {
        let _ = fs::remove_file(&temporary);
        fail(&format!(
            "cannot write checkpoint {}: {error}",
            path.display()
        ));
    }
}

fn write_new_report(path: &Path, bytes: &[u8]) {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap_or_else(|error| fail(&format!("cannot create {}: {error}", path.display())));
    file.write_all(bytes).expect("write benchmark report");
}

fn validate_resume(saved: &SuiteRecord, expected: &SuiteRecord, files: &[PathBuf]) {
    let metadata_matches = saved.schema == expected.schema
        && saved.rounds_requested == expected.rounds_requested
        && saved.timeout_ms == expected.timeout_ms
        && saved.source_revision == expected.source_revision
        && saved.source_dirty == expected.source_dirty
        && same_json(&saved.corpus, &expected.corpus)
        && same_json(&saved.host, &expected.host)
        && same_json(&saved.engines, &expected.engines)
        && same_json(&saved.suite_inputs, &expected.suite_inputs);
    if !metadata_matches {
        fail(
            "checkpoint provenance does not match this source, host, corpus, engines, rounds, or timeout",
        );
    }
    if saved.source_dirty {
        fail("cannot resume a checkpoint recorded from a dirty source tree");
    }
    let allowed = files
        .iter()
        .map(|file| file.display().to_string())
        .collect::<BTreeSet<_>>();
    if saved.fixtures.keys().any(|key| !allowed.contains(key)) {
        fail("checkpoint contains fixtures outside the selected fixture set");
    }
    for (key, fixture) in &saved.fixtures {
        let file = files
            .iter()
            .find(|file| file.display().to_string() == *key)
            .expect("fixture key validated above");
        if !same_json(&fixture.source, &artifact(file)) {
            fail(&format!("checkpoint fixture input changed: {key}"));
        }
        if fixture.valid && fixture.output_equal {
            let engine_names = saved
                .engines
                .iter()
                .map(|engine| engine.name.as_str())
                .collect::<Vec<_>>();
            let invalid_completed_round = fixture.rounds.len() != saved.rounds_requested
                || fixture.rounds.iter().enumerate().any(|(index, round)| {
                    let expected_order = (0..engine_names.len())
                        .map(|offset| engine_names[(offset + index) % engine_names.len()])
                        .collect::<Vec<_>>();
                    round.round != index
                        || round
                            .execution_order
                            .iter()
                            .map(String::as_str)
                            .collect::<Vec<_>>()
                            != expected_order
                        || round.samples.len() != engine_names.len()
                        || engine_names
                            .iter()
                            .any(|name| !round.samples.get(*name).is_some_and(Sample::valid))
                });
            if invalid_completed_round {
                fail(&format!(
                    "checkpoint claims invalid completed rounds for {key}"
                ));
            }
        }
    }
}

fn same_json<T: Serialize>(left: &T, right: &T) -> bool {
    serde_json::to_value(left).ok() == serde_json::to_value(right).ok()
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
        quench_peer: None,
        rounds: MIN_QUALIFYING_ROUNDS,
        fixed_work: false,
        timeout_ms: DEFAULT_TIMEOUT_MS,
        output: None,
        checkpoint: None,
        resume: None,
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
            "--quench-peer" => {
                options.quench_peer = Some(required_path(&mut args, "--quench-peer"))
            }
            "--fixed-work" => options.fixed_work = true,
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
            "--checkpoint" => options.checkpoint = Some(required_path(&mut args, "--checkpoint")),
            "--resume" => options.resume = Some(required_path(&mut args, "--resume")),
            "--help" | "-h" => usage(""),
            _ => usage(&format!("unknown argument: {arg}")),
        }
    }
    if options.preflight_only && options.fixture.is_some() {
        usage("--preflight-only does not take a fixture");
    }
    if options.checkpoint.is_some() && options.resume.is_some() {
        usage("--checkpoint and --resume are mutually exclusive");
    }
    if (options.checkpoint.is_some() || options.resume.is_some()) && !options.all {
        usage("--checkpoint and --resume require --all");
    }
    if options.preflight_only && (options.checkpoint.is_some() || options.resume.is_some()) {
        usage("--preflight-only cannot use checkpoints");
    }
    if options.quench_peer.is_some() && !options.fixed_work {
        usage("--quench-peer requires --fixed-work");
    }
    if options.fixed_work
        && (options.preflight_only || options.checkpoint.is_some() || options.resume.is_some())
    {
        usage("--fixed-work cannot use preflight or checkpoints");
    }
    if let (Some(checkpoint), Some(output)) = (
        options.checkpoint.as_ref().or(options.resume.as_ref()),
        &options.output,
    ) {
        if checkpoint == output {
            usage("checkpoint and final --out must use different paths");
        }
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

fn prepare_quench_pair(options: &Options) -> Vec<EngineSpec> {
    let mut baseline = spec("quench_baseline", &options.quench, vec![], BTreeMap::new());
    let mut candidate = spec(
        "quench_candidate",
        options.quench_peer.as_ref().expect("pair mode has a peer"),
        vec![],
        BTreeMap::new(),
    );
    let version = format!(
        "Quench shared-VM interpreter build; package {}",
        env!("CARGO_PKG_VERSION")
    );
    for engine in [&mut baseline, &mut candidate] {
        engine.version = version.clone();
        engine.jit_proof_command = vec![engine.executable.display().to_string()];
        engine.jit_proof =
            "the measured shared VM executes interpreter bytecode; no guest JIT path is enabled"
                .into();
    }
    vec![baseline, candidate]
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

fn fixed_work_plan(file: &Path) -> FixedWorkPlan {
    let Some(plan) = file
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| FIXED_WORK_PLANS.iter().find(|plan| plan.fixture == name))
        .copied()
    else {
        fail(&format!("no fixed-work plan for {}", file.display()));
    };
    let expected = Path::new(SUITE_DIR).join(plan.fixture);
    let selected_path = file.canonicalize().unwrap_or_else(|error| {
        fail(&format!(
            "cannot resolve fixed-work fixture {}: {error}",
            file.display()
        ))
    });
    let expected_path = expected.canonicalize().unwrap_or_else(|error| {
        fail(&format!(
            "cannot resolve fixed-work corpus input {}: {error}",
            expected.display()
        ))
    });
    if selected_path != expected_path {
        fail("fixed-work plans are restricted to the pinned V8-v7 fixtures");
    }
    plan
}

fn fixed_work_runner(plan: FixedWorkPlan, iterations: usize) -> String {
    format!(
        r#"
var __quenchFixedSuites = BenchmarkSuite.suites;
var __quenchFixedPrint = typeof console !== "undefined" && typeof console.log === "function"
  ? console.log.bind(console)
  : print;
if (__quenchFixedSuites.length !== {suite_count}) throw new Error("fixed-work suite count mismatch");
var __quenchFixedSuite = __quenchFixedSuites[0];
if (__quenchFixedSuite.name !== "{suite}") throw new Error("fixed-work suite name mismatch");
if (__quenchFixedSuite.benchmarks.length !== {benchmark_count}) throw new Error("fixed-work benchmark count mismatch");
for (var __quenchFixedIndex = 0; __quenchFixedIndex < __quenchFixedSuite.benchmarks.length; __quenchFixedIndex++) {{
  var __quenchFixedBenchmark = __quenchFixedSuite.benchmarks[__quenchFixedIndex];
  __quenchFixedBenchmark.Setup();
  try {{
    for (var __quenchFixedRun = 0; __quenchFixedRun < {iterations}; __quenchFixedRun++) {{
      __quenchFixedBenchmark.run();
    }}
  }} finally {{
    __quenchFixedBenchmark.TearDown();
  }}
}}
__quenchFixedPrint("{marker_prefix}{suite}:{benchmark_count}:{iterations}");
"#,
        suite = plan.suite,
        suite_count = FIXED_WORK_SUITE_COUNT,
        marker_prefix = FIXED_WORK_MARKER_PREFIX,
        benchmark_count = plan.benchmark_count,
        iterations = iterations,
    )
}

fn fixed_work_marker(plan: FixedWorkPlan, iterations: usize) -> String {
    format!(
        "{}{}:{}:{}",
        FIXED_WORK_MARKER_PREFIX, plan.suite, plan.benchmark_count, iterations
    )
}

fn run_fixed_work_report(
    options: &Options,
    files: &[PathBuf],
    engines: &[EngineSpec],
    engine_records: Vec<EngineRecord>,
    source_revision: String,
    source_dirty: bool,
) -> FixedWorkReport {
    let host = host_identity();
    let mut fixtures = BTreeMap::new();
    for file in files {
        let key = file.display().to_string();
        let plan = fixed_work_plan(file);
        let record = run_fixed_work_fixture(
            file,
            plan,
            engines,
            options.rounds,
            options.timeout_ms,
            &host,
        );
        eprintln!(
            "{}: fixed work {} ({}/{} valid rounds, {}/{} clean rounds, {} iterations per benchmark)",
            file.display(),
            if record.valid && record.output_equal {
                "valid"
            } else {
                "invalid"
            },
            record
                .rounds
                .iter()
                .filter(|round| { fixed_work_round_is_valid(round, plan) })
                .count(),
            options.rounds,
            record.clean_rounds,
            options.rounds,
            plan.iterations_per_benchmark,
        );
        fixtures.insert(key, record);
    }
    let complete = !fixtures.is_empty()
        && fixtures
            .values()
            .all(|fixture| fixture.valid && fixture.output_equal);
    FixedWorkReport {
        schema: FIXED_WORK_REPORT_SCHEMA,
        measurement_mode: FIXED_WORK_MEASUREMENT_MODE.into(),
        created_unix_ns: now_ns(),
        rounds_requested: options.rounds,
        timeout_ms: options.timeout_ms,
        source_revision,
        source_dirty,
        corpus: corpus_record(files),
        host,
        contention_policy: contention::POLICY,
        measurement_runner: artifact(&env::current_exe().expect("current benchmark executable")),
        engines: engine_records,
        suite_inputs: corpus_inputs(),
        fixtures,
        complete,
        qualification_ready: false,
    }
}

fn run_fixed_work_fixture(
    file: &Path,
    plan: FixedWorkPlan,
    engines: &[EngineSpec],
    rounds: usize,
    timeout_ms: u64,
    host: &HostRecord,
) -> FixedWorkFixtureRecord {
    let runner = fixed_work_runner(plan, plan.iterations_per_benchmark);
    let setup_only_runner = fixed_work_runner(plan, 0);
    let temporary = materialize_with_runner(file, runner.as_bytes());
    let setup_only_temporary = materialize_with_runner(file, setup_only_runner.as_bytes());
    let materialized_source = artifact(&temporary);
    let setup_only_source = artifact(&setup_only_temporary);
    let mut results = Vec::with_capacity(rounds);
    for round in 0..rounds {
        let attempt = run_fixed_work_attempt(
            &temporary,
            &setup_only_temporary,
            engines,
            round,
            0,
            timeout_ms,
        );
        results.push(FixedWorkRound {
            round,
            execution_order: attempt.execution_order,
            rejected_attempts: Vec::new(),
            selected_attempt: 0,
            samples: attempt.samples,
            setup_only_samples: attempt.setup_only_samples,
        });
        if !fixed_work_round_is_valid(results.last().unwrap(), plan) {
            break;
        }
    }
    let references = contention::assess_rounds(&mut results, engines, plan, host);
    retry_contended_rounds(
        &mut results,
        &temporary,
        &setup_only_temporary,
        engines,
        plan,
        timeout_ms,
        host,
        &references,
    );
    let _ = fs::remove_file(temporary);
    let _ = fs::remove_file(setup_only_temporary);
    let valid = results.len() == rounds
        && results
            .iter()
            .all(|round| fixed_work_round_is_valid(round, plan));
    let output_equal = fixed_work_outputs_equal(&results, engines);
    let clean_rounds = results
        .iter()
        .filter(|round| contention::round_is_clean(round))
        .count();
    let summaries = engines
        .iter()
        .map(|engine| {
            (
                engine.name.to_string(),
                fixed_work_summary(&results, engine.name, plan),
            )
        })
        .collect();
    FixedWorkFixtureRecord {
        source: artifact(file),
        materialized_source,
        setup_only_source,
        plan,
        valid,
        output_equal,
        clean_rounds,
        summaries,
        rounds: results,
    }
}

fn fixed_work_outputs_equal(rounds: &[FixedWorkRound], engines: &[EngineSpec]) -> bool {
    rounds.iter().all(|round| {
        fixed_work_sample_outputs_equal(&round.samples, engines)
            && fixed_work_sample_outputs_equal(&round.setup_only_samples, engines)
    })
}

fn fixed_work_round_is_valid(round: &FixedWorkRound, plan: FixedWorkPlan) -> bool {
    !round.samples.is_empty()
        && round.samples.len() == round.setup_only_samples.len()
        && round
            .samples
            .values()
            .all(|sample| sample.valid_fixed_work(plan, plan.iterations_per_benchmark))
        && round
            .setup_only_samples
            .values()
            .all(|sample| sample.valid_fixed_work(plan, 0))
}

fn run_fixed_work_attempt(
    work_source: &Path,
    setup_only_source: &Path,
    engines: &[EngineSpec],
    round: usize,
    attempt: usize,
    timeout_ms: u64,
) -> FixedWorkRoundAttempt {
    let order = rotated_order(engines, round + attempt);
    let mut samples = BTreeMap::new();
    let mut setup_only_samples = BTreeMap::new();
    let mut execution_order = Vec::with_capacity(engines.len() * 2);
    for engine in &order {
        let work_first = (round + attempt) % FIXED_WORK_SAMPLE_ORDER_PERIOD == 0;
        let sample_order = if work_first {
            [FixedWorkSampleKind::Work, FixedWorkSampleKind::SetupOnly]
        } else {
            [FixedWorkSampleKind::SetupOnly, FixedWorkSampleKind::Work]
        };
        for kind in sample_order {
            let source = match kind {
                FixedWorkSampleKind::Work => work_source,
                FixedWorkSampleKind::SetupOnly => setup_only_source,
            };
            let sample = run(engine, source, timeout_ms);
            execution_order.push(FixedWorkExecution {
                engine: engine.name.to_string(),
                sample: kind,
            });
            match kind {
                FixedWorkSampleKind::Work => {
                    samples.insert(engine.name.to_string(), sample);
                }
                FixedWorkSampleKind::SetupOnly => {
                    setup_only_samples.insert(engine.name.to_string(), sample);
                }
            }
        }
    }
    FixedWorkRoundAttempt {
        attempt,
        execution_order,
        samples,
        setup_only_samples,
    }
}

fn retry_contended_rounds(
    rounds: &mut [FixedWorkRound],
    work_source: &Path,
    setup_only_source: &Path,
    engines: &[EngineSpec],
    plan: FixedWorkPlan,
    timeout_ms: u64,
    host: &HostRecord,
    references: &contention::References,
) {
    for round in rounds {
        for attempt_index in 1..=contention::MAX_RETRIES {
            if !contention::round_needs_retry(round) {
                break;
            }
            let mut attempt = run_fixed_work_attempt(
                work_source,
                setup_only_source,
                engines,
                round.round,
                attempt_index,
                timeout_ms,
            );
            contention::assess_attempt(&mut attempt, engines, references, host);
            if !fixed_work_attempt_is_valid(&attempt, engines, plan) {
                round.rejected_attempts.push(attempt);
                continue;
            }
            if contention::attempt_is_clean(&attempt) {
                if round.selected_attempt == 0 {
                    round.rejected_attempts.push(FixedWorkRoundAttempt {
                        attempt: 0,
                        execution_order: round.execution_order.clone(),
                        samples: round.samples.clone(),
                        setup_only_samples: round.setup_only_samples.clone(),
                    });
                }
                round.selected_attempt = attempt_index;
                round.execution_order = attempt.execution_order.clone();
                round.samples = attempt.samples;
                round.setup_only_samples = attempt.setup_only_samples;
                break;
            }
            round.rejected_attempts.push(attempt);
        }
        round
            .rejected_attempts
            .sort_by_key(|attempt| attempt.attempt);
    }
}

fn fixed_work_attempt_is_valid(
    attempt: &FixedWorkRoundAttempt,
    engines: &[EngineSpec],
    plan: FixedWorkPlan,
) -> bool {
    !attempt.samples.is_empty()
        && attempt.samples.len() == attempt.setup_only_samples.len()
        && attempt
            .samples
            .values()
            .all(|sample| sample.valid_fixed_work(plan, plan.iterations_per_benchmark))
        && attempt
            .setup_only_samples
            .values()
            .all(|sample| sample.valid_fixed_work(plan, 0))
        && fixed_work_sample_outputs_equal(&attempt.samples, engines)
        && fixed_work_sample_outputs_equal(&attempt.setup_only_samples, engines)
}

fn fixed_work_sample_outputs_equal(
    samples: &BTreeMap<String, Sample>,
    engines: &[EngineSpec],
) -> bool {
    let Some(first) = engines.first().and_then(|engine| samples.get(engine.name)) else {
        return false;
    };
    engines.iter().all(|engine| {
        samples.get(engine.name).is_some_and(|sample| {
            sample.status == first.status
                && semantic_output(&sample.stdout) == semantic_output(&first.stdout)
        })
    })
}

fn fixed_work_summary(
    rounds: &[FixedWorkRound],
    engine: &str,
    plan: FixedWorkPlan,
) -> FixedWorkSummary {
    let work_samples = rounds
        .iter()
        .filter_map(|round| round.samples.get(engine))
        .filter(|sample| sample.valid_fixed_work(plan, plan.iterations_per_benchmark))
        .collect::<Vec<_>>();
    let setup_only_samples = rounds
        .iter()
        .filter_map(|round| round.setup_only_samples.get(engine))
        .filter(|sample| sample.valid_fixed_work(plan, 0))
        .collect::<Vec<_>>();
    let marginal_per_run = fixed_work_marginal_summary(rounds, engine, plan);
    FixedWorkSummary {
        work: fixed_work_process_summary(&work_samples),
        setup_only: fixed_work_process_summary(&setup_only_samples),
        marginal_per_run,
    }
}

fn fixed_work_process_summary(samples: &[&Sample]) -> FixedWorkProcessSummary {
    let clean_samples = samples
        .iter()
        .copied()
        .filter(|sample| contention::sample_is_clean(sample))
        .collect::<Vec<_>>();
    FixedWorkProcessSummary {
        median_wall_ns: median_u128(clean_samples.iter().map(|sample| sample.wall_ns).collect()),
        median_cycles: median_u64(
            clean_samples
                .iter()
                .filter_map(|sample| sample.cycles)
                .collect(),
        ),
        median_instructions: median_u64(
            clean_samples
                .iter()
                .filter_map(|sample| sample.instructions)
                .collect(),
        ),
        median_max_rss_bytes: median_u64(
            samples
                .iter()
                .filter_map(|sample| sample.peak_rss_bytes)
                .collect(),
        ),
        valid_samples: samples.len(),
        clean_samples: clean_samples.len(),
    }
}

fn fixed_work_marginal_summary(
    rounds: &[FixedWorkRound],
    engine: &str,
    plan: FixedWorkPlan,
) -> FixedWorkPerRunSummary {
    let work_calls = plan.total_run_calls() as f64;
    let mut wall_deltas = Vec::new();
    let mut cycle_deltas = Vec::new();
    let mut instruction_deltas = Vec::new();
    let mut paired_samples = 0;
    for round in rounds {
        let (Some(work), Some(setup_only)) = (
            round.samples.get(engine),
            round.setup_only_samples.get(engine),
        ) else {
            continue;
        };
        if !work.valid_fixed_work(plan, plan.iterations_per_benchmark)
            || !setup_only.valid_fixed_work(plan, 0)
        {
            continue;
        }
        paired_samples += 1;
        if !contention::sample_is_clean(work) || !contention::sample_is_clean(setup_only) {
            continue;
        }
        wall_deltas.push((work.wall_ns as f64 - setup_only.wall_ns as f64) / work_calls);
        if let (Some(work), Some(setup)) = (work.cycles, setup_only.cycles) {
            cycle_deltas.push((work as f64 - setup as f64) / work_calls);
        }
        if let (Some(work), Some(setup)) = (work.instructions, setup_only.instructions) {
            instruction_deltas.push((work as f64 - setup as f64) / work_calls);
        }
    }
    let clean_paired_samples = wall_deltas.len();
    FixedWorkPerRunSummary {
        median_wall_ns: median_f64(wall_deltas),
        median_cycles: median_f64(cycle_deltas),
        median_instructions: median_f64(instruction_deltas),
        paired_samples,
        clean_paired_samples,
    }
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
    let host_before = host_snapshot();
    let mut sample = run_measured(engine, source, timeout_ms);
    sample.host_before = Some(host_before);
    sample.host_after = Some(host_snapshot());
    sample
}

#[cfg(target_os = "macos")]
fn run_measured(engine: &EngineSpec, source: &Path, timeout_ms: u64) -> Sample {
    let started = Instant::now();
    let timeout_seconds = format!("{:.3}", timeout_ms as f64 / 1000.0);
    let output = Command::new("timeout")
        .args([
            "--signal=TERM",
            "--kill-after=1",
            &timeout_seconds,
            "/usr/bin/time",
            "-l",
        ])
        .arg(&engine.executable)
        .args(&engine.argv)
        .arg(source)
        .env_clear()
        .envs(&engine.env)
        .output();
    let (status, stdout, stderr) = match output {
        Ok(output) => (
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ),
        Err(error) => (-1, String::new(), error.to_string()),
    };
    let timed_out = matches!(status, 124 | 137);
    let score = stdout
        .lines()
        .find_map(|line| line.strip_prefix("Score: "))
        .and_then(|value| value.parse().ok());
    Sample {
        status,
        timed_out,
        wall_ns: started.elapsed().as_nanos(),
        peak_rss_bytes: time_metric(&stderr, "maximum resident set size"),
        score,
        instructions: time_metric(&stderr, "instructions retired"),
        cycles: time_metric(&stderr, "cycles elapsed"),
        page_faults: time_metric(&stderr, "page faults"),
        page_reclaims: time_metric(&stderr, "page reclaims"),
        involuntary_context_switches: time_metric(&stderr, "involuntary context switches"),
        host_before: None,
        host_after: None,
        contention: None,
        stdout,
        stderr,
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn run_measured(engine: &EngineSpec, source: &Path, timeout_ms: u64) -> Sample {
    run_wait4(engine, source, timeout_ms)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn run_wait4(engine: &EngineSpec, source: &Path, timeout_ms: u64) -> Sample {
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
        host_before: None,
        host_after: None,
        contention: None,
        stdout,
        stderr,
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn read_pipe(mut reader: impl Read) -> (Vec<u8>, Option<String>) {
    let mut output = Vec::new();
    let error = reader
        .read_to_end(&mut output)
        .err()
        .map(|error| error.to_string());
    (output, error)
}

#[cfg(all(unix, not(target_os = "macos")))]
struct ProcessUsage {
    peak_rss_bytes: Option<u64>,
    page_faults: u64,
    page_reclaims: u64,
    involuntary_context_switches: u64,
}

#[cfg(all(unix, not(target_os = "macos")))]
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
            if timed_out {
                // The leader may exit on SIGTERM while descendants still hold the
                // captured pipes. Finish the group timeout before the caller joins.
                // SAFETY: `pid` is the process-group ID established before spawn.
                unsafe { libc::kill(-pid, libc::SIGKILL) };
            }
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

#[cfg(all(unix, not(target_os = "macos")))]
fn nonnegative(value: libc::c_long) -> u64 {
    u64::try_from(value).unwrap_or(0)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn peak_rss_bytes(raw_rss: libc::c_long) -> Option<u64> {
    let rss = u64::try_from(raw_rss).ok()?;
    if cfg!(target_os = "linux") {
        rss.checked_mul(1024)
    } else {
        Some(rss)
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
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
        host_before: None,
        host_after: None,
        contention: None,
        stdout: String::new(),
        stderr: error,
    }
}

#[cfg(target_os = "macos")]
fn time_metric(stderr: &str, suffix: &str) -> Option<u64> {
    stderr.lines().find_map(|line| {
        line.trim()
            .strip_suffix(suffix)
            .and_then(|value| value.trim().parse().ok())
    })
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

fn median_u128(mut values: Vec<u128>) -> Option<u128> {
    values.sort_unstable();
    values.get(values.len() / 2).copied()
}

fn materialize(file: &Path) -> PathBuf {
    materialize_with_runner(file, RUNNER.as_bytes())
}

fn materialize_with_runner(file: &Path, runner: &[u8]) -> PathBuf {
    let nonce = now_ns();
    let name = file.file_name().unwrap_or_default().to_string_lossy();
    let path = Path::new("/tmp").join(format!(
        "quench-v8-v7-{}-{nonce}-{name}",
        std::process::id()
    ));
    let base = fs::read(Path::new(SUITE_DIR).join("base.js"))
        .unwrap_or_else(|error| fail(&error.to_string()));
    let fixture = fs::read(file).unwrap_or_else(|error| fail(&error.to_string()));
    let mut source = Vec::with_capacity(base.len() + fixture.len() + runner.len() + 2);
    source.extend_from_slice(&base);
    source.push(b'\n');
    source.extend_from_slice(&fixture);
    source.push(b'\n');
    source.extend_from_slice(runner);
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
                && !line.starts_with("__quenchFixedWork:")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn host_snapshot() -> HostSnapshot {
    let (load_average, load_average_error) = capture_load_average();
    let (top_cpu_consumers, process_list_error) = capture_cpu_consumers();
    HostSnapshot {
        captured_unix_ns: now_ns(),
        load_average,
        load_average_error,
        top_cpu_consumers,
        process_list_error,
    }
}

#[cfg(target_os = "macos")]
fn capture_load_average() -> (Option<LoadAverage>, Option<String>) {
    let output = Command::new("sysctl").args(["-n", "vm.loadavg"]).output();
    match output {
        Ok(output) if output.status.success() => {
            let text = String::from_utf8_lossy(&output.stdout);
            match parse_load_average(&text) {
                Some(load) => (Some(load), None),
                None => (None, Some(format!("cannot parse sysctl output: {text}"))),
            }
        }
        Ok(output) => (None, Some(command_failure("sysctl", &output))),
        Err(error) => (None, Some(error.to_string())),
    }
}

#[cfg(target_os = "linux")]
fn capture_load_average() -> (Option<LoadAverage>, Option<String>) {
    match fs::read_to_string("/proc/loadavg") {
        Ok(text) => match parse_load_average(&text) {
            Some(load) => (Some(load), None),
            None => (None, Some("cannot parse /proc/loadavg".to_string())),
        },
        Err(error) => (None, Some(error.to_string())),
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn capture_load_average() -> (Option<LoadAverage>, Option<String>) {
    (
        None,
        Some("load average capture is unsupported on this platform".into()),
    )
}

fn parse_load_average(text: &str) -> Option<LoadAverage> {
    let values = text
        .split(|character: char| {
            !character.is_ascii_digit() && character != '.' && character != '-'
        })
        .filter(|value| !value.is_empty())
        .take(3)
        .map(str::parse::<f64>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    let [one_minute, five_minutes, fifteen_minutes]: [f64; 3] = values.try_into().ok()?;
    [one_minute, five_minutes, fifteen_minutes]
        .iter()
        .all(|value| value.is_finite() && *value >= 0.0)
        .then_some(LoadAverage {
            one_minute,
            five_minutes,
            fifteen_minutes,
        })
}

fn capture_cpu_consumers() -> (Vec<CpuConsumer>, Option<String>) {
    let output = Command::new("ps")
        .args(["-A", "-o", "pid=,pcpu=,comm="])
        .output();
    match output {
        Ok(output) if output.status.success() => {
            let text = String::from_utf8_lossy(&output.stdout);
            (parse_cpu_consumers(&text), None)
        }
        Ok(output) => (Vec::new(), Some(command_failure("ps", &output))),
        Err(error) => (Vec::new(), Some(error.to_string())),
    }
}

fn command_failure(command: &str, output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.is_empty() {
        format!("{command} exited with {}", output.status)
    } else {
        stderr
    }
}

fn parse_cpu_consumers(text: &str) -> Vec<CpuConsumer> {
    let mut consumers = text
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let pid = fields.next()?.parse().ok()?;
            let cpu_percent = fields.next()?.parse::<f64>().ok()?;
            let command = fields.collect::<Vec<_>>().join(" ");
            (!command.is_empty() && cpu_percent.is_finite() && cpu_percent >= 0.0).then_some(
                CpuConsumer {
                    pid,
                    cpu_percent,
                    command,
                },
            )
        })
        .collect::<Vec<_>>();
    consumers.sort_unstable_by(|left, right| {
        right
            .cpu_percent
            .total_cmp(&left.cpu_percent)
            .then_with(|| left.pid.cmp(&right.pid))
    });
    consumers.truncate(HOST_CPU_CONSUMER_LIMIT);
    consumers
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
        logical_cpus: std::thread::available_parallelism().ok().map(usize::from),
        process_metrics_backend: if cfg!(target_os = "macos") {
            "macOS /usr/bin/time -l".into()
        } else if cfg!(target_os = "linux") {
            "Linux wait4 rusage".into()
        } else {
            "Unix wait4 rusage".into()
        },
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
        .ok()
        .filter(|output| output.status.success())
        .or_else(|| {
            Command::new("sha256sum")
                .arg(path)
                .output()
                .ok()
                .filter(|output| output.status.success())
        })?;
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
        "{message}\nusage: quench-bench <fixture.js>|--all [--fixed-work] [--quench PATH] [--quench-peer PATH] [--qjs PATH] [--bun PATH] [--node PATH] [--runs N] [--timeout-ms N] [--checkpoint PATH|--resume PATH] [--out PATH]\n       quench-bench --preflight-only [engine options]\n       quench-bench --analyze REPORT [--out JSON]"
    );
    std::process::exit(2)
}

fn fail(message: &str) -> ! {
    eprintln!("{message}");
    std::process::exit(2)
}

#[cfg(test)]
mod fixed_work_tests {
    use super::{
        contention, fixed_work_marker, fixed_work_runner, parse_cpu_consumers, parse_load_average,
        FixedWorkPlan, HostSnapshot, LoadAverage, Sample, FIXED_WORK_PLANS, FIXTURES,
    };
    use std::collections::BTreeMap;

    #[test]
    fn fixed_work_plan_covers_each_fixture_once_with_positive_work() {
        let planned = FIXED_WORK_PLANS
            .iter()
            .map(|plan| plan.fixture)
            .collect::<Vec<_>>();
        assert_eq!(planned, FIXTURES);
        assert!(FIXED_WORK_PLANS.iter().all(|plan| {
            !plan.suite.is_empty()
                && plan.benchmark_count > 0
                && plan.iterations_per_benchmark > 0
                && plan.total_run_calls() > 0
        }));
    }

    #[test]
    fn fixed_work_runner_keeps_setup_run_and_teardown_order_explicit() {
        let plan = *FIXED_WORK_PLANS
            .iter()
            .find(|plan| plan.fixture == "splay.js")
            .unwrap();
        let runner = fixed_work_runner(plan, plan.iterations_per_benchmark);
        let setup = runner.find("__quenchFixedBenchmark.Setup();").unwrap();
        let iterations = format!("__quenchFixedRun < {}", plan.iterations_per_benchmark);
        let run_loop = runner.find(&iterations).unwrap();
        let run_call = runner.find("__quenchFixedBenchmark.run();").unwrap();
        let teardown = runner.find("__quenchFixedBenchmark.TearDown();").unwrap();
        let marker = runner
            .find(&fixed_work_marker(plan, plan.iterations_per_benchmark))
            .unwrap();
        assert!(
            setup < run_loop && run_loop < run_call && run_call < teardown && teardown < marker
        );
        assert!(runner.contains("finally"));
        assert!(!runner.contains("RunSuites"));
        assert!(!runner.contains("Date"));

        let setup_only = fixed_work_runner(plan, 0);
        assert!(setup_only.contains("__quenchFixedRun < 0"));
        assert!(setup_only.contains(&fixed_work_marker(plan, 0)));
    }

    #[test]
    fn fixed_work_summary_uses_paired_setup_only_differences() {
        let plan = FixedWorkPlan {
            fixture: "crypto.js",
            suite: "Crypto",
            benchmark_count: 2,
            iterations_per_benchmark: 5,
        };
        let rounds = [(100, 99), (220, 100), (160, 159)]
            .into_iter()
            .enumerate()
            .map(
                |(round, (work_cycles, setup_cycles))| super::FixedWorkRound {
                    round,
                    execution_order: Vec::new(),
                    rejected_attempts: Vec::new(),
                    selected_attempt: 0,
                    samples: BTreeMap::from([(
                        "quench".into(),
                        fixed_work_sample(plan, 5, work_cycles),
                    )]),
                    setup_only_samples: BTreeMap::from([(
                        "quench".into(),
                        fixed_work_sample(plan, 0, setup_cycles),
                    )]),
                },
            )
            .collect::<Vec<_>>();

        let summary = super::fixed_work_summary(&rounds, "quench", plan);

        assert_eq!(summary.marginal_per_run.median_cycles, Some(0.1));
        assert_eq!(summary.marginal_per_run.median_instructions, Some(0.1));
        assert_eq!(summary.marginal_per_run.paired_samples, 3);
        assert_eq!(summary.work.median_cycles, Some(160));
        assert_eq!(summary.setup_only.median_cycles, Some(100));
    }

    #[test]
    fn fixed_work_sample_requires_completion_marker_without_a_score() {
        let plan = FixedWorkPlan {
            fixture: "crypto.js",
            suite: "Crypto",
            benchmark_count: 2,
            iterations_per_benchmark: 10,
        };
        let sample = Sample {
            status: 0,
            timed_out: false,
            wall_ns: 1,
            peak_rss_bytes: Some(1),
            score: None,
            instructions: Some(2),
            cycles: Some(3),
            page_faults: None,
            page_reclaims: None,
            involuntary_context_switches: None,
            host_before: None,
            host_after: None,
            contention: None,
            stdout: format!(
                "{}\n",
                fixed_work_marker(plan, plan.iterations_per_benchmark)
            ),
            stderr: String::new(),
        };
        assert!(sample.valid_fixed_work(plan, plan.iterations_per_benchmark));

        let scored_sample = Sample {
            score: Some(1.0),
            ..sample.clone()
        };
        assert!(!scored_sample.valid_fixed_work(plan, plan.iterations_per_benchmark));
        let mut wrong_plan = plan;
        wrong_plan.iterations_per_benchmark += 1;
        let wrong_marker = Sample {
            stdout: format!(
                "{}\n",
                fixed_work_marker(wrong_plan, wrong_plan.iterations_per_benchmark)
            ),
            ..sample
        };
        assert!(!wrong_marker.valid_fixed_work(plan, plan.iterations_per_benchmark));
    }

    fn fixed_work_sample(plan: FixedWorkPlan, iterations: usize, cycles: u64) -> Sample {
        Sample {
            status: 0,
            timed_out: false,
            wall_ns: cycles as u128,
            peak_rss_bytes: Some(1),
            score: None,
            instructions: Some(cycles),
            cycles: Some(cycles),
            page_faults: None,
            page_reclaims: None,
            involuntary_context_switches: Some(1),
            host_before: Some(HostSnapshot {
                captured_unix_ns: 0,
                load_average: Some(LoadAverage {
                    one_minute: 0.0,
                    five_minutes: 0.0,
                    fifteen_minutes: 0.0,
                }),
                load_average_error: None,
                top_cpu_consumers: Vec::new(),
                process_list_error: None,
            }),
            host_after: Some(HostSnapshot {
                captured_unix_ns: 1,
                load_average: Some(LoadAverage {
                    one_minute: 0.0,
                    five_minutes: 0.0,
                    fifteen_minutes: 0.0,
                }),
                load_average_error: None,
                top_cpu_consumers: Vec::new(),
                process_list_error: None,
            }),
            contention: Some(contention::verified_clean_assessment()),
            stdout: format!("{}\n", fixed_work_marker(plan, iterations)),
            stderr: String::new(),
        }
    }

    #[test]
    fn load_average_parser_accepts_sysctl_and_proc_formats() {
        assert_eq!(
            parse_load_average("{ 1.25 2.50 3.75 }\n"),
            Some(LoadAverage {
                one_minute: 1.25,
                five_minutes: 2.50,
                fifteen_minutes: 3.75,
            })
        );
        assert_eq!(
            parse_load_average("1.25 2.50 3.75 2/100 1234\n"),
            Some(LoadAverage {
                one_minute: 1.25,
                five_minutes: 2.50,
                fifteen_minutes: 3.75,
            })
        );
        assert_eq!(parse_load_average("1.25 2.50"), None);
        assert_eq!(parse_load_average("NaN 2 3"), None);
    }

    #[test]
    fn cpu_process_parser_keeps_the_sorted_top_five() {
        let input = (1..=7)
            .rev()
            .map(|pid| format!("{pid} {pid}.0 process-{pid}\n"))
            .collect::<String>();
        let consumers = parse_cpu_consumers(&input);
        assert_eq!(consumers.len(), 5);
        assert_eq!(consumers[0].pid, 7);
        assert_eq!(consumers[0].cpu_percent, 7.0);
        assert_eq!(consumers[0].command, "process-7");
    }
}

#[cfg(all(test, unix, not(target_os = "macos")))]
mod tests {
    use super::{
        artifact, now_ns, peak_rss_bytes, read_checkpoint, validate_resume, wait_child,
        write_checkpoint, CorpusRecord, FixtureRecord, HostRecord, SuiteRecord,
    };
    use std::process::Command;
    use std::{collections::BTreeMap, fs, path::PathBuf};

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

    #[test]
    fn checkpoint_round_trips_and_validates_fixture_inputs() {
        let directory = std::env::temp_dir().join(format!(
            "quench-bench-checkpoint-{}-{}",
            std::process::id(),
            now_ns()
        ));
        fs::create_dir(&directory).expect("create checkpoint test directory");
        let fixture_path = directory.join("fixture.js");
        fs::write(&fixture_path, "// pinned fixture\n").expect("write fixture");
        let report_path = directory.join("report.json");
        let expected = SuiteRecord {
            schema: 5,
            created_unix_ns: now_ns(),
            rounds_requested: 11,
            timeout_ms: 300_000,
            source_revision: "a".repeat(40),
            source_dirty: false,
            corpus: CorpusRecord {
                pinned_revision: Some("b".repeat(40)),
                checkout_revision: Some("b".repeat(40)),
                checkout_clean: true,
                fixture_set_matches: true,
            },
            host: HostRecord {
                uname: "test host".into(),
                rustc: "rustc test".into(),
                model: Some("test model".into()),
                memory_bytes: Some(1),
                memory_limit_bytes: Some(1),
                cpu_quota: Some("400000 100000".into()),
                logical_cpus: Some(4),
                process_metrics_backend: "wait4 test".into(),
            },
            engines: Vec::new(),
            suite_inputs: Vec::new(),
            fixtures: BTreeMap::new(),
            complete: false,
            qualification_ready: false,
        };
        let mut saved = expected.clone();
        saved.fixtures.insert(
            fixture_path.display().to_string(),
            FixtureRecord {
                source: artifact(&fixture_path),
                valid: false,
                output_equal: false,
                summaries: BTreeMap::new(),
                rounds: Vec::new(),
            },
        );

        write_checkpoint(&report_path, &saved);
        let loaded = read_checkpoint(&report_path);
        validate_resume(&loaded, &expected, &[PathBuf::from(&fixture_path)]);
        assert_eq!(loaded.fixtures.len(), 1);
        fs::remove_dir_all(directory).expect("remove checkpoint test directory");
    }
}
