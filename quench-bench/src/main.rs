mod analysis;
mod contention;
mod jetstream;

use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    ffi::OsString,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::OnceLock,
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
const MIN_DIAGNOSTIC_SCREEN_ROUNDS: usize = 3;
const DEFAULT_TIMEOUT_MS: u64 = 300_000;
const MEASUREMENT_ENV: &[&str] = &["PATH", "HOME", "TMPDIR", "LANG", "LC_ALL", "TZ"];
const QUENCH_WASM_EMSCRIPTEN_MEMCPY_DIRECT: &str = "QUENCH_WASM_EMSCRIPTEN_MEMCPY_DIRECT";
const QUENCH_WASM_EMSCRIPTEN_MEMCPY_RAW_I32: &str = "QUENCH_WASM_EMSCRIPTEN_MEMCPY_RAW_I32";
const QUENCH_WASM_EMSCRIPTEN_MEMCPY_EARLY_IMPORT: &str =
    "QUENCH_WASM_EMSCRIPTEN_MEMCPY_EARLY_IMPORT";
const QUENCH_WASM_JIT_EMSCRIPTEN_MEMCPY_LEAF: &str = "QUENCH_WASM_JIT_EMSCRIPTEN_MEMCPY_LEAF";
const QUENCH_WASM_JIT_DIRECT_COMPARE_BRANCHES: &str = "QUENCH_WASM_JIT_DIRECT_COMPARE_BRANCHES";
const QUENCH_WASM_JIT_AFFINE_PAIR: &str = "QUENCH_WASM_JIT_AFFINE_PAIR";
const QUENCH_WASM_JIT_REFERENCE_BODY_CACHE: &str = "QUENCH_WASM_JIT_REFERENCE_BODY_CACHE";
const QUENCH_WASM_JIT_SELECTIVE: &str = "QUENCH_WASM_JIT_SELECTIVE";
const FIXED_WORK_REPORT_SCHEMA: u32 = 5;
const SUITE_REPORT_SCHEMA: u32 = 16;
const FIXED_WORK_MEASUREMENT_MODE: &str = "fixed_work_diagnostic";
const FIXED_WORK_MARKER_PREFIX: &str = "__quenchFixedWork:";
const FIXED_WORK_SUITE_COUNT: usize = 1;
const FIXED_WORK_MARKER_COUNT: usize = 1;
const FIXED_WORK_SAMPLE_ORDER_PERIOD: usize = 2;
const HOST_CPU_CONSUMER_LIMIT: usize = 5;
const QUENCH_JS_JIT_GENERAL_DISPATCH: &str = "QUENCH_JS_JIT_GENERAL_DISPATCH";
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
const V8_V7_CORPUS: &str = "quench-bench/js-engine-benchmark";

/// A benchmark suite: its corpus checkout, fixtures, engine invocation and
/// Score line. Every suite shares the matched-round runner and its report.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum Suite {
    #[default]
    V8V7,
    JetStream3,
}

/// Whether the benchmark run enables each engine's optimizing compiler.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum JitMode {
    #[default]
    NoJit,
    Jit,
}

impl Suite {
    fn parse(name: &str) -> Self {
        match name {
            "v8-v7" => Self::V8V7,
            "jetstream3" => Self::JetStream3,
            _ => usage(&format!("unknown suite: {name}")),
        }
    }

    /// The submodule pinned by the repository.
    fn corpus(self) -> &'static str {
        match self {
            Self::V8V7 => V8_V7_CORPUS,
            Self::JetStream3 => jetstream::SUITE_DIR,
        }
    }

    fn fixtures(self) -> Vec<PathBuf> {
        match self {
            Self::V8V7 => FIXTURES
                .iter()
                .map(|name| Path::new(SUITE_DIR).join(name))
                .collect(),
            Self::JetStream3 => jetstream::fixtures(),
        }
    }

    /// A fixture named on the command line: a file, or a JetStream test.
    fn fixture(self, name: &str) -> PathBuf {
        match self {
            Self::V8V7 => PathBuf::from(name),
            Self::JetStream3 => jetstream::fixture_path(name),
        }
    }

    fn inputs(self) -> Vec<Artifact> {
        match self {
            Self::V8V7 => std::iter::once(Path::new(SUITE_DIR).join("base.js"))
                .chain(FIXTURES.iter().map(|name| Path::new(SUITE_DIR).join(name)))
                .map(|path| artifact(&path))
                .collect(),
            Self::JetStream3 => [
                "cli.js",
                "JetStreamDriver.js",
                "utils/params.js",
                "utils/shell-config.js",
            ]
            .iter()
            .map(|name| artifact(&Path::new(jetstream::SUITE_DIR).join(name)))
            .collect(),
        }
    }

    /// The measured engines, Quench first. QuickJS has no realm API, which
    /// JetStream needs for every benchmark.
    fn engine_names(self) -> &'static [&'static str] {
        match self {
            Self::V8V7 => &["quench", "quickjs", "bun", "node"],
            Self::JetStream3 => &["quench", "bun", "node"],
        }
    }

    fn fixture_names(self) -> &'static [&'static str] {
        match self {
            Self::V8V7 => FIXTURES,
            Self::JetStream3 => jetstream::FIXTURES,
        }
    }

    /// Writes the runnable source and returns it with the engine arguments.
    fn materialize(self, file: &Path) -> (PathBuf, Vec<OsString>) {
        match self {
            Self::V8V7 => {
                let source = materialize(file);
                let arguments = vec![source.clone().into_os_string()];
                (source, arguments)
            }
            Self::JetStream3 => {
                let prelude = temporary_path("jetstream3-prelude.js");
                fs::write(&prelude, jetstream::prelude())
                    .unwrap_or_else(|error| fail(&error.to_string()));
                let arguments = jetstream::arguments(file, &prelude);
                (prelude, arguments)
            }
        }
    }

    fn score(self, file: &Path, stdout: &str) -> Option<f64> {
        match self {
            Self::V8V7 => v8_v7_score(stdout),
            Self::JetStream3 => jetstream::score(file, stdout),
        }
    }

    fn semantic_output(self, stdout: &str) -> String {
        match self {
            Self::V8V7 => semantic_output(stdout),
            Self::JetStream3 => jetstream::semantic_output(stdout),
        }
    }

    /// Why `engine` cannot run `file`, when it cannot.
    fn unsupported_reason(self, file: &Path, engine: &EngineSpec) -> Option<String> {
        match self {
            Self::V8V7 => None,
            Self::JetStream3 => (jetstream::requires_webassembly(file) && !engine.webassembly)
                .then(|| "the test is tagged Wasm and the engine has no WebAssembly".to_string()),
        }
    }
}

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
    /// Whether the engine exposes `WebAssembly`, probed with its measurement arguments.
    webassembly: bool,
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
    #[serde(default = "webassembly_assumed")]
    webassembly: bool,
}

/// Reports written before the probe existed measured only engines that ran every fixture.
fn webassembly_assumed() -> bool {
    true
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
    /// Engines that cannot run the fixture, with the reason; they are not measured on it.
    #[serde(default)]
    unsupported_engines: BTreeMap<String, String>,
    rounds: Vec<RoundRecord>,
}

#[derive(Clone, Deserialize, Serialize)]
struct SuiteRecord {
    schema: u32,
    #[serde(default)]
    suite: Suite,
    #[serde(default)]
    jit_mode: JitMode,
    /// Runs with JIT counters/reporting enabled are diagnostic, not score
    /// qualification measurements.
    #[serde(default)]
    jit_diagnostics: bool,
    /// Uses the experimental JS GeneralDispatch tier; not qualification mode.
    #[serde(default)]
    jit_js_general_dispatch: bool,
    /// Experimental bounded root compilation; never qualification mode.
    #[serde(default)]
    jit_wasm_root: Option<u32>,
    /// Experimental interpreter-to-native single-root mode; never qualification mode.
    #[serde(default)]
    jit_wasm_root_only: bool,
    /// Starts from one selected Wasm root and then promotes hot fallback callees.
    #[serde(default)]
    jit_wasm_root_first: bool,
    /// Starts Wasm functions in the interpreter and promotes hot roots only.
    #[serde(default)]
    jit_wasm_selective: bool,
    /// Paired GCC-Wasm diagnostic for the direct Emscripten memcpy path.
    #[serde(default)]
    jit_wasm_memcpy_ab: bool,
    /// Paired GCC-Wasm diagnostic for raw-i32 Emscripten memcpy arguments.
    #[serde(default)]
    jit_wasm_memcpy_raw_ab: bool,
    /// Paired GCC-Wasm diagnostic for attempting raw-i32 memcpy before the generic import boundary.
    #[serde(default)]
    jit_wasm_memcpy_early_ab: bool,
    /// Paired QuickSort-Wasm diagnostic for direct Wasm compare-branch lowering.
    #[serde(default)]
    jit_wasm_compare_branch_ab: bool,
    /// Paired GCC-Wasm diagnostic for affine pair-loop vectorization.
    #[serde(default)]
    jit_wasm_affine_pair_ab: bool,
    /// Paired TSF-Wasm diagnostic for same-artifact indirect-call body pointers.
    #[serde(default)]
    jit_wasm_reference_body_cache_ab: bool,
    /// Paired GCC-Wasm diagnostic for a native Emscripten memcpy leaf call.
    #[serde(default)]
    jit_wasm_memcpy_leaf_ab: bool,
    /// Paired GCC-Wasm diagnostic for selective CallKnown handoff.
    #[serde(default)]
    jit_wasm_selective_ab: bool,
    created_unix_ns: u128,
    rounds_requested: usize,
    timeout_ms: u64,
    source_revision: String,
    source_dirty: bool,
    corpus: CorpusRecord,
    host: HostRecord,
    engines: Vec<EngineRecord>,
    suite_inputs: Vec<Artifact>,
    /// The exact canonical fixture selection, including unfinished checkpoint runs.
    #[serde(default)]
    selected_fixtures: Vec<String>,
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
    suite: Suite,
    jit_mode: JitMode,
    jit_diagnostics: bool,
    jit_js_general_dispatch: bool,
    jit_wasm_root: Option<u32>,
    jit_wasm_root_only: bool,
    jit_wasm_root_first: bool,
    jit_wasm_selective: bool,
    jit_wasm_memcpy_ab: bool,
    jit_wasm_memcpy_raw_ab: bool,
    jit_wasm_memcpy_early_ab: bool,
    jit_wasm_compare_branch_ab: bool,
    jit_wasm_affine_pair_ab: bool,
    jit_wasm_reference_body_cache_ab: bool,
    jit_wasm_memcpy_leaf_ab: bool,
    jit_wasm_selective_ab: bool,
    fixture: Option<String>,
    fixtures: Option<Vec<String>>,
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

    let suite = options.suite;
    let mut report = SuiteRecord {
        schema: SUITE_REPORT_SCHEMA,
        suite,
        jit_mode: options.jit_mode,
        jit_diagnostics: options.jit_diagnostics,
        jit_js_general_dispatch: options.jit_js_general_dispatch,
        jit_wasm_root: options.jit_wasm_root,
        jit_wasm_root_only: options.jit_wasm_root_only,
        jit_wasm_root_first: options.jit_wasm_root_first,
        jit_wasm_selective: options.jit_wasm_selective,
        jit_wasm_memcpy_ab: options.jit_wasm_memcpy_ab,
        jit_wasm_memcpy_raw_ab: options.jit_wasm_memcpy_raw_ab,
        jit_wasm_memcpy_early_ab: options.jit_wasm_memcpy_early_ab,
        jit_wasm_compare_branch_ab: options.jit_wasm_compare_branch_ab,
        jit_wasm_affine_pair_ab: options.jit_wasm_affine_pair_ab,
        jit_wasm_reference_body_cache_ab: options.jit_wasm_reference_body_cache_ab,
        jit_wasm_memcpy_leaf_ab: options.jit_wasm_memcpy_leaf_ab,
        jit_wasm_selective_ab: options.jit_wasm_selective_ab,
        created_unix_ns: now_ns(),
        rounds_requested: options.rounds,
        timeout_ms: options.timeout_ms,
        source_revision,
        source_dirty,
        corpus: corpus_record(suite, &files),
        host: host_identity(),
        engines: engine_records,
        suite_inputs: suite.inputs(),
        selected_fixtures: files
            .iter()
            .map(|file| file.display().to_string())
            .collect(),
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
    for file in &files {
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
        let fixture = run_fixture(suite, file, &engines, options.rounds, options.timeout_ms);
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

    let complete = !report.fixtures.is_empty()
        && report
            .fixtures
            .values()
            .all(|fixture| fixture.valid && fixture.output_equal);
    report.complete = complete;
    report.qualification_ready = complete
        && options.rounds >= MIN_QUALIFYING_ROUNDS
        && !options.jit_diagnostics
        && !options.jit_js_general_dispatch
        && options.jit_wasm_root.is_none()
        && !options.jit_wasm_root_only
        && !options.jit_wasm_root_first
        && !options.jit_wasm_selective
        && !options.jit_wasm_memcpy_ab
        && !options.jit_wasm_memcpy_raw_ab
        && !options.jit_wasm_memcpy_early_ab
        && !options.jit_wasm_compare_branch_ab
        && !options.jit_wasm_affine_pair_ab
        && !options.jit_wasm_reference_body_cache_ab
        && !options.jit_wasm_memcpy_leaf_ab
        && !options.jit_wasm_selective_ab
        && report
            .fixtures
            .values()
            .all(|fixture| fixture.source.sha256.as_deref().is_some_and(valid_sha256))
        && fixture_selection_is_valid(options.suite, &files)
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
            webassembly: self.webassembly,
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
    if !resume_metadata_matches(saved, expected) {
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
        if !same_json(&fixture.source, &fixture_artifact(expected.suite, file)) {
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

fn resume_metadata_matches(saved: &SuiteRecord, expected: &SuiteRecord) -> bool {
    saved.schema == expected.schema
        && saved.suite == expected.suite
        && saved.jit_mode == expected.jit_mode
        && saved.jit_diagnostics == expected.jit_diagnostics
        && saved.jit_js_general_dispatch == expected.jit_js_general_dispatch
        && saved.jit_wasm_root == expected.jit_wasm_root
        && saved.jit_wasm_root_only == expected.jit_wasm_root_only
        && saved.jit_wasm_root_first == expected.jit_wasm_root_first
        && saved.jit_wasm_selective == expected.jit_wasm_selective
        && saved.jit_wasm_memcpy_ab == expected.jit_wasm_memcpy_ab
        && saved.jit_wasm_memcpy_raw_ab == expected.jit_wasm_memcpy_raw_ab
        && saved.jit_wasm_memcpy_early_ab == expected.jit_wasm_memcpy_early_ab
        && saved.jit_wasm_compare_branch_ab == expected.jit_wasm_compare_branch_ab
        && saved.jit_wasm_affine_pair_ab == expected.jit_wasm_affine_pair_ab
        && saved.jit_wasm_reference_body_cache_ab == expected.jit_wasm_reference_body_cache_ab
        && saved.jit_wasm_memcpy_leaf_ab == expected.jit_wasm_memcpy_leaf_ab
        && saved.jit_wasm_selective_ab == expected.jit_wasm_selective_ab
        && saved.rounds_requested == expected.rounds_requested
        && saved.timeout_ms == expected.timeout_ms
        && saved.source_revision == expected.source_revision
        && saved.source_dirty == expected.source_dirty
        && same_json(&saved.corpus, &expected.corpus)
        && same_json(&saved.host, &expected.host)
        && same_json(&saved.engines, &expected.engines)
        && same_json(&saved.suite_inputs, &expected.suite_inputs)
        && saved.selected_fixtures == expected.selected_fixtures
}

fn same_json<T: Serialize>(left: &T, right: &T) -> bool {
    serde_json::to_value(left).ok() == serde_json::to_value(right).ok()
}

fn jit_wasm_root_only_error(
    suite: Suite,
    jit_mode: JitMode,
    root: Option<u32>,
    root_only: bool,
) -> Option<&'static str> {
    (root_only && (suite != Suite::JetStream3 || jit_mode != JitMode::Jit || root.is_none()))
        .then_some("--jit-wasm-root-only requires --suite jetstream3 --jit --jit-wasm-root ID")
}

fn jit_wasm_root_first_error(
    suite: Suite,
    jit_mode: JitMode,
    root: Option<u32>,
    root_first: bool,
    root_only: bool,
) -> Option<&'static str> {
    (root_first
        && (suite != Suite::JetStream3
            || jit_mode != JitMode::Jit
            || root.is_none()
            || root_only))
        .then_some("--jit-wasm-root-first requires --suite jetstream3 --jit --jit-wasm-root ID and cannot combine with --jit-wasm-root-only")
}

fn jit_wasm_selective_error(
    suite: Suite,
    jit_mode: JitMode,
    root: Option<u32>,
    root_only: bool,
    selective: bool,
) -> Option<&'static str> {
    (selective
        && (suite != Suite::JetStream3
            || jit_mode != JitMode::Jit
            || root.is_some()
            || root_only))
        .then_some("--jit-wasm-selective requires --suite jetstream3 --jit and cannot combine with --jit-wasm-root, --jit-wasm-root-only, or --jit-wasm-root-first")
}

fn jit_wasm_memcpy_ab_error(options: &Options) -> Option<&'static str> {
    let mode_count = [
        options.jit_wasm_memcpy_ab,
        options.jit_wasm_memcpy_raw_ab,
        options.jit_wasm_memcpy_early_ab,
    ]
    .into_iter()
    .filter(|selected| *selected)
    .count();
    if mode_count == 0 {
        return None;
    }
    if mode_count > 1 {
        return Some("Wasm memcpy A/B diagnostic modes are mutually exclusive");
    }
    let raw_i32 = options.jit_wasm_memcpy_raw_ab;
    let early_import = options.jit_wasm_memcpy_early_ab;
    let minimum_rounds = if early_import {
        MIN_DIAGNOSTIC_SCREEN_ROUNDS
    } else {
        MIN_QUALIFYING_ROUNDS
    };
    if options.suite != Suite::JetStream3
        || options.jit_mode != JitMode::Jit
        || options.rounds < minimum_rounds
        || options.jit_diagnostics
        || options.jit_wasm_root.is_some()
        || options.jit_wasm_root_only
        || options.jit_wasm_root_first
        || options.jit_wasm_selective
        || options.fixed_work
        || options.quench_peer.is_some()
    {
        return Some(if raw_i32 {
            "--jit-wasm-memcpy-raw-ab requires --suite jetstream3 --jit, at least 11 rounds, and no other JIT diagnostic modes"
        } else if early_import {
            "--jit-wasm-memcpy-early-ab requires --suite jetstream3 --jit, at least 3 rounds, and no other JIT diagnostic modes"
        } else {
            "--jit-wasm-memcpy-ab requires --suite jetstream3 --jit, at least 11 rounds, and no other JIT diagnostic modes"
        });
    }
    if options.preflight_only {
        return (options.fixture.is_some() || options.fixtures.is_some() || options.all)
            .then_some("--preflight-only cannot select fixtures");
    }
    let one_pinned_fixture = options.fixture.as_deref() == Some("gcc-loops-wasm")
        || options
            .fixtures
            .as_ref()
            .is_some_and(|fixtures| fixtures.len() == 1 && fixtures[0] == "gcc-loops-wasm");
    (!one_pinned_fixture || options.all).then_some(if raw_i32 {
        "--jit-wasm-memcpy-raw-ab is pinned to the single JetStream fixture gcc-loops-wasm"
    } else if early_import {
        "--jit-wasm-memcpy-early-ab is pinned to the single JetStream fixture gcc-loops-wasm"
    } else {
        "--jit-wasm-memcpy-ab is pinned to the single JetStream fixture gcc-loops-wasm"
    })
}

fn jit_wasm_compare_branch_ab_error(options: &Options) -> Option<&'static str> {
    if !options.jit_wasm_compare_branch_ab {
        return None;
    }
    if options.jit_wasm_memcpy_ab
        || options.jit_wasm_memcpy_raw_ab
        || options.jit_wasm_memcpy_early_ab
    {
        return Some("--jit-wasm-compare-branch-ab cannot be combined with a memcpy A/B mode");
    }
    if options.suite != Suite::JetStream3
        || options.jit_mode != JitMode::Jit
        || options.rounds < MIN_DIAGNOSTIC_SCREEN_ROUNDS
        || options.jit_diagnostics
        || options.jit_wasm_root.is_some()
        || options.jit_wasm_root_only
        || options.jit_wasm_root_first
        || options.jit_wasm_selective
        || options.fixed_work
        || options.quench_peer.is_some()
    {
        return Some(
            "--jit-wasm-compare-branch-ab requires --suite jetstream3 --jit, at least 3 rounds, and no other JIT diagnostic modes",
        );
    }
    if options.preflight_only {
        return (options.fixture.is_some() || options.fixtures.is_some() || options.all)
            .then_some("--preflight-only cannot select fixtures");
    }
    let one_pinned_fixture = options.fixture.as_deref() == Some("quicksort-wasm")
        || options
            .fixtures
            .as_ref()
            .is_some_and(|fixtures| fixtures.len() == 1 && fixtures[0] == "quicksort-wasm");
    (!one_pinned_fixture || options.all).then_some(
        "--jit-wasm-compare-branch-ab is pinned to the single JetStream fixture quicksort-wasm",
    )
}

fn jit_wasm_affine_pair_ab_error(options: &Options) -> Option<&'static str> {
    if !options.jit_wasm_affine_pair_ab {
        return None;
    }
    if options.jit_wasm_memcpy_ab
        || options.jit_wasm_memcpy_raw_ab
        || options.jit_wasm_memcpy_early_ab
        || options.jit_wasm_compare_branch_ab
    {
        return Some("--jit-wasm-affine-pair-ab cannot be combined with another JIT A/B mode");
    }
    if options.suite != Suite::JetStream3
        || options.jit_mode != JitMode::Jit
        || options.rounds < MIN_DIAGNOSTIC_SCREEN_ROUNDS
        || options.jit_diagnostics
        || options.jit_wasm_root.is_some()
        || options.jit_wasm_root_only
        || options.jit_wasm_root_first
        || options.jit_wasm_selective
        || options.fixed_work
        || options.quench_peer.is_some()
    {
        return Some(
            "--jit-wasm-affine-pair-ab requires --suite jetstream3 --jit, at least 3 rounds, and no other JIT diagnostic modes",
        );
    }
    if options.preflight_only {
        return (options.fixture.is_some() || options.fixtures.is_some() || options.all)
            .then_some("--preflight-only cannot select fixtures");
    }
    let one_pinned_fixture = options.fixture.as_deref() == Some("gcc-loops-wasm")
        || options
            .fixtures
            .as_ref()
            .is_some_and(|fixtures| fixtures.len() == 1 && fixtures[0] == "gcc-loops-wasm");
    (!one_pinned_fixture || options.all).then_some(
        "--jit-wasm-affine-pair-ab is pinned to the single JetStream fixture gcc-loops-wasm",
    )
}

fn jit_wasm_reference_body_cache_ab_error(options: &Options) -> Option<&'static str> {
    if !options.jit_wasm_reference_body_cache_ab {
        return None;
    }
    if options.jit_wasm_memcpy_ab
        || options.jit_wasm_memcpy_raw_ab
        || options.jit_wasm_memcpy_early_ab
        || options.jit_wasm_compare_branch_ab
        || options.jit_wasm_affine_pair_ab
    {
        return Some(
            "--jit-wasm-reference-body-cache-ab cannot be combined with another JIT A/B mode",
        );
    }
    if options.suite != Suite::JetStream3
        || options.jit_mode != JitMode::Jit
        || options.rounds < MIN_DIAGNOSTIC_SCREEN_ROUNDS
        || options.jit_diagnostics
        || options.jit_wasm_root.is_some()
        || options.jit_wasm_root_only
        || options.jit_wasm_root_first
        || options.jit_wasm_selective
        || options.fixed_work
        || options.quench_peer.is_some()
    {
        return Some(
            "--jit-wasm-reference-body-cache-ab requires --suite jetstream3 --jit, at least 3 rounds, and no other JIT diagnostic modes",
        );
    }
    if options.preflight_only {
        return (options.fixture.is_some() || options.fixtures.is_some() || options.all)
            .then_some("--preflight-only cannot select fixtures");
    }
    let supported_fixture = options
        .fixture
        .as_deref()
        .is_some_and(|fixture| matches!(fixture, "tsf-wasm" | "sqlite3-wasm"))
        || options.fixtures.as_ref().is_some_and(|fixtures| {
            fixtures.len() == 1
                && matches!(fixtures[0].as_str(), "tsf-wasm" | "sqlite3-wasm")
        });
    (!supported_fixture || options.all).then_some(
        "--jit-wasm-reference-body-cache-ab supports one JetStream fixture: tsf-wasm or sqlite3-wasm",
    )
}

fn jit_wasm_memcpy_leaf_ab_error(options: &Options) -> Option<&'static str> {
    if !options.jit_wasm_memcpy_leaf_ab {
        return None;
    }
    if options.jit_wasm_memcpy_ab
        || options.jit_wasm_memcpy_raw_ab
        || options.jit_wasm_memcpy_early_ab
        || options.jit_wasm_compare_branch_ab
        || options.jit_wasm_affine_pair_ab
        || options.jit_wasm_reference_body_cache_ab
    {
        return Some("--jit-wasm-memcpy-leaf-ab cannot be combined with another JIT A/B mode");
    }
    if options.suite != Suite::JetStream3
        || options.jit_mode != JitMode::Jit
        || options.rounds < MIN_DIAGNOSTIC_SCREEN_ROUNDS
        || options.jit_diagnostics
        || options.jit_wasm_root.is_some()
        || options.jit_wasm_root_only
        || options.jit_wasm_root_first
        || options.jit_wasm_selective
        || options.fixed_work
        || options.quench_peer.is_some()
    {
        return Some(
            "--jit-wasm-memcpy-leaf-ab requires --suite jetstream3 --jit, at least 3 rounds, and no other JIT diagnostic modes",
        );
    }
    if options.preflight_only {
        return (options.fixture.is_some() || options.fixtures.is_some() || options.all)
            .then_some("--preflight-only cannot select fixtures");
    }
    let one_pinned_fixture = options.fixture.as_deref() == Some("gcc-loops-wasm")
        || options
            .fixtures
            .as_ref()
            .is_some_and(|fixtures| fixtures.len() == 1 && fixtures[0] == "gcc-loops-wasm");
    (!one_pinned_fixture || options.all).then_some(
        "--jit-wasm-memcpy-leaf-ab is pinned to the single JetStream fixture gcc-loops-wasm",
    )
}

fn jit_wasm_selective_ab_error(options: &Options) -> Option<&'static str> {
    if !options.jit_wasm_selective_ab {
        return None;
    }
    if options.jit_wasm_memcpy_ab
        || options.jit_wasm_memcpy_raw_ab
        || options.jit_wasm_memcpy_early_ab
        || options.jit_wasm_compare_branch_ab
        || options.jit_wasm_affine_pair_ab
        || options.jit_wasm_reference_body_cache_ab
        || options.jit_wasm_memcpy_leaf_ab
    {
        return Some("--jit-wasm-selective-ab cannot be combined with another JIT A/B mode");
    }
    if options.suite != Suite::JetStream3
        || options.jit_mode != JitMode::Jit
        || options.rounds < MIN_DIAGNOSTIC_SCREEN_ROUNDS
        || options.jit_diagnostics
        || options.jit_wasm_root.is_some()
        || options.jit_wasm_root_only
        || options.jit_wasm_root_first
        || options.jit_wasm_selective
        || options.fixed_work
        || options.quench_peer.is_some()
    {
        return Some(
            "--jit-wasm-selective-ab requires --suite jetstream3 --jit, at least 3 rounds, and no other JIT diagnostic modes",
        );
    }
    if options.preflight_only {
        return (options.fixture.is_some() || options.fixtures.is_some() || options.all)
            .then_some("--preflight-only cannot select fixtures");
    }
    let one_pinned_fixture = options.fixture.as_deref() == Some("gcc-loops-wasm")
        || options
            .fixtures
            .as_ref()
            .is_some_and(|fixtures| fixtures.len() == 1 && fixtures[0] == "gcc-loops-wasm");
    (!one_pinned_fixture || options.all).then_some(
        "--jit-wasm-selective-ab is pinned to the single JetStream fixture gcc-loops-wasm",
    )
}

fn jit_js_general_dispatch_error(options: &Options) -> Option<&'static str> {
    if !options.jit_js_general_dispatch {
        return None;
    }
    (options.suite != Suite::JetStream3 || options.jit_mode != JitMode::Jit)
        .then_some("--jit-js-general-dispatch requires --suite jetstream3 --jit")
}

fn parse_options() -> Options {
    parse_options_from(env::args().skip(1))
}

fn parse_options_from(args: impl IntoIterator<Item = String>) -> Options {
    let mut args = args.into_iter();
    let first = args
        .next()
        .unwrap_or_else(|| usage("missing fixture or --all"));
    let mut options = Options {
        suite: Suite::V8V7,
        jit_mode: JitMode::NoJit,
        jit_diagnostics: false,
        jit_js_general_dispatch: false,
        jit_wasm_root: None,
        jit_wasm_root_only: false,
        jit_wasm_root_first: false,
        jit_wasm_selective: false,
        jit_wasm_memcpy_ab: false,
        jit_wasm_memcpy_raw_ab: false,
        jit_wasm_memcpy_early_ab: false,
        jit_wasm_compare_branch_ab: false,
        jit_wasm_affine_pair_ab: false,
        jit_wasm_reference_body_cache_ab: false,
        jit_wasm_memcpy_leaf_ab: false,
        jit_wasm_selective_ab: false,
        fixture: None,
        fixtures: None,
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
    if first == "--fixtures" {
        let list = args
            .next()
            .unwrap_or_else(|| usage("--fixtures requires a comma-separated list"));
        options.fixtures = Some(parse_fixture_list(&list));
    } else if !options.all && !options.preflight_only {
        options.fixture = Some(first);
    }
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--suite" => {
                let name = args
                    .next()
                    .unwrap_or_else(|| usage("missing value for --suite"));
                options.suite = Suite::parse(&name);
            }
            "--fixtures" => {
                if options.fixture.is_some() || options.fixtures.is_some() {
                    usage("--fixtures cannot be combined with a positional fixture");
                }
                let list = args
                    .next()
                    .unwrap_or_else(|| usage("--fixtures requires a comma-separated list"));
                options.fixtures = Some(parse_fixture_list(&list));
            }
            "--jit" => options.jit_mode = JitMode::Jit,
            "--jit-diagnostics" => options.jit_diagnostics = true,
            "--jit-js-general-dispatch" => options.jit_js_general_dispatch = true,
            "--jit-wasm-selective" => options.jit_wasm_selective = true,
            "--jit-wasm-memcpy-ab" => options.jit_wasm_memcpy_ab = true,
            "--jit-wasm-memcpy-raw-ab" => options.jit_wasm_memcpy_raw_ab = true,
            "--jit-wasm-memcpy-early-ab" => options.jit_wasm_memcpy_early_ab = true,
            "--jit-wasm-compare-branch-ab" => options.jit_wasm_compare_branch_ab = true,
            "--jit-wasm-affine-pair-ab" => options.jit_wasm_affine_pair_ab = true,
            "--jit-wasm-reference-body-cache-ab" => options.jit_wasm_reference_body_cache_ab = true,
            "--jit-wasm-memcpy-leaf-ab" => options.jit_wasm_memcpy_leaf_ab = true,
            "--jit-wasm-selective-ab" => options.jit_wasm_selective_ab = true,
            "--jit-wasm-root-only" => options.jit_wasm_root_only = true,
            "--jit-wasm-root-first" => options.jit_wasm_root_first = true,
            "--jit-wasm-root" => {
                options.jit_wasm_root = Some(
                    u32::try_from(required_number(&mut args, "--jit-wasm-root"))
                        .unwrap_or_else(|_| usage("--jit-wasm-root is too large")),
                )
            }
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
    if options.preflight_only && (options.fixture.is_some() || options.fixtures.is_some()) {
        usage("--preflight-only does not take fixtures");
    }
    if options.checkpoint.is_some() && options.resume.is_some() {
        usage("--checkpoint and --resume are mutually exclusive");
    }
    if (options.checkpoint.is_some() || options.resume.is_some())
        && !options.all
        && options.fixtures.is_none()
    {
        usage("--checkpoint and --resume require --all or --fixtures");
    }
    if options.all && options.fixtures.is_some() {
        usage("use --fixtures by itself; it already selects an explicit fixture set");
    }
    if options.fixtures.is_some() && options.suite != Suite::JetStream3 {
        usage("--fixtures is currently supported only with --suite jetstream3");
    }
    if options.preflight_only && (options.checkpoint.is_some() || options.resume.is_some()) {
        usage("--preflight-only cannot use checkpoints");
    }
    if options.fixed_work && options.suite != Suite::V8V7 {
        usage("--fixed-work measures V8-v7 fixtures only");
    }
    if options.jit_mode == JitMode::Jit && options.suite != Suite::JetStream3 {
        usage("--jit is currently supported only with --suite jetstream3");
    }
    if options.jit_diagnostics && options.jit_mode != JitMode::Jit {
        usage("--jit-diagnostics requires --suite jetstream3 --jit");
    }
    if let Some(error) = jit_js_general_dispatch_error(&options) {
        usage(error);
    }
    if options.jit_wasm_root.is_some()
        && (options.jit_mode != JitMode::Jit || options.suite != Suite::JetStream3)
    {
        usage("--jit-wasm-root requires --suite jetstream3 --jit");
    }
    if let Some(error) = jit_wasm_root_only_error(
        options.suite,
        options.jit_mode,
        options.jit_wasm_root,
        options.jit_wasm_root_only,
    ) {
        usage(error);
    }
    if let Some(error) = jit_wasm_root_first_error(
        options.suite,
        options.jit_mode,
        options.jit_wasm_root,
        options.jit_wasm_root_first,
        options.jit_wasm_root_only,
    ) {
        usage(error);
    }
    if let Some(error) = jit_wasm_selective_error(
        options.suite,
        options.jit_mode,
        options.jit_wasm_root,
        options.jit_wasm_root_only,
        options.jit_wasm_selective,
    ) {
        usage(error);
    }
    if let Some(error) = jit_wasm_memcpy_ab_error(&options) {
        usage(error);
    }
    if let Some(error) = jit_wasm_compare_branch_ab_error(&options) {
        usage(error);
    }
    if let Some(error) = jit_wasm_affine_pair_ab_error(&options) {
        usage(error);
    }
    if let Some(error) = jit_wasm_reference_body_cache_ab_error(&options) {
        usage(error);
    }
    if let Some(error) = jit_wasm_memcpy_leaf_ab_error(&options) {
        usage(error);
    }
    if let Some(error) = jit_wasm_selective_ab_error(&options) {
        usage(error);
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

fn parse_fixture_list(list: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut seen = BTreeSet::new();
    for name in list.split(',').map(str::trim) {
        if name.is_empty() || !seen.insert(name.to_string()) {
            usage("--fixtures must be a non-empty comma-separated list without duplicates");
        }
        names.push(name.to_string());
    }
    names
}

fn fixture_selection_is_valid(suite: Suite, files: &[PathBuf]) -> bool {
    if files.is_empty() {
        return false;
    }
    let names = files
        .iter()
        .filter_map(|file| file.file_name()?.to_str())
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
    if names.len() != files.len()
        || names
            .iter()
            .any(|name| !suite.fixture_names().contains(&name.as_str()))
    {
        return false;
    }
    match suite {
        Suite::JetStream3 => files.iter().all(|file| {
            file.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| suite.fixture(name) == *file)
        }),
        Suite::V8V7 => {
            files.iter().cloned().collect::<BTreeSet<_>>()
                == suite.fixtures().into_iter().collect::<BTreeSet<_>>()
        }
    }
}

fn prepare_engines(options: &Options) -> Vec<EngineSpec> {
    let engines = if options.jit_wasm_memcpy_leaf_ab {
        let [leaf_off, leaf_on] = quench_memcpy_leaf_ab_pair(options);
        vec![leaf_off, leaf_on, bun_engine(options), node_engine(options)]
    } else if options.jit_wasm_selective_ab {
        let [selective_off, selective_on] = quench_selective_ab_pair(options);
        vec![
            selective_off,
            selective_on,
            bun_engine(options),
            node_engine(options),
        ]
    } else if options.jit_wasm_reference_body_cache_ab {
        let [cache_off, cache_on] = quench_reference_body_cache_ab_pair(options);
        vec![
            cache_off,
            cache_on,
            bun_engine(options),
            node_engine(options),
        ]
    } else if options.jit_wasm_affine_pair_ab {
        let [affine_off, affine_on] = quench_affine_pair_ab_pair(options);
        vec![
            affine_off,
            affine_on,
            bun_engine(options),
            node_engine(options),
        ]
    } else if options.jit_wasm_compare_branch_ab {
        let [branch_off, branch_on] = quench_compare_branch_ab_pair(options);
        vec![
            branch_off,
            branch_on,
            bun_engine(options),
            node_engine(options),
        ]
    } else if options.jit_wasm_memcpy_early_ab {
        let [early_off, early_on] = quench_memcpy_early_ab_pair(options);
        vec![
            early_off,
            early_on,
            bun_engine(options),
            node_engine(options),
        ]
    } else if options.jit_wasm_memcpy_ab || options.jit_wasm_memcpy_raw_ab {
        let [direct_off, direct_on] =
            quench_memcpy_ab_pair(options, options.jit_wasm_memcpy_raw_ab);
        vec![
            direct_off,
            direct_on,
            bun_engine(options),
            node_engine(options),
        ]
    } else {
        options
            .suite
            .engine_names()
            .iter()
            .map(|name| match *name {
                "quench" => quench_engine(options),
                "quickjs" => quickjs_engine(options),
                "bun" => bun_engine(options),
                "node" => node_engine(options),
                _ => unreachable!("suites name only known engines"),
            })
            .collect()
    };
    engines
        .into_iter()
        .map(|mut engine| {
            engine.webassembly = probe_webassembly(&engine);
            engine
        })
        .collect()
}

fn quench_selective_ab_pair(options: &Options) -> [EngineSpec; 2] {
    let mut selective_off = quench_engine(options);
    let mut selective_on = quench_engine(options);
    selective_off.name = "quench_selective_handoff_off";
    selective_on.name = "quench_selective_handoff_on";
    selective_off
        .env
        .insert(QUENCH_WASM_JIT_SELECTIVE.into(), "0".into());
    selective_on
        .env
        .insert(QUENCH_WASM_JIT_SELECTIVE.into(), "1".into());
    selective_off.jit_proof = format!(
        "{}; diagnostic selective CallKnown handoff A/B arm with {QUENCH_WASM_JIT_SELECTIVE}=0; not for qualification",
        selective_off.jit_proof
    );
    selective_on.jit_proof = format!(
        "{}; diagnostic selective CallKnown handoff A/B arm with {QUENCH_WASM_JIT_SELECTIVE}=1; not for qualification",
        selective_on.jit_proof
    );
    [selective_off, selective_on]
}

fn quench_affine_pair_ab_pair(options: &Options) -> [EngineSpec; 2] {
    let mut affine_off = quench_engine(options);
    let mut affine_on = quench_engine(options);
    affine_off.name = "quench_affine_pair_off";
    affine_on.name = "quench_affine_pair_on";
    affine_off
        .env
        .insert(QUENCH_WASM_JIT_AFFINE_PAIR.into(), "0".into());
    affine_on
        .env
        .insert(QUENCH_WASM_JIT_AFFINE_PAIR.into(), "1".into());
    affine_off.jit_proof = format!(
        "{}; diagnostic affine-pair A/B arm with {QUENCH_WASM_JIT_AFFINE_PAIR}=0",
        affine_off.jit_proof
    );
    affine_on.jit_proof = format!(
        "{}; diagnostic affine-pair A/B arm with {QUENCH_WASM_JIT_AFFINE_PAIR}=1",
        affine_on.jit_proof
    );
    [affine_off, affine_on]
}

fn quench_reference_body_cache_ab_pair(options: &Options) -> [EngineSpec; 2] {
    let mut cache_off = quench_engine(options);
    let mut cache_on = quench_engine(options);
    cache_off.name = "quench_reference_body_cache_off";
    cache_on.name = "quench_reference_body_cache_on";
    cache_off
        .env
        .insert(QUENCH_WASM_JIT_REFERENCE_BODY_CACHE.into(), "0".into());
    cache_on
        .env
        .insert(QUENCH_WASM_JIT_REFERENCE_BODY_CACHE.into(), "1".into());
    cache_off.jit_proof = format!(
        "{}; diagnostic reference-body-cache A/B arm with {QUENCH_WASM_JIT_REFERENCE_BODY_CACHE}=0",
        cache_off.jit_proof
    );
    cache_on.jit_proof = format!(
        "{}; diagnostic reference-body-cache A/B arm with {QUENCH_WASM_JIT_REFERENCE_BODY_CACHE}=1",
        cache_on.jit_proof
    );
    [cache_off, cache_on]
}

fn quench_compare_branch_ab_pair(options: &Options) -> [EngineSpec; 2] {
    let mut branch_off = quench_engine(options);
    let mut branch_on = quench_engine(options);
    branch_off.name = "quench_direct_compare_branches_off";
    branch_on.name = "quench_direct_compare_branches_on";
    branch_off
        .env
        .insert(QUENCH_WASM_JIT_DIRECT_COMPARE_BRANCHES.into(), "0".into());
    branch_on
        .env
        .insert(QUENCH_WASM_JIT_DIRECT_COMPARE_BRANCHES.into(), "1".into());
    branch_off.jit_proof = format!(
        "{}; diagnostic direct compare-branch A/B arm with {QUENCH_WASM_JIT_DIRECT_COMPARE_BRANCHES}=0",
        branch_off.jit_proof
    );
    branch_on.jit_proof = format!(
        "{}; diagnostic direct compare-branch A/B arm with {QUENCH_WASM_JIT_DIRECT_COMPARE_BRANCHES}=1",
        branch_on.jit_proof
    );
    [branch_off, branch_on]
}

fn quench_memcpy_ab_pair(options: &Options, raw_i32: bool) -> [EngineSpec; 2] {
    let mut direct_off = quench_engine(options);
    let mut direct_on = quench_engine(options);
    if raw_i32 {
        direct_off.name = "quench_memcpy_raw_i32_off";
        direct_on.name = "quench_memcpy_raw_i32_on";
        for engine in [&mut direct_off, &mut direct_on] {
            engine
                .env
                .insert(QUENCH_WASM_EMSCRIPTEN_MEMCPY_DIRECT.into(), "1".into());
        }
        direct_off
            .env
            .insert(QUENCH_WASM_EMSCRIPTEN_MEMCPY_RAW_I32.into(), "0".into());
        direct_on
            .env
            .insert(QUENCH_WASM_EMSCRIPTEN_MEMCPY_RAW_I32.into(), "1".into());
        direct_off.jit_proof = format!(
            "{}; diagnostic raw-i32 A/B arm with {QUENCH_WASM_EMSCRIPTEN_MEMCPY_DIRECT}=1 and {QUENCH_WASM_EMSCRIPTEN_MEMCPY_RAW_I32}=0",
            direct_off.jit_proof
        );
        direct_on.jit_proof = format!(
            "{}; diagnostic raw-i32 A/B arm with {QUENCH_WASM_EMSCRIPTEN_MEMCPY_DIRECT}=1 and {QUENCH_WASM_EMSCRIPTEN_MEMCPY_RAW_I32}=1",
            direct_on.jit_proof
        );
    } else {
        direct_off.name = "quench_memcpy_direct_off";
        direct_on.name = "quench_memcpy_direct_on";
        direct_off
            .env
            .insert(QUENCH_WASM_EMSCRIPTEN_MEMCPY_DIRECT.into(), "0".into());
        direct_on
            .env
            .insert(QUENCH_WASM_EMSCRIPTEN_MEMCPY_DIRECT.into(), "1".into());
        direct_off.jit_proof = format!(
            "{}; diagnostic A/B arm with {QUENCH_WASM_EMSCRIPTEN_MEMCPY_DIRECT}=0",
            direct_off.jit_proof
        );
        direct_on.jit_proof = format!(
            "{}; diagnostic A/B arm with {QUENCH_WASM_EMSCRIPTEN_MEMCPY_DIRECT}=1",
            direct_on.jit_proof
        );
    }
    [direct_off, direct_on]
}

fn quench_memcpy_early_ab_pair(options: &Options) -> [EngineSpec; 2] {
    let mut early_off = quench_engine(options);
    let mut early_on = quench_engine(options);
    early_off.name = "quench_memcpy_early_import_off";
    early_on.name = "quench_memcpy_early_import_on";
    for engine in [&mut early_off, &mut early_on] {
        engine
            .env
            .insert(QUENCH_WASM_EMSCRIPTEN_MEMCPY_DIRECT.into(), "1".into());
        engine
            .env
            .insert(QUENCH_WASM_EMSCRIPTEN_MEMCPY_RAW_I32.into(), "1".into());
    }
    early_off.env.insert(
        QUENCH_WASM_EMSCRIPTEN_MEMCPY_EARLY_IMPORT.into(),
        "0".into(),
    );
    early_on.env.insert(
        QUENCH_WASM_EMSCRIPTEN_MEMCPY_EARLY_IMPORT.into(),
        "1".into(),
    );
    early_off.jit_proof = format!(
        "{}; diagnostic early-import A/B arm with {QUENCH_WASM_EMSCRIPTEN_MEMCPY_DIRECT}=1, {QUENCH_WASM_EMSCRIPTEN_MEMCPY_RAW_I32}=1, and {QUENCH_WASM_EMSCRIPTEN_MEMCPY_EARLY_IMPORT}=0",
        early_off.jit_proof
    );
    early_on.jit_proof = format!(
        "{}; diagnostic early-import A/B arm with {QUENCH_WASM_EMSCRIPTEN_MEMCPY_DIRECT}=1, {QUENCH_WASM_EMSCRIPTEN_MEMCPY_RAW_I32}=1, and {QUENCH_WASM_EMSCRIPTEN_MEMCPY_EARLY_IMPORT}=1",
        early_on.jit_proof
    );
    [early_off, early_on]
}

fn quench_memcpy_leaf_ab_pair(options: &Options) -> [EngineSpec; 2] {
    let mut leaf_off = quench_engine(options);
    let mut leaf_on = quench_engine(options);
    leaf_off.name = "quench_memcpy_native_leaf_off";
    leaf_on.name = "quench_memcpy_native_leaf_on";
    for engine in [&mut leaf_off, &mut leaf_on] {
        engine
            .env
            .insert(QUENCH_WASM_EMSCRIPTEN_MEMCPY_DIRECT.into(), "1".into());
        engine
            .env
            .insert(QUENCH_WASM_EMSCRIPTEN_MEMCPY_RAW_I32.into(), "1".into());
        engine.env.insert(
            QUENCH_WASM_EMSCRIPTEN_MEMCPY_EARLY_IMPORT.into(),
            "1".into(),
        );
    }
    leaf_off
        .env
        .insert(QUENCH_WASM_JIT_EMSCRIPTEN_MEMCPY_LEAF.into(), "0".into());
    leaf_on
        .env
        .insert(QUENCH_WASM_JIT_EMSCRIPTEN_MEMCPY_LEAF.into(), "1".into());
    leaf_off.jit_proof = format!(
        "{}; diagnostic native memcpy leaf A/B arm with {QUENCH_WASM_EMSCRIPTEN_MEMCPY_DIRECT}=1, {QUENCH_WASM_EMSCRIPTEN_MEMCPY_RAW_I32}=1, {QUENCH_WASM_EMSCRIPTEN_MEMCPY_EARLY_IMPORT}=1, and {QUENCH_WASM_JIT_EMSCRIPTEN_MEMCPY_LEAF}=0; not for qualification",
        leaf_off.jit_proof
    );
    leaf_on.jit_proof = format!(
        "{}; diagnostic native memcpy leaf A/B arm with {QUENCH_WASM_EMSCRIPTEN_MEMCPY_DIRECT}=1, {QUENCH_WASM_EMSCRIPTEN_MEMCPY_RAW_I32}=1, {QUENCH_WASM_EMSCRIPTEN_MEMCPY_EARLY_IMPORT}=1, and {QUENCH_WASM_JIT_EMSCRIPTEN_MEMCPY_LEAF}=1; not for qualification",
        leaf_on.jit_proof
    );
    [leaf_off, leaf_on]
}

/// Runs `typeof WebAssembly` with the engine's measurement arguments and environment.
fn probe_webassembly(engine: &EngineSpec) -> bool {
    let mut args = engine.argv.iter().map(String::as_str).collect::<Vec<_>>();
    args.extend(["-e", "console.log(typeof WebAssembly)"]);
    let probe = capture(&engine.executable, &args, &engine.env);
    probe.status == 0 && probe.stdout.trim() == "object"
}

fn node_engine(options: &Options) -> EngineSpec {
    let no_jit = options.jit_mode == JitMode::NoJit;
    let mut node = spec(
        "node",
        &options.node,
        if no_jit {
            vec!["--jitless".into()]
        } else {
            Vec::new()
        },
        BTreeMap::new(),
    );
    node.version = version(&node.executable, &["--version"], &node.env);
    if no_jit {
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
    } else {
        let source = "function hot(x) { return x + 1; } for (let i = 0; i < 100000; i++) hot(i);";
        let arguments = ["--trace-opt", "--no-concurrent-recompilation", "-e", source];
        node.jit_proof_command = std::iter::once(node.executable.display().to_string())
            .chain(arguments.iter().map(|argument| (*argument).to_string()))
            .collect();
        let proof = capture(&node.executable, &arguments, &node.env);
        node.jit_proof = require_proof("Node optimizing JIT", &proof, "completed compiling");
        node.jit_mode = "enabled-and-probed";
    }
    node
}

fn bun_engine(options: &Options) -> EngineSpec {
    let mut bun_env = BTreeMap::new();
    bun_env.insert(
        "BUN_JSC_useJIT".into(),
        if options.jit_mode == JitMode::NoJit {
            "0"
        } else {
            "1"
        }
        .into(),
    );
    let mut bun = spec("bun", &options.bun, vec![], bun_env);
    bun.version = version(&bun.executable, &["--version"], &bun.env);
    let mut proof_env = bun.env.clone();
    proof_env.insert("BUN_JSC_dumpOptions".into(), "1".into());
    let proof = capture(
        &bun.executable,
        &["-e", "console.log(Bun.version)"],
        &proof_env,
    );
    bun.jit_proof_command = vec!["BUN_JSC_dumpOptions=1".into()];
    bun.jit_proof_command.extend([
        bun.executable.display().to_string(),
        "-e".into(),
        "console.log(Bun.version)".into(),
    ]);
    if options.jit_mode == JitMode::NoJit {
        bun.jit_proof_command.insert(0, "BUN_JSC_useJIT=0".into());
        bun.jit_proof = require_proof("BUN_JSC_useJIT=0 + JSC option dump", &proof, "useJIT=false");
    } else {
        bun.jit_proof_command.insert(0, "BUN_JSC_useJIT=1".into());
        bun.jit_proof = require_proof("Bun JSC optimizing JIT", &proof, "useJIT=true");
        bun.jit_mode = "enabled-and-probed";
    }
    bun
}

fn quickjs_engine(options: &Options) -> EngineSpec {
    let mut qjs = spec("quickjs", &options.qjs, vec![], BTreeMap::new());
    qjs.version = version(&qjs.executable, &["--version"], &qjs.env);
    qjs.jit_proof_command = vec![qjs.executable.display().to_string(), "--version".into()];
    qjs.jit_proof =
        "QuickJS qjs executes bytecode in its interpreter; binary version and SHA-256 are recorded"
            .into();
    qjs
}

fn quench_engine(options: &Options) -> EngineSpec {
    let jit_enabled = options.jit_mode == JitMode::Jit;
    let argv = if jit_enabled {
        vec!["--jit".into()]
    } else {
        Vec::new()
    };
    let mut environment = BTreeMap::new();
    if options.jit_diagnostics {
        environment.insert("QUENCH_WASM_JIT_STATS".into(), "1".into());
        environment.insert("QUENCH_JS_JIT_STATS".into(), "1".into());
    }
    if options.jit_js_general_dispatch {
        environment.insert(QUENCH_JS_JIT_GENERAL_DISPATCH.into(), "1".into());
    }
    if options.jit_wasm_root.is_some() {
        if let Ok(value) = std::env::var("QUENCH_WASM_JIT_ROOT_OPT_LEVEL") {
            environment.insert("QUENCH_WASM_JIT_ROOT_OPT_LEVEL".into(), value);
        }
    }
    if options.jit_wasm_selective {
        environment.insert("QUENCH_WASM_JIT_SELECTIVE".into(), "1".into());
    }
    if options.jit_wasm_root_only {
        environment.insert("QUENCH_WASM_JIT_ROOT_ONLY".into(), "1".into());
    }
    if options.jit_wasm_root_first {
        environment.insert("QUENCH_WASM_JIT_ROOT_FIRST".into(), "1".into());
    }
    if let Some(root) = options.jit_wasm_root {
        environment.insert("QUENCH_WASM_JIT_ROOT".into(), root.to_string());
    }
    let mut quench = spec("quench", &options.quench, argv, environment);
    let execution_tier = if jit_enabled { "JIT" } else { "interpreter" };
    quench.version = format!(
        "Quench shared-VM {execution_tier} build; package {}",
        env!("CARGO_PKG_VERSION")
    );
    quench.jit_proof_command = vec![quench.executable.display().to_string()];
    if jit_enabled {
        quench.jit_proof_command.push("--jit".into());
        quench.jit_mode = "enabled-by-cli-flag";
        quench.jit_proof = if options.jit_diagnostics {
            "the measured Quench process is launched with --jit and opt-in JavaScript/WebAssembly JIT counters; diagnostic-only, not for qualification".into()
        } else {
            "the measured Quench process is launched with --jit".into()
        };
        if options.jit_js_general_dispatch {
            quench.jit_proof = format!(
                "{}; experimental priced JavaScript GeneralDispatch enabled with QUENCH_JS_JIT_GENERAL_DISPATCH=1; not qualification mode",
                quench.jit_proof
            );
        }
        if let Some(root) = options.jit_wasm_root {
            quench.jit_proof = format!(
                "{}; bounded experimental root compilation selected residual function {root} via QUENCH_WASM_JIT_ROOT; not qualification mode",
                quench.jit_proof
            );
        }
        if options.jit_wasm_root_only {
            quench.jit_proof = format!(
                "{}; strict experimental single-root compilation enabled with QUENCH_WASM_JIT_ROOT_ONLY=1; hot callees remain interpreted; not qualification mode",
                quench.jit_proof
            );
        }
        if options.jit_wasm_root_first {
            quench.jit_proof = format!(
                "{}; experimental root-first tiering enabled with QUENCH_WASM_JIT_ROOT_FIRST=1; starts with the selected root and promotes hot fallback callees; not qualification mode",
                quench.jit_proof
            );
        }
        if options.jit_wasm_selective {
            quench.jit_proof = format!(
                "{}; opt-in selective Wasm hot-root tiering enabled with QUENCH_WASM_JIT_SELECTIVE=1; not qualification mode",
                quench.jit_proof
            );
        }
    } else {
        quench.jit_proof =
            "the measured shared VM executes interpreter bytecode; no guest JIT path is enabled"
                .into();
    }
    quench
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
        webassembly: false,
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
    if let Some(fixtures) = &options.fixtures {
        return fixtures
            .iter()
            .map(|fixture| options.suite.fixture(fixture))
            .collect();
    }
    if options.all {
        return options.suite.fixtures();
    }
    vec![options.suite.fixture(options.fixture.as_deref().unwrap())]
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
        corpus: corpus_record(Suite::V8V7, files),
        host,
        contention_policy: contention::POLICY,
        measurement_runner: artifact(&env::current_exe().expect("current benchmark executable")),
        engines: engine_records,
        suite_inputs: Suite::V8V7.inputs(),
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
            let mut sample = run(engine, &[source.as_os_str().to_owned()], timeout_ms);
            sample.score = v8_v7_score(&sample.stdout);
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
    suite: Suite,
    file: &Path,
    engines: &[EngineSpec],
    rounds: usize,
    timeout_ms: u64,
) -> FixtureRecord {
    let (supported, unsupported): (Vec<_>, Vec<_>) = engines
        .iter()
        .map(|engine| (engine, suite.unsupported_reason(file, engine)))
        .partition(|(_, reason)| reason.is_none());
    let unsupported_engines = unsupported
        .into_iter()
        .filter_map(|(engine, reason)| Some((engine.name.to_string(), reason?)))
        .collect();
    let engines = &supported
        .into_iter()
        .map(|(engine, _)| engine.clone())
        .collect::<Vec<_>>()[..];
    let (temporary, arguments) = suite.materialize(file);
    let mut results = Vec::with_capacity(rounds);
    for round in 0..rounds {
        let order = rotated_order(engines, round);
        let mut samples = BTreeMap::new();
        for engine in &order {
            let mut sample = run(engine, &arguments, timeout_ms);
            sample.score = suite.score(file, &sample.stdout);
            samples.insert(engine.name.to_string(), sample);
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
    let output_equal = outputs_equal(suite, &results, engines);
    let summaries = engines
        .iter()
        .map(|engine| (engine.name.to_string(), summary(&results, engine.name)))
        .collect();
    FixtureRecord {
        source: fixture_artifact(suite, file),
        valid,
        output_equal,
        summaries,
        unsupported_engines,
        rounds: results,
    }
}

fn rotated_order(engines: &[EngineSpec], round: usize) -> Vec<EngineSpec> {
    (0..engines.len())
        .map(|offset| engines[(offset + round) % engines.len()].clone())
        .collect()
}

fn outputs_equal(suite: Suite, rounds: &[RoundRecord], engines: &[EngineSpec]) -> bool {
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
                    && suite.semantic_output(&sample.stdout) == suite.semantic_output(&first.stdout)
            })
        })
    })
}

fn run(engine: &EngineSpec, arguments: &[OsString], timeout_ms: u64) -> Sample {
    let host_before = host_snapshot();
    let mut sample = run_measured(engine, arguments, timeout_ms);
    sample.host_before = Some(host_before);
    sample.host_after = Some(host_snapshot());
    sample
}

#[cfg(target_os = "macos")]
fn run_measured(engine: &EngineSpec, arguments: &[OsString], timeout_ms: u64) -> Sample {
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
        .args(arguments)
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
    Sample {
        status,
        timed_out,
        wall_ns: started.elapsed().as_nanos(),
        peak_rss_bytes: time_metric(&stderr, "maximum resident set size"),
        score: None,
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
fn run_measured(engine: &EngineSpec, arguments: &[OsString], timeout_ms: u64) -> Sample {
    run_wait4(engine, arguments, timeout_ms)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn run_wait4(engine: &EngineSpec, arguments: &[OsString], timeout_ms: u64) -> Sample {
    let started = Instant::now();
    let mut command = Command::new(&engine.executable);
    command
        .args(&engine.argv)
        .args(arguments)
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
    Sample {
        status,
        timed_out,
        wall_ns: started.elapsed().as_nanos(),
        peak_rss_bytes: usage.peak_rss_bytes,
        score: None,
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

fn temporary_path(name: &str) -> PathBuf {
    Path::new("/tmp").join(format!("quench-{}-{}-{name}", std::process::id(), now_ns()))
}

fn materialize_with_runner(file: &Path, runner: &[u8]) -> PathBuf {
    let name = file.file_name().unwrap_or_default().to_string_lossy();
    let path = temporary_path(&format!("v8-v7-{name}"));
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

fn v8_v7_score(stdout: &str) -> Option<f64> {
    stdout
        .lines()
        .find_map(|line| line.strip_prefix("Score: "))
        .and_then(|value| value.parse().ok())
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
        .args(["-A", "-o", "pid=,pcpu=,stat=,comm="])
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
            let state = fields.next()?;
            let command = fields.collect::<Vec<_>>().join(" ");
            (!state.starts_with('Z')
                && !command.is_empty()
                && cpu_percent.is_finite()
                && cpu_percent >= 0.0)
                .then_some(CpuConsumer {
                    pid,
                    cpu_percent,
                    command,
                })
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

fn corpus_record(suite: Suite, selected: &[PathBuf]) -> CorpusRecord {
    let corpus = suite.corpus();
    let pinned_revision = command_output("git", &["ls-tree", "HEAD", "--", corpus])
        .split_whitespace()
        .nth(2)
        .map(str::to_string);
    let checkout_revision = command_output("git", &["-C", corpus, "rev-parse", "HEAD"])
        .lines()
        .next()
        .map(str::to_string);
    let checkout_clean = command_output("git", &["-C", corpus, "status", "--porcelain"]).is_empty();
    let expected = suite.fixtures().into_iter().collect::<BTreeSet<_>>();
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

fn fixture_artifact(suite: Suite, file: &Path) -> Artifact {
    if suite != Suite::JetStream3 {
        return artifact(file);
    }
    let mut source = artifact(&jetstream::source_path(file));
    source.path = file.display().to_string();
    source
}

fn valid_sha256(hash: &str) -> bool {
    hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(crate) fn sha256(path: &Path) -> Option<String> {
    if fs::metadata(path).ok()?.is_dir() {
        static JETSTREAM_ROOT_HASH: OnceLock<Option<String>> = OnceLock::new();
        if fs::canonicalize(path).ok().is_some_and(|path| {
            fs::canonicalize(jetstream::SUITE_DIR).is_ok_and(|root| path == root)
        }) {
            return JETSTREAM_ROOT_HASH
                .get_or_init(|| sha256_directory(path))
                .clone();
        }
        return sha256_directory(path);
    }
    sha256_file(path)
}

fn sha256_file(path: &Path) -> Option<String> {
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

fn sha256_directory(root: &Path) -> Option<String> {
    fn collect(directory: &Path, entries: &mut Vec<PathBuf>) -> std::io::Result<()> {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            if entry.file_name() == ".git" {
                continue;
            }
            let path = entry.path();
            let file_type = entry.file_type()?;
            entries.push(path.clone());
            if file_type.is_dir() {
                collect(&path, entries)?;
            }
        }
        Ok(())
    }

    fn relative_name(root: &Path, path: &Path) -> Option<Vec<u8>> {
        let relative = path.strip_prefix(root).ok()?;
        let normalized = relative
            .components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        Some(normalized.into_bytes())
    }

    let mut entries = Vec::new();
    collect(root, &mut entries).ok()?;
    entries.sort_by(|left, right| relative_name(root, left).cmp(&relative_name(root, right)));

    let mut child = Command::new("sha256sum")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .or_else(|_| {
            Command::new("shasum")
                .args(["-a", "256"])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .spawn()
        })
        .ok()?;
    let mut input = child.stdin.take()?;
    for path in entries {
        let name = relative_name(root, &path)?;
        let metadata = fs::symlink_metadata(&path).ok()?;
        let file_type = metadata.file_type();
        let name_len = u64::try_from(name.len()).ok()?;
        if file_type.is_symlink() {
            let target = fs::read_link(&path).ok()?.to_string_lossy().into_owned();
            let target = target.as_bytes();
            input.write_all(b"L").ok()?;
            input.write_all(&name_len.to_be_bytes()).ok()?;
            input.write_all(&name).ok()?;
            input
                .write_all(&u64::try_from(target.len()).ok()?.to_be_bytes())
                .ok()?;
            input.write_all(target).ok()?;
        } else if file_type.is_dir() {
            input.write_all(b"D").ok()?;
            input.write_all(&name_len.to_be_bytes()).ok()?;
            input.write_all(&name).ok()?;
        } else if file_type.is_file() {
            input.write_all(b"F").ok()?;
            input.write_all(&name_len.to_be_bytes()).ok()?;
            input.write_all(&name).ok()?;
            input.write_all(&metadata.len().to_be_bytes()).ok()?;
            let mut file = fs::File::open(&path).ok()?;
            std::io::copy(&mut file, &mut input).ok()?;
        } else {
            return None;
        }
    }
    drop(input);
    let output = child.wait_with_output().ok()?;
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
        "{message}\nusage: quench-bench <fixture.js>|--all|--fixtures NAME,NAME,... [--suite v8-v7|jetstream3] [--jit] [--jit-diagnostics] [--jit-js-general-dispatch] [--jit-wasm-root ID] [--jit-wasm-root-only] [--jit-wasm-root-first] [--jit-wasm-selective] [--jit-wasm-memcpy-ab] [--jit-wasm-memcpy-raw-ab] [--jit-wasm-memcpy-early-ab] [--jit-wasm-memcpy-leaf-ab] [--jit-wasm-selective-ab] [--jit-wasm-compare-branch-ab] [--jit-wasm-affine-pair-ab] [--jit-wasm-reference-body-cache-ab] [--fixed-work] [--quench PATH] [--quench-peer PATH] [--qjs PATH] [--bun PATH] [--node PATH] [--runs N] [--timeout-ms N] [--checkpoint PATH|--resume PATH] [--out PATH]\n       quench-bench --preflight-only [--suite v8-v7|jetstream3] [--jit] [--jit-diagnostics] [--jit-js-general-dispatch] [--jit-wasm-root ID] [--jit-wasm-root-only] [--jit-wasm-root-first] [--jit-wasm-selective] [--jit-wasm-memcpy-ab] [--jit-wasm-memcpy-raw-ab] [--jit-wasm-memcpy-early-ab] [--jit-wasm-memcpy-leaf-ab] [--jit-wasm-selective-ab] [--jit-wasm-compare-branch-ab] [--jit-wasm-affine-pair-ab] [--jit-wasm-reference-body-cache-ab] [engine options]\n       quench-bench --analyze REPORT [--out JSON]"
    );
    std::process::exit(2)
}

fn fail(message: &str) -> ! {
    eprintln!("{message}");
    std::process::exit(2)
}

#[cfg(test)]
mod fixture_selection_tests {
    use super::{Suite, fixture_selection_is_valid};

    #[test]
    fn jetstream_subset_accepts_only_unique_pinned_lane_fixtures() {
        let selected = [
            "HashSet-wasm",
            "gcc-loops-wasm",
            "quicksort-wasm",
            "tsf-wasm",
            "zlib-wasm",
        ]
        .map(|name| Suite::JetStream3.fixture(name))
        .to_vec();
        assert!(fixture_selection_is_valid(Suite::JetStream3, &selected));

        let unknown = [Suite::JetStream3.fixture("unknown-wasm")];
        assert!(!fixture_selection_is_valid(Suite::JetStream3, &unknown));
        let duplicate = [
            Suite::JetStream3.fixture("zlib-wasm"),
            Suite::JetStream3.fixture("zlib-wasm"),
        ];
        assert!(!fixture_selection_is_valid(Suite::JetStream3, &duplicate));
    }

    #[test]
    fn v8_qualification_still_requires_the_complete_fixture_set() {
        let all = Suite::V8V7.fixtures();
        assert!(fixture_selection_is_valid(Suite::V8V7, &all));
        assert!(!fixture_selection_is_valid(Suite::V8V7, &all[..1]));
    }
}

#[cfg(test)]
mod jit_wasm_root_only_tests {
    use super::{
        JitMode, Options, Suite, jit_wasm_root_first_error, jit_wasm_root_only_error,
        jit_wasm_selective_error, parse_options_from, quench_engine,
    };

    #[test]
    fn root_only_cli_switch_parses_with_its_required_root_configuration() {
        let options = parse_options_from(
            [
                "--fixtures",
                "HashSet-wasm",
                "--suite",
                "jetstream3",
                "--jit",
                "--jit-wasm-root",
                "19",
                "--jit-wasm-root-only",
            ]
            .map(str::to_string),
        );

        assert_eq!(options.suite, Suite::JetStream3);
        assert_eq!(options.jit_mode, JitMode::Jit);
        assert_eq!(options.jit_wasm_root, Some(19));
        assert!(options.jit_wasm_root_only);
    }

    #[test]
    fn root_first_cli_switch_parses_with_distinct_tiered_mode() {
        let options = parse_options_from(
            [
                "--fixtures",
                "gcc-loops-wasm",
                "--suite",
                "jetstream3",
                "--jit",
                "--jit-wasm-root",
                "19",
                "--jit-wasm-root-first",
            ]
            .map(str::to_string),
        );

        assert_eq!(options.suite, Suite::JetStream3);
        assert_eq!(options.jit_mode, JitMode::Jit);
        assert_eq!(options.jit_wasm_root, Some(19));
        assert!(options.jit_wasm_root_first);
        assert!(!options.jit_wasm_root_only);
    }

    #[test]
    fn root_only_requires_the_jetstream_jit_root_configuration() {
        assert!(
            jit_wasm_root_only_error(Suite::JetStream3, JitMode::Jit, Some(19), true).is_none()
        );
        assert!(jit_wasm_root_only_error(Suite::JetStream3, JitMode::Jit, None, true).is_some());
        assert!(jit_wasm_root_only_error(Suite::V8V7, JitMode::Jit, Some(19), true).is_some());
        assert!(
            jit_wasm_root_only_error(Suite::JetStream3, JitMode::NoJit, Some(19), true).is_some()
        );
        assert!(
            jit_wasm_root_only_error(Suite::JetStream3, JitMode::Jit, Some(19), false).is_none()
        );
    }

    #[test]
    fn root_first_requires_a_root_and_is_mutually_exclusive_with_strict_root_only() {
        assert!(
            jit_wasm_root_first_error(Suite::JetStream3, JitMode::Jit, Some(19), true, false)
                .is_none()
        );
        assert!(
            jit_wasm_root_first_error(Suite::JetStream3, JitMode::Jit, None, true, false).is_some()
        );
        assert!(
            jit_wasm_root_first_error(Suite::V8V7, JitMode::Jit, Some(19), true, false).is_some()
        );
        assert!(
            jit_wasm_root_first_error(Suite::JetStream3, JitMode::NoJit, Some(19), true, false)
                .is_some()
        );
        assert!(
            jit_wasm_root_first_error(Suite::JetStream3, JitMode::Jit, Some(19), true, true)
                .is_some()
        );
    }

    #[test]
    fn selective_and_root_only_modes_cannot_be_combined() {
        assert!(
            jit_wasm_selective_error(Suite::JetStream3, JitMode::Jit, Some(19), true, true)
                .is_some()
        );
        assert!(
            jit_wasm_selective_error(Suite::JetStream3, JitMode::Jit, None, false, true).is_none()
        );
    }

    #[test]
    fn root_only_is_injected_and_recorded_as_experimental_provenance() {
        let executable = std::env::current_exe().expect("test executable path");
        let options = Options {
            suite: Suite::JetStream3,
            jit_mode: JitMode::Jit,
            jit_diagnostics: false,
            jit_js_general_dispatch: false,
            jit_wasm_root: Some(19),
            jit_wasm_root_only: true,
            jit_wasm_root_first: false,
            jit_wasm_selective: false,
            jit_wasm_memcpy_ab: false,
            jit_wasm_memcpy_raw_ab: false,
            jit_wasm_memcpy_early_ab: false,
            jit_wasm_compare_branch_ab: false,
            jit_wasm_affine_pair_ab: false,
            jit_wasm_reference_body_cache_ab: false,
            jit_wasm_memcpy_leaf_ab: false,
            jit_wasm_selective_ab: false,
            fixture: None,
            fixtures: Some(vec!["HashSet-wasm".into()]),
            all: false,
            preflight_only: false,
            node: executable.clone(),
            bun: executable.clone(),
            qjs: executable.clone(),
            quench: executable,
            quench_peer: None,
            rounds: 1,
            fixed_work: false,
            timeout_ms: 1000,
            output: None,
            checkpoint: None,
            resume: None,
        };
        let engine = quench_engine(&options);

        assert_eq!(
            engine
                .env
                .get("QUENCH_WASM_JIT_ROOT_ONLY")
                .map(String::as_str),
            Some("1")
        );
        assert!(engine.jit_proof.contains("single-root compilation"));
        assert!(engine.jit_proof.contains("not qualification mode"));
    }

    #[test]
    fn root_first_is_injected_and_reported_as_tiered_experimental_mode() {
        let executable = std::env::current_exe().expect("test executable path");
        let options = Options {
            suite: Suite::JetStream3,
            jit_mode: JitMode::Jit,
            jit_diagnostics: false,
            jit_js_general_dispatch: false,
            jit_wasm_root: Some(19),
            jit_wasm_root_only: false,
            jit_wasm_root_first: true,
            jit_wasm_selective: false,
            jit_wasm_memcpy_ab: false,
            jit_wasm_memcpy_raw_ab: false,
            jit_wasm_memcpy_early_ab: false,
            jit_wasm_compare_branch_ab: false,
            jit_wasm_affine_pair_ab: false,
            jit_wasm_reference_body_cache_ab: false,
            jit_wasm_memcpy_leaf_ab: false,
            jit_wasm_selective_ab: false,
            fixture: None,
            fixtures: Some(vec!["gcc-loops-wasm".into()]),
            all: false,
            preflight_only: false,
            node: executable.clone(),
            bun: executable.clone(),
            qjs: executable.clone(),
            quench: executable,
            quench_peer: None,
            rounds: 1,
            fixed_work: false,
            timeout_ms: 1000,
            output: None,
            checkpoint: None,
            resume: None,
        };
        let engine = quench_engine(&options);

        assert_eq!(
            engine
                .env
                .get("QUENCH_WASM_JIT_ROOT_FIRST")
                .map(String::as_str),
            Some("1")
        );
        assert!(engine.jit_proof.contains("root-first tiering"));
        assert!(engine.jit_proof.contains("promotes hot fallback callees"));
        assert!(engine.jit_proof.contains("not qualification mode"));
    }
}

#[cfg(test)]
mod jit_wasm_memcpy_ab_tests {
    use super::{
        JitMode, QUENCH_WASM_EMSCRIPTEN_MEMCPY_DIRECT, QUENCH_WASM_EMSCRIPTEN_MEMCPY_EARLY_IMPORT,
        QUENCH_WASM_EMSCRIPTEN_MEMCPY_RAW_I32, Suite, jit_wasm_memcpy_ab_error, parse_options_from,
        quench_memcpy_ab_pair, quench_memcpy_early_ab_pair,
    };

    fn gcc_wasm_ab_options() -> super::Options {
        parse_options_from(
            [
                "gcc-loops-wasm",
                "--suite",
                "jetstream3",
                "--jit",
                "--jit-wasm-memcpy-ab",
            ]
            .map(str::to_string),
        )
    }

    fn gcc_wasm_early_ab_options() -> super::Options {
        parse_options_from(
            [
                "gcc-loops-wasm",
                "--suite",
                "jetstream3",
                "--jit",
                "--jit-wasm-memcpy-early-ab",
                "--runs",
                "3",
            ]
            .map(str::to_string),
        )
    }

    #[test]
    fn diagnostic_mode_requires_default_jit_and_is_pinned_to_gcc_wasm() {
        let options = gcc_wasm_ab_options();
        assert_eq!(options.suite, Suite::JetStream3);
        assert_eq!(options.jit_mode, JitMode::Jit);
        assert!(options.jit_wasm_memcpy_ab);
        assert_eq!(options.fixture.as_deref(), Some("gcc-loops-wasm"));
        assert!(jit_wasm_memcpy_ab_error(&options).is_none());

        let mut other_fixture = options;
        other_fixture.fixture = Some("quicksort-wasm".into());
        assert!(jit_wasm_memcpy_ab_error(&other_fixture).is_some());
    }

    #[test]
    fn quench_arms_share_binary_and_jit_args_but_set_explicit_opposite_switches() {
        let mut options = gcc_wasm_ab_options();
        options.quench = std::env::current_exe().expect("test executable path");
        let [direct_off, direct_on] = quench_memcpy_ab_pair(&options, false);

        assert_eq!(direct_off.argv, vec!["--jit".to_string()]);
        assert_eq!(direct_on.argv, direct_off.argv);
        assert_eq!(direct_off.executable, direct_on.executable);
        assert_eq!(direct_off.executable_sha256, direct_on.executable_sha256);
        assert_eq!(
            direct_off
                .env
                .get(QUENCH_WASM_EMSCRIPTEN_MEMCPY_DIRECT)
                .map(String::as_str),
            Some("0")
        );
        assert_eq!(
            direct_on
                .env
                .get(QUENCH_WASM_EMSCRIPTEN_MEMCPY_DIRECT)
                .map(String::as_str),
            Some("1")
        );
        assert!(direct_off.jit_proof.contains("diagnostic A/B arm"));
        assert!(direct_on.jit_proof.contains("diagnostic A/B arm"));
    }

    #[test]
    fn raw_i32_mode_keeps_direct_copy_enabled_and_varies_only_raw_i32_switch() {
        let mut options = parse_options_from(
            [
                "gcc-loops-wasm",
                "--suite",
                "jetstream3",
                "--jit",
                "--jit-wasm-memcpy-raw-ab",
            ]
            .map(str::to_string),
        );
        options.quench = std::env::current_exe().expect("test executable path");
        assert!(options.jit_wasm_memcpy_raw_ab);
        assert!(jit_wasm_memcpy_ab_error(&options).is_none());
        let [raw_off, raw_on] = quench_memcpy_ab_pair(&options, true);

        assert_eq!(raw_off.name, "quench_memcpy_raw_i32_off");
        assert_eq!(raw_on.name, "quench_memcpy_raw_i32_on");
        assert_eq!(raw_off.argv, vec!["--jit".to_string()]);
        assert_eq!(raw_on.argv, raw_off.argv);
        assert_eq!(raw_off.executable, raw_on.executable);
        assert_eq!(raw_off.executable_sha256, raw_on.executable_sha256);
        for engine in [&raw_off, &raw_on] {
            assert_eq!(
                engine
                    .env
                    .get(QUENCH_WASM_EMSCRIPTEN_MEMCPY_DIRECT)
                    .map(String::as_str),
                Some("1")
            );
        }
        assert_eq!(
            raw_off
                .env
                .get(QUENCH_WASM_EMSCRIPTEN_MEMCPY_RAW_I32)
                .map(String::as_str),
            Some("0")
        );
        assert_eq!(
            raw_on
                .env
                .get(QUENCH_WASM_EMSCRIPTEN_MEMCPY_RAW_I32)
                .map(String::as_str),
            Some("1")
        );
        let mut conflicting = options;
        conflicting.jit_wasm_memcpy_ab = true;
        assert!(jit_wasm_memcpy_ab_error(&conflicting).is_some());
    }

    #[test]
    fn early_import_mode_pins_gcc_and_varies_only_the_early_import_switch() {
        let mut options = gcc_wasm_early_ab_options();
        assert!(options.jit_wasm_memcpy_early_ab);
        assert_eq!(options.rounds, 3);
        assert!(jit_wasm_memcpy_ab_error(&options).is_none());

        let mut too_few_rounds = gcc_wasm_early_ab_options();
        too_few_rounds.rounds = 2;
        assert!(jit_wasm_memcpy_ab_error(&too_few_rounds).is_some());
        let mut no_jit = gcc_wasm_early_ab_options();
        no_jit.jit_mode = JitMode::NoJit;
        assert!(jit_wasm_memcpy_ab_error(&no_jit).is_some());
        let mut wrong_fixture = gcc_wasm_early_ab_options();
        wrong_fixture.fixture = Some("quicksort-wasm".into());
        assert!(jit_wasm_memcpy_ab_error(&wrong_fixture).is_some());
        let mut conflicting = gcc_wasm_early_ab_options();
        conflicting.jit_wasm_memcpy_raw_ab = true;
        assert!(jit_wasm_memcpy_ab_error(&conflicting).is_some());

        options.quench = std::env::current_exe().expect("test executable path");
        let [early_off, early_on] = quench_memcpy_early_ab_pair(&options);
        assert_eq!(early_off.name, "quench_memcpy_early_import_off");
        assert_eq!(early_on.name, "quench_memcpy_early_import_on");
        assert_eq!(early_off.argv, vec!["--jit".to_string()]);
        assert_eq!(early_on.argv, early_off.argv);
        assert_eq!(early_off.executable, early_on.executable);
        assert_eq!(early_off.executable_sha256, early_on.executable_sha256);
        assert_eq!(early_off.env.len(), early_on.env.len());
        for engine in [&early_off, &early_on] {
            assert_eq!(
                engine
                    .env
                    .get(QUENCH_WASM_EMSCRIPTEN_MEMCPY_DIRECT)
                    .map(String::as_str),
                Some("1")
            );
            assert_eq!(
                engine
                    .env
                    .get(QUENCH_WASM_EMSCRIPTEN_MEMCPY_RAW_I32)
                    .map(String::as_str),
                Some("1")
            );
        }
        assert_eq!(
            early_off
                .env
                .get(QUENCH_WASM_EMSCRIPTEN_MEMCPY_EARLY_IMPORT)
                .map(String::as_str),
            Some("0")
        );
        assert_eq!(
            early_on
                .env
                .get(QUENCH_WASM_EMSCRIPTEN_MEMCPY_EARLY_IMPORT)
                .map(String::as_str),
            Some("1")
        );
        let mut off_environment = early_off.env.clone();
        let mut on_environment = early_on.env.clone();
        off_environment.remove(QUENCH_WASM_EMSCRIPTEN_MEMCPY_EARLY_IMPORT);
        on_environment.remove(QUENCH_WASM_EMSCRIPTEN_MEMCPY_EARLY_IMPORT);
        assert_eq!(off_environment, on_environment);
    }
}

#[cfg(test)]
mod jit_wasm_compare_branch_ab_tests {
    use super::{
        JitMode, QUENCH_WASM_JIT_DIRECT_COMPARE_BRANCHES, Suite, jit_wasm_compare_branch_ab_error,
        parse_options_from, quench_compare_branch_ab_pair,
    };

    fn quicksort_ab_options() -> super::Options {
        parse_options_from(
            [
                "quicksort-wasm",
                "--suite",
                "jetstream3",
                "--jit",
                "--jit-wasm-compare-branch-ab",
                "--runs",
                "3",
            ]
            .map(str::to_string),
        )
    }

    #[test]
    fn diagnostic_mode_is_pinned_to_quicksort_and_allows_three_rounds() {
        let options = quicksort_ab_options();
        assert_eq!(options.suite, Suite::JetStream3);
        assert_eq!(options.jit_mode, JitMode::Jit);
        assert!(options.jit_wasm_compare_branch_ab);
        assert_eq!(options.rounds, 3);
        assert_eq!(options.fixture.as_deref(), Some("quicksort-wasm"));
        assert!(jit_wasm_compare_branch_ab_error(&options).is_none());

        let mut wrong_fixture = options;
        wrong_fixture.fixture = Some("gcc-loops-wasm".into());
        assert!(jit_wasm_compare_branch_ab_error(&wrong_fixture).is_some());

        let mut too_few_rounds = quicksort_ab_options();
        too_few_rounds.rounds = 2;
        assert!(jit_wasm_compare_branch_ab_error(&too_few_rounds).is_some());
    }

    #[test]
    fn quench_arms_share_binary_and_jit_args_and_explicitly_toggle_branch_switch() {
        let mut options = quicksort_ab_options();
        options.quench = std::env::current_exe().expect("test executable path");
        let [branch_off, branch_on] = quench_compare_branch_ab_pair(&options);

        assert_eq!(branch_off.name, "quench_direct_compare_branches_off");
        assert_eq!(branch_on.name, "quench_direct_compare_branches_on");
        assert_eq!(branch_off.argv, vec!["--jit".to_string()]);
        assert_eq!(branch_on.argv, branch_off.argv);
        assert_eq!(branch_off.executable, branch_on.executable);
        assert_eq!(branch_off.executable_sha256, branch_on.executable_sha256);
        assert_eq!(
            branch_off
                .env
                .get(QUENCH_WASM_JIT_DIRECT_COMPARE_BRANCHES)
                .map(String::as_str),
            Some("0")
        );
        assert_eq!(
            branch_on
                .env
                .get(QUENCH_WASM_JIT_DIRECT_COMPARE_BRANCHES)
                .map(String::as_str),
            Some("1")
        );
    }
}

#[cfg(test)]
mod jit_wasm_affine_pair_ab_tests {
    use super::{
        JitMode, QUENCH_WASM_JIT_AFFINE_PAIR, Suite, jit_wasm_affine_pair_ab_error,
        parse_options_from, quench_affine_pair_ab_pair,
    };

    fn gcc_affine_pair_options() -> super::Options {
        parse_options_from(
            [
                "gcc-loops-wasm",
                "--suite",
                "jetstream3",
                "--jit",
                "--jit-wasm-affine-pair-ab",
                "--runs",
                "3",
            ]
            .map(str::to_string),
        )
    }

    #[test]
    fn diagnostic_mode_is_pinned_to_gcc_and_allows_three_rounds() {
        let options = gcc_affine_pair_options();
        assert_eq!(options.suite, Suite::JetStream3);
        assert_eq!(options.jit_mode, JitMode::Jit);
        assert!(options.jit_wasm_affine_pair_ab);
        assert_eq!(options.rounds, 3);
        assert_eq!(options.fixture.as_deref(), Some("gcc-loops-wasm"));
        assert!(jit_wasm_affine_pair_ab_error(&options).is_none());

        let mut wrong_fixture = options;
        wrong_fixture.fixture = Some("quicksort-wasm".into());
        assert!(jit_wasm_affine_pair_ab_error(&wrong_fixture).is_some());

        let mut too_few_rounds = gcc_affine_pair_options();
        too_few_rounds.rounds = 2;
        assert!(jit_wasm_affine_pair_ab_error(&too_few_rounds).is_some());
    }

    #[test]
    fn quench_arms_share_the_binary_and_differ_only_by_the_affine_switch() {
        let mut options = gcc_affine_pair_options();
        options.quench = std::env::current_exe().expect("test executable path");
        let [affine_off, affine_on] = quench_affine_pair_ab_pair(&options);

        assert_eq!(affine_off.name, "quench_affine_pair_off");
        assert_eq!(affine_on.name, "quench_affine_pair_on");
        assert_eq!(affine_off.argv, vec!["--jit".to_string()]);
        assert_eq!(affine_on.argv, affine_off.argv);
        assert_eq!(affine_off.executable, affine_on.executable);
        assert_eq!(affine_off.executable_sha256, affine_on.executable_sha256);
        let mut off_environment = affine_off.env.clone();
        let mut on_environment = affine_on.env.clone();
        assert_eq!(
            off_environment
                .remove(QUENCH_WASM_JIT_AFFINE_PAIR)
                .as_deref(),
            Some("0")
        );
        assert_eq!(
            on_environment
                .remove(QUENCH_WASM_JIT_AFFINE_PAIR)
                .as_deref(),
            Some("1")
        );
        assert_eq!(off_environment, on_environment);
    }
}

#[cfg(test)]
mod jit_wasm_reference_body_cache_ab_tests {
    use super::{
        JitMode, QUENCH_WASM_JIT_REFERENCE_BODY_CACHE, Suite,
        jit_wasm_reference_body_cache_ab_error, parse_options_from,
        quench_reference_body_cache_ab_pair,
    };

    fn tsf_reference_cache_options() -> super::Options {
        parse_options_from(
            [
                "tsf-wasm",
                "--suite",
                "jetstream3",
                "--jit",
                "--jit-wasm-reference-body-cache-ab",
                "--runs",
                "3",
            ]
            .map(str::to_string),
        )
    }

    #[test]
    fn diagnostic_mode_is_pinned_to_tsf_and_allows_three_rounds() {
        let options = tsf_reference_cache_options();
        assert_eq!(options.suite, Suite::JetStream3);
        assert_eq!(options.jit_mode, JitMode::Jit);
        assert!(options.jit_wasm_reference_body_cache_ab);
        assert_eq!(options.rounds, 3);
        assert_eq!(options.fixture.as_deref(), Some("tsf-wasm"));
        assert!(jit_wasm_reference_body_cache_ab_error(&options).is_none());

        let mut wrong_fixture = options;
        wrong_fixture.fixture = Some("gcc-loops-wasm".into());
        assert!(jit_wasm_reference_body_cache_ab_error(&wrong_fixture).is_some());

        let mut too_few_rounds = tsf_reference_cache_options();
        too_few_rounds.rounds = 2;
        assert!(jit_wasm_reference_body_cache_ab_error(&too_few_rounds).is_some());
    }

    #[test]
    fn quench_arms_share_the_binary_and_differ_only_by_the_cache_switch() {
        let mut options = tsf_reference_cache_options();
        options.quench = std::env::current_exe().expect("test executable path");
        let [cache_off, cache_on] = quench_reference_body_cache_ab_pair(&options);

        assert_eq!(cache_off.name, "quench_reference_body_cache_off");
        assert_eq!(cache_on.name, "quench_reference_body_cache_on");
        assert_eq!(cache_off.argv, vec!["--jit".to_string()]);
        assert_eq!(cache_on.argv, cache_off.argv);
        assert_eq!(cache_off.executable, cache_on.executable);
        assert_eq!(cache_off.executable_sha256, cache_on.executable_sha256);
        let mut off_environment = cache_off.env.clone();
        let mut on_environment = cache_on.env.clone();
        assert_eq!(
            off_environment
                .remove(QUENCH_WASM_JIT_REFERENCE_BODY_CACHE)
                .as_deref(),
            Some("0")
        );
        assert_eq!(
            on_environment
                .remove(QUENCH_WASM_JIT_REFERENCE_BODY_CACHE)
                .as_deref(),
            Some("1")
        );
        assert_eq!(off_environment, on_environment);
    }
}

#[cfg(test)]
mod jit_wasm_memcpy_leaf_ab_tests {
    use super::{
        JitMode, QUENCH_WASM_JIT_EMSCRIPTEN_MEMCPY_LEAF, Suite, jit_wasm_memcpy_leaf_ab_error,
        parse_options_from, quench_memcpy_leaf_ab_pair,
    };

    fn gcc_memcpy_leaf_options() -> super::Options {
        parse_options_from(
            [
                "gcc-loops-wasm",
                "--suite",
                "jetstream3",
                "--jit",
                "--jit-wasm-memcpy-leaf-ab",
                "--runs",
                "3",
            ]
            .map(str::to_string),
        )
    }

    #[test]
    fn diagnostic_mode_is_pinned_to_gcc_and_allows_three_rounds() {
        let options = gcc_memcpy_leaf_options();
        assert_eq!(options.suite, Suite::JetStream3);
        assert_eq!(options.jit_mode, JitMode::Jit);
        assert!(options.jit_wasm_memcpy_leaf_ab);
        assert_eq!(options.rounds, 3);
        assert_eq!(options.fixture.as_deref(), Some("gcc-loops-wasm"));
        assert!(jit_wasm_memcpy_leaf_ab_error(&options).is_none());

        let mut wrong_fixture = options;
        wrong_fixture.fixture = Some("quicksort-wasm".into());
        assert!(jit_wasm_memcpy_leaf_ab_error(&wrong_fixture).is_some());

        let mut too_few_rounds = gcc_memcpy_leaf_options();
        too_few_rounds.rounds = 2;
        assert!(jit_wasm_memcpy_leaf_ab_error(&too_few_rounds).is_some());

        let mut no_jit = gcc_memcpy_leaf_options();
        no_jit.jit_mode = JitMode::NoJit;
        assert!(jit_wasm_memcpy_leaf_ab_error(&no_jit).is_some());

        let mut conflicting_mode = gcc_memcpy_leaf_options();
        conflicting_mode.jit_wasm_memcpy_early_ab = true;
        assert!(jit_wasm_memcpy_leaf_ab_error(&conflicting_mode).is_some());
    }

    #[test]
    fn quench_arms_share_binary_and_differ_only_by_the_leaf_switch() {
        let mut options = gcc_memcpy_leaf_options();
        options.quench = std::env::current_exe().expect("test executable path");
        let [leaf_off, leaf_on] = quench_memcpy_leaf_ab_pair(&options);

        assert_eq!(leaf_off.name, "quench_memcpy_native_leaf_off");
        assert_eq!(leaf_on.name, "quench_memcpy_native_leaf_on");
        assert_eq!(leaf_off.argv, vec!["--jit".to_string()]);
        assert_eq!(leaf_on.argv, leaf_off.argv);
        assert_eq!(leaf_off.executable, leaf_on.executable);
        assert_eq!(leaf_off.executable_sha256, leaf_on.executable_sha256);
        let mut off_environment = leaf_off.env.clone();
        let mut on_environment = leaf_on.env.clone();
        assert_eq!(
            off_environment
                .remove(QUENCH_WASM_JIT_EMSCRIPTEN_MEMCPY_LEAF)
                .as_deref(),
            Some("0")
        );
        assert_eq!(
            on_environment
                .remove(QUENCH_WASM_JIT_EMSCRIPTEN_MEMCPY_LEAF)
                .as_deref(),
            Some("1")
        );
        assert_eq!(off_environment, on_environment);
        assert!(leaf_off.jit_proof.contains("native memcpy leaf A/B arm"));
        assert!(leaf_on.jit_proof.contains("native memcpy leaf A/B arm"));
        assert!(leaf_off.jit_proof.contains("not for qualification"));
    }
}

#[cfg(test)]
mod jit_js_general_dispatch_tests {
    use super::{
        jit_js_general_dispatch_error, parse_options_from, quench_engine, JitMode, Suite,
        QUENCH_JS_JIT_GENERAL_DISPATCH,
    };

    fn experimental_options() -> super::Options {
        parse_options_from(
            [
                "--all",
                "--suite",
                "jetstream3",
                "--jit",
                "--jit-js-general-dispatch",
            ]
            .map(str::to_string),
        )
    }

    #[test]
    fn flag_is_limited_to_jetstream_jit_mode() {
        let options = experimental_options();
        assert_eq!(options.suite, Suite::JetStream3);
        assert_eq!(options.jit_mode, JitMode::Jit);
        assert!(options.jit_js_general_dispatch);
        assert!(jit_js_general_dispatch_error(&options).is_none());

        let mut no_jit = options;
        no_jit.jit_mode = JitMode::NoJit;
        assert!(jit_js_general_dispatch_error(&no_jit).is_some());

        let mut wrong_suite = experimental_options();
        wrong_suite.suite = Suite::V8V7;
        assert!(jit_js_general_dispatch_error(&wrong_suite).is_some());
    }

    #[test]
    fn engine_receives_general_dispatch_environment_and_provenance() {
        let mut options = experimental_options();
        options.quench = std::env::current_exe().expect("test executable path");
        let engine = quench_engine(&options);
        assert_eq!(engine.argv, vec!["--jit".to_string()]);
        assert_eq!(
            engine
                .env
                .get(QUENCH_JS_JIT_GENERAL_DISPATCH)
                .map(String::as_str),
            Some("1")
        );
        assert!(engine
            .jit_proof
            .contains("experimental priced JavaScript GeneralDispatch"));
        assert!(engine.jit_proof.contains("not qualification mode"));
    }
}

#[cfg(test)]
mod jit_wasm_selective_ab_tests {
    use super::{
        JitMode, QUENCH_WASM_JIT_SELECTIVE, Suite, jit_wasm_selective_ab_error, parse_options_from,
        quench_selective_ab_pair,
    };

    fn gcc_selective_options() -> super::Options {
        parse_options_from(
            [
                "gcc-loops-wasm",
                "--suite",
                "jetstream3",
                "--jit",
                "--jit-wasm-selective-ab",
                "--runs",
                "3",
            ]
            .map(str::to_string),
        )
    }

    #[test]
    fn diagnostic_mode_is_pinned_to_gcc_and_allows_three_rounds() {
        let options = gcc_selective_options();
        assert_eq!(options.suite, Suite::JetStream3);
        assert_eq!(options.jit_mode, JitMode::Jit);
        assert!(options.jit_wasm_selective_ab);
        assert_eq!(options.rounds, 3);
        assert_eq!(options.fixture.as_deref(), Some("gcc-loops-wasm"));
        assert!(jit_wasm_selective_ab_error(&options).is_none());

        let mut wrong_fixture = options;
        wrong_fixture.fixture = Some("quicksort-wasm".into());
        assert!(jit_wasm_selective_ab_error(&wrong_fixture).is_some());

        let mut too_few_rounds = gcc_selective_options();
        too_few_rounds.rounds = 2;
        assert!(jit_wasm_selective_ab_error(&too_few_rounds).is_some());

        let mut no_jit = gcc_selective_options();
        no_jit.jit_mode = JitMode::NoJit;
        assert!(jit_wasm_selective_ab_error(&no_jit).is_some());

        let mut conflicting_mode = gcc_selective_options();
        conflicting_mode.jit_wasm_memcpy_leaf_ab = true;
        assert!(jit_wasm_selective_ab_error(&conflicting_mode).is_some());
    }

    #[test]
    fn quench_arms_share_binary_and_differ_only_by_selective_handoff_switch() {
        let mut options = gcc_selective_options();
        options.quench = std::env::current_exe().expect("test executable path");
        let [selective_off, selective_on] = quench_selective_ab_pair(&options);

        assert_eq!(selective_off.name, "quench_selective_handoff_off");
        assert_eq!(selective_on.name, "quench_selective_handoff_on");
        assert_eq!(selective_off.argv, vec!["--jit".to_string()]);
        assert_eq!(selective_on.argv, selective_off.argv);
        assert_eq!(selective_off.executable, selective_on.executable);
        assert_eq!(
            selective_off.executable_sha256,
            selective_on.executable_sha256
        );
        let mut off_environment = selective_off.env.clone();
        let mut on_environment = selective_on.env.clone();
        assert_eq!(
            off_environment.remove(QUENCH_WASM_JIT_SELECTIVE).as_deref(),
            Some("0")
        );
        assert_eq!(
            on_environment.remove(QUENCH_WASM_JIT_SELECTIVE).as_deref(),
            Some("1")
        );
        assert_eq!(off_environment, on_environment);
        assert!(
            selective_off
                .jit_proof
                .contains("selective CallKnown handoff A/B arm")
        );
        assert!(selective_on.jit_proof.contains("not for qualification"));
    }
}

#[cfg(test)]
mod fixed_work_tests {
    use super::{
        FIXED_WORK_PLANS, FIXTURES, FixedWorkPlan, HostSnapshot, LoadAverage, Sample, contention,
        fixed_work_marker, fixed_work_runner, parse_cpu_consumers, parse_load_average,
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
            .map(|pid| format!("{pid} {pid}.0 R process-{pid}\n"))
            .collect::<String>();
        let input = format!("{input}99 999.0 Z rustc-zombie\n");
        let consumers = parse_cpu_consumers(&input);
        assert_eq!(consumers.len(), 5);
        assert_eq!(consumers[0].pid, 7);
        assert_eq!(consumers[0].cpu_percent, 7.0);
        assert_eq!(consumers[0].command, "process-7");
        assert!(consumers.iter().all(|consumer| consumer.pid != 99));
    }
}

#[cfg(all(test, unix, not(target_os = "macos")))]
mod tests {
    use super::{
        CorpusRecord, FixtureRecord, HostRecord, SuiteRecord, artifact, fixture_artifact, now_ns,
        peak_rss_bytes, read_checkpoint, resume_metadata_matches, validate_resume, wait_child,
        write_checkpoint,
    };
    use std::process::Command;
    use std::{
        collections::BTreeMap,
        fs,
        path::{Path, PathBuf},
    };

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
            schema: super::SUITE_REPORT_SCHEMA,
            suite: super::Suite::JetStream3,
            jit_mode: super::JitMode::Jit,
            jit_diagnostics: false,
            jit_js_general_dispatch: false,
            jit_wasm_root: Some(19),
            jit_wasm_root_only: true,
            jit_wasm_root_first: false,
            jit_wasm_selective: false,
            jit_wasm_memcpy_ab: false,
            jit_wasm_memcpy_raw_ab: false,
            jit_wasm_memcpy_early_ab: false,
            jit_wasm_compare_branch_ab: false,
            jit_wasm_affine_pair_ab: false,
            jit_wasm_reference_body_cache_ab: false,
            jit_wasm_memcpy_leaf_ab: false,
            jit_wasm_selective_ab: false,
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
            selected_fixtures: vec![fixture_path.display().to_string()],
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
                unsupported_engines: BTreeMap::new(),
                rounds: Vec::new(),
            },
        );

        write_checkpoint(&report_path, &saved);
        let loaded = read_checkpoint(&report_path);
        validate_resume(&loaded, &expected, &[PathBuf::from(&fixture_path)]);
        assert_eq!(loaded.fixtures.len(), 1);
        assert!(loaded.jit_wasm_root_only);
        let mut different_mode = expected.clone();
        different_mode.jit_wasm_root_only = false;
        assert!(!resume_metadata_matches(&loaded, &different_mode));

        let root_first_path = directory.join("root-first-report.json");
        let mut root_first = expected.clone();
        root_first.jit_wasm_root_only = false;
        root_first.jit_wasm_root_first = true;
        write_checkpoint(&root_first_path, &root_first);
        let root_first_loaded = read_checkpoint(&root_first_path);
        assert!(root_first_loaded.jit_wasm_root_first);
        assert!(!root_first_loaded.jit_wasm_root_only);
        assert!(resume_metadata_matches(&root_first_loaded, &root_first));
        assert!(!resume_metadata_matches(&loaded, &root_first));

        let memcpy_ab_path = directory.join("memcpy-ab-report.json");
        let mut memcpy_ab = expected.clone();
        memcpy_ab.jit_wasm_root_only = false;
        memcpy_ab.jit_wasm_memcpy_ab = true;
        write_checkpoint(&memcpy_ab_path, &memcpy_ab);
        let memcpy_ab_loaded = read_checkpoint(&memcpy_ab_path);
        assert!(memcpy_ab_loaded.jit_wasm_memcpy_ab);
        assert!(resume_metadata_matches(&memcpy_ab_loaded, &memcpy_ab));
        assert!(!resume_metadata_matches(&loaded, &memcpy_ab));

        let raw_ab_path = directory.join("memcpy-raw-ab-report.json");
        let mut raw_ab = expected.clone();
        raw_ab.jit_wasm_root_only = false;
        raw_ab.jit_wasm_memcpy_raw_ab = true;
        write_checkpoint(&raw_ab_path, &raw_ab);
        let raw_ab_loaded = read_checkpoint(&raw_ab_path);
        assert!(raw_ab_loaded.jit_wasm_memcpy_raw_ab);
        assert!(resume_metadata_matches(&raw_ab_loaded, &raw_ab));
        assert!(!resume_metadata_matches(&memcpy_ab_loaded, &raw_ab));

        let early_ab_path = directory.join("memcpy-early-ab-report.json");
        let mut early_ab = expected.clone();
        early_ab.jit_wasm_root_only = false;
        early_ab.jit_wasm_memcpy_early_ab = true;
        write_checkpoint(&early_ab_path, &early_ab);
        let early_ab_loaded = read_checkpoint(&early_ab_path);
        assert!(early_ab_loaded.jit_wasm_memcpy_early_ab);
        assert!(resume_metadata_matches(&early_ab_loaded, &early_ab));
        assert!(!resume_metadata_matches(&raw_ab_loaded, &early_ab));

        let compare_branch_ab_path = directory.join("compare-branch-ab-report.json");
        let mut compare_branch_ab = expected.clone();
        compare_branch_ab.jit_wasm_root_only = false;
        compare_branch_ab.jit_wasm_compare_branch_ab = true;
        write_checkpoint(&compare_branch_ab_path, &compare_branch_ab);
        let compare_branch_ab_loaded = read_checkpoint(&compare_branch_ab_path);
        assert!(compare_branch_ab_loaded.jit_wasm_compare_branch_ab);
        assert!(resume_metadata_matches(
            &compare_branch_ab_loaded,
            &compare_branch_ab
        ));
        assert!(!resume_metadata_matches(&raw_ab_loaded, &compare_branch_ab));

        let memcpy_leaf_ab_path = directory.join("memcpy-leaf-ab-report.json");
        let mut memcpy_leaf_ab = expected.clone();
        memcpy_leaf_ab.jit_wasm_root = None;
        memcpy_leaf_ab.jit_wasm_root_only = false;
        memcpy_leaf_ab.jit_wasm_memcpy_leaf_ab = true;
        write_checkpoint(&memcpy_leaf_ab_path, &memcpy_leaf_ab);
        let memcpy_leaf_ab_loaded = read_checkpoint(&memcpy_leaf_ab_path);
        assert!(memcpy_leaf_ab_loaded.jit_wasm_memcpy_leaf_ab);
        assert!(resume_metadata_matches(
            &memcpy_leaf_ab_loaded,
            &memcpy_leaf_ab
        ));
        assert!(!resume_metadata_matches(&loaded, &memcpy_leaf_ab));
        fs::remove_dir_all(directory).expect("remove checkpoint test directory");
    }

    #[test]
    fn directory_artifacts_hash_sorted_relative_paths_and_contents() {
        let directory = std::env::temp_dir().join(format!(
            "quench-bench-directory-hash-{}-{}",
            std::process::id(),
            now_ns()
        ));
        fs::create_dir_all(directory.join("nested")).expect("create directory fixture");
        fs::write(directory.join("z.js"), "last").expect("write first file");
        fs::write(directory.join("nested/a.wasm"), "first").expect("write nested file");

        let first = super::sha256(&directory).expect("hash directory fixture");
        assert_eq!(first.len(), 64);
        assert_eq!(first, super::sha256(&directory).expect("repeat hash"));
        fs::write(directory.join("nested/a.wasm"), "changed").expect("change nested file");
        assert_ne!(
            first,
            super::sha256(&directory).expect("hash changed fixture")
        );

        fs::remove_dir_all(directory).expect("remove directory fixture");
    }

    #[test]
    fn synthetic_wasm_fixture_artifact_hashes_its_module_directory() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("workspace root");
        let fixture = workspace.join(super::jetstream::fixture_path("HashSet-wasm"));
        let source = fixture_artifact(super::Suite::JetStream3, &fixture);
        assert_eq!(source.path, fixture.display().to_string());
        assert!(source.sha256.as_deref().is_some_and(super::valid_sha256));
    }
}
