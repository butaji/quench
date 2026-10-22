use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use crate::{now_ns, sha256};

const MIN_QUALIFYING_ROUNDS: usize = 11;
const MIN_DIAGNOSTIC_SCREEN_ROUNDS: usize = 3;
const BOOTSTRAP_REPLICATES: usize = 100_000;
const BOOTSTRAP_INTERVAL_LEVEL: f64 = 0.95;
const BOOTSTRAP_BASE_SEED: u64 = 0x51A7_BEEF_CAFE_2C31;
const QUENCH: &str = "quench";
const MEMCPY_DIRECT_OFF: &str = "quench_memcpy_direct_off";
const MEMCPY_DIRECT_ON: &str = "quench_memcpy_direct_on";
const MEMCPY_DIRECT_ENV: &str = "QUENCH_WASM_EMSCRIPTEN_MEMCPY_DIRECT";
const MEMCPY_RAW_OFF: &str = "quench_memcpy_raw_i32_off";
const MEMCPY_RAW_ON: &str = "quench_memcpy_raw_i32_on";
const MEMCPY_RAW_ENV: &str = "QUENCH_WASM_EMSCRIPTEN_MEMCPY_RAW_I32";
const MEMCPY_EARLY_OFF: &str = "quench_memcpy_early_import_off";
const MEMCPY_EARLY_ON: &str = "quench_memcpy_early_import_on";
const MEMCPY_EARLY_ENV: &str = "QUENCH_WASM_EMSCRIPTEN_MEMCPY_EARLY_IMPORT";
const DIRECT_COMPARE_BRANCHES_OFF: &str = "quench_direct_compare_branches_off";
const DIRECT_COMPARE_BRANCHES_ON: &str = "quench_direct_compare_branches_on";
const DIRECT_COMPARE_BRANCHES_ENV: &str = "QUENCH_WASM_JIT_DIRECT_COMPARE_BRANCHES";
const AFFINE_PAIR_OFF: &str = "quench_affine_pair_off";
const AFFINE_PAIR_ON: &str = "quench_affine_pair_on";
const AFFINE_PAIR_ENV: &str = "QUENCH_WASM_JIT_AFFINE_PAIR";
const REFERENCE_BODY_CACHE_OFF: &str = "quench_reference_body_cache_off";
const REFERENCE_BODY_CACHE_ON: &str = "quench_reference_body_cache_on";
const REFERENCE_BODY_CACHE_ENV: &str = "QUENCH_WASM_JIT_REFERENCE_BODY_CACHE";
const MEMCPY_LEAF_OFF: &str = "quench_memcpy_native_leaf_off";
const MEMCPY_LEAF_ON: &str = "quench_memcpy_native_leaf_on";
const MEMCPY_LEAF_ENV: &str = "QUENCH_WASM_JIT_EMSCRIPTEN_MEMCPY_LEAF";
const SELECTIVE_HANDOFF_OFF: &str = "quench_selective_handoff_off";
const SELECTIVE_HANDOFF_ON: &str = "quench_selective_handoff_on";
const SELECTIVE_HANDOFF_ENV: &str = "QUENCH_WASM_JIT_SELECTIVE";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DiagnosticAbMode {
    DirectMemcpy,
    RawI32Memcpy,
    EarlyImportMemcpy,
    DirectCompareBranches,
    AffinePair,
    ReferenceBodyCache,
    NativeMemcpyLeaf,
    SelectiveHandoff,
}

#[derive(Clone, Copy)]
struct DiagnosticAbConfig {
    mode: DiagnosticAbMode,
    report_kind: &'static str,
    off_engine: &'static str,
    on_engine: &'static str,
    switch_env: &'static str,
    off_value: &'static str,
    on_value: &'static str,
    fixed_env: Option<(&'static str, &'static str)>,
    fixed_env2: Option<(&'static str, &'static str)>,
    fixed_env3: Option<(&'static str, &'static str)>,
    fixture_name: &'static str,
    minimum_rounds: usize,
}

static MEMCPY_DIRECT_CONFIG: DiagnosticAbConfig = DiagnosticAbConfig {
    mode: DiagnosticAbMode::DirectMemcpy,
    report_kind: "jit_wasm_memcpy_ab_diagnostic",
    off_engine: MEMCPY_DIRECT_OFF,
    on_engine: MEMCPY_DIRECT_ON,
    switch_env: MEMCPY_DIRECT_ENV,
    off_value: "0",
    on_value: "1",
    fixed_env: None,
    fixed_env2: None,
    fixed_env3: None,
    fixture_name: "gcc-loops-wasm",
    minimum_rounds: MIN_QUALIFYING_ROUNDS,
};

static MEMCPY_RAW_CONFIG: DiagnosticAbConfig = DiagnosticAbConfig {
    mode: DiagnosticAbMode::RawI32Memcpy,
    report_kind: "jit_wasm_memcpy_raw_ab_diagnostic",
    off_engine: MEMCPY_RAW_OFF,
    on_engine: MEMCPY_RAW_ON,
    switch_env: MEMCPY_RAW_ENV,
    off_value: "0",
    on_value: "1",
    fixed_env: Some((MEMCPY_DIRECT_ENV, "1")),
    fixed_env2: None,
    fixed_env3: None,
    fixture_name: "gcc-loops-wasm",
    minimum_rounds: MIN_QUALIFYING_ROUNDS,
};

static MEMCPY_EARLY_CONFIG: DiagnosticAbConfig = DiagnosticAbConfig {
    mode: DiagnosticAbMode::EarlyImportMemcpy,
    report_kind: "jit_wasm_memcpy_early_ab_diagnostic",
    off_engine: MEMCPY_EARLY_OFF,
    on_engine: MEMCPY_EARLY_ON,
    switch_env: MEMCPY_EARLY_ENV,
    off_value: "0",
    on_value: "1",
    fixed_env: Some((MEMCPY_DIRECT_ENV, "1")),
    fixed_env2: Some((MEMCPY_RAW_ENV, "1")),
    fixed_env3: None,
    fixture_name: "gcc-loops-wasm",
    minimum_rounds: MIN_DIAGNOSTIC_SCREEN_ROUNDS,
};

static DIRECT_COMPARE_BRANCHES_CONFIG: DiagnosticAbConfig = DiagnosticAbConfig {
    mode: DiagnosticAbMode::DirectCompareBranches,
    report_kind: "jit_wasm_compare_branch_ab_diagnostic",
    off_engine: DIRECT_COMPARE_BRANCHES_OFF,
    on_engine: DIRECT_COMPARE_BRANCHES_ON,
    switch_env: DIRECT_COMPARE_BRANCHES_ENV,
    off_value: "0",
    on_value: "1",
    fixed_env: None,
    fixed_env2: None,
    fixed_env3: None,
    fixture_name: "quicksort-wasm",
    minimum_rounds: MIN_DIAGNOSTIC_SCREEN_ROUNDS,
};

static AFFINE_PAIR_CONFIG: DiagnosticAbConfig = DiagnosticAbConfig {
    mode: DiagnosticAbMode::AffinePair,
    report_kind: "jit_wasm_affine_pair_ab_diagnostic",
    off_engine: AFFINE_PAIR_OFF,
    on_engine: AFFINE_PAIR_ON,
    switch_env: AFFINE_PAIR_ENV,
    off_value: "0",
    on_value: "1",
    fixed_env: None,
    fixed_env2: None,
    fixed_env3: None,
    fixture_name: "gcc-loops-wasm",
    minimum_rounds: MIN_DIAGNOSTIC_SCREEN_ROUNDS,
};

static REFERENCE_BODY_CACHE_CONFIG: DiagnosticAbConfig = DiagnosticAbConfig {
    mode: DiagnosticAbMode::ReferenceBodyCache,
    report_kind: "jit_wasm_reference_body_cache_ab_diagnostic",
    off_engine: REFERENCE_BODY_CACHE_OFF,
    on_engine: REFERENCE_BODY_CACHE_ON,
    switch_env: REFERENCE_BODY_CACHE_ENV,
    off_value: "0",
    on_value: "1",
    fixed_env: None,
    fixed_env2: None,
    fixed_env3: None,
    fixture_name: "tsf-wasm",
    minimum_rounds: MIN_DIAGNOSTIC_SCREEN_ROUNDS,
};

static MEMCPY_LEAF_CONFIG: DiagnosticAbConfig = DiagnosticAbConfig {
    mode: DiagnosticAbMode::NativeMemcpyLeaf,
    report_kind: "jit_wasm_memcpy_native_leaf_ab_diagnostic",
    off_engine: MEMCPY_LEAF_OFF,
    on_engine: MEMCPY_LEAF_ON,
    switch_env: MEMCPY_LEAF_ENV,
    off_value: "0",
    on_value: "1",
    fixed_env: Some((MEMCPY_DIRECT_ENV, "1")),
    fixed_env2: Some((MEMCPY_RAW_ENV, "1")),
    fixed_env3: Some((MEMCPY_EARLY_ENV, "1")),
    fixture_name: "gcc-loops-wasm",
    minimum_rounds: MIN_DIAGNOSTIC_SCREEN_ROUNDS,
};

static SELECTIVE_HANDOFF_CONFIG: DiagnosticAbConfig = DiagnosticAbConfig {
    mode: DiagnosticAbMode::SelectiveHandoff,
    report_kind: "jit_wasm_selective_handoff_ab_diagnostic",
    off_engine: SELECTIVE_HANDOFF_OFF,
    on_engine: SELECTIVE_HANDOFF_ON,
    switch_env: SELECTIVE_HANDOFF_ENV,
    off_value: "0",
    on_value: "1",
    fixed_env: None,
    fixed_env2: None,
    fixed_env3: None,
    fixture_name: "gcc-loops-wasm",
    minimum_rounds: MIN_DIAGNOSTIC_SCREEN_ROUNDS,
};

fn diagnostic_ab_engines(config: DiagnosticAbConfig) -> [&'static str; 4] {
    [config.off_engine, config.on_engine, "bun", "node"]
}

fn selected_diagnostic_ab_config(
    direct_copy: bool,
    raw_i32: bool,
    compare_branches: bool,
    early_import: bool,
    affine_pair: bool,
    reference_body_cache: bool,
    memcpy_leaf: bool,
    selective_handoff: bool,
) -> Result<DiagnosticAbConfig, String> {
    if [
        direct_copy,
        raw_i32,
        compare_branches,
        early_import,
        affine_pair,
        reference_body_cache,
        memcpy_leaf,
        selective_handoff,
    ]
    .into_iter()
    .filter(|selected| *selected)
    .count()
        != 1
    {
        return Err(
            if direct_copy
                || raw_i32
                || compare_branches
                || early_import
                || affine_pair
                || reference_body_cache
                || memcpy_leaf
                || selective_handoff
            {
                "Wasm diagnostic A/B modes are mutually exclusive".into()
            } else {
                "no Wasm diagnostic A/B mode was selected".into()
            },
        );
    }
    Ok(if direct_copy {
        MEMCPY_DIRECT_CONFIG
    } else if raw_i32 {
        MEMCPY_RAW_CONFIG
    } else if compare_branches {
        DIRECT_COMPARE_BRANCHES_CONFIG
    } else if early_import {
        MEMCPY_EARLY_CONFIG
    } else if affine_pair {
        AFFINE_PAIR_CONFIG
    } else if reference_body_cache {
        REFERENCE_BODY_CACHE_CONFIG
    } else if memcpy_leaf {
        MEMCPY_LEAF_CONFIG
    } else {
        SELECTIVE_HANDOFF_CONFIG
    })
}

#[derive(Deserialize)]
struct SuiteInput {
    schema: u32,
    #[serde(default)]
    suite: crate::Suite,
    #[serde(default)]
    jit_mode: crate::JitMode,
    #[serde(default)]
    jit_diagnostics: bool,
    #[serde(default)]
    jit_wasm_root: Option<u32>,
    #[serde(default)]
    jit_wasm_root_only: bool,
    #[serde(default)]
    jit_wasm_root_first: bool,
    #[serde(default)]
    jit_wasm_selective: bool,
    #[serde(default)]
    jit_wasm_memcpy_ab: bool,
    #[serde(default)]
    jit_wasm_memcpy_raw_ab: bool,
    #[serde(default)]
    jit_wasm_memcpy_early_ab: bool,
    #[serde(default)]
    jit_wasm_compare_branch_ab: bool,
    #[serde(default)]
    jit_wasm_affine_pair_ab: bool,
    #[serde(default)]
    jit_wasm_reference_body_cache_ab: bool,
    #[serde(default)]
    jit_wasm_memcpy_leaf_ab: bool,
    #[serde(default)]
    jit_wasm_selective_ab: bool,
    rounds_requested: usize,
    timeout_ms: u64,
    source_revision: String,
    source_dirty: bool,
    host: HostInput,
    corpus: CorpusInput,
    engines: Vec<EngineInput>,
    suite_inputs: Vec<ArtifactInput>,
    fixtures: BTreeMap<String, FixtureInput>,
    complete: bool,
    qualification_ready: bool,
}

#[derive(Clone, Deserialize, Serialize)]
struct HostInput {
    uname: String,
    rustc: String,
    model: Option<String>,
    memory_bytes: Option<u64>,
    memory_limit_bytes: Option<u64>,
    cpu_quota: Option<String>,
    process_metrics_backend: String,
}

#[derive(Deserialize)]
struct CorpusInput {
    pinned_revision: Option<String>,
    checkout_revision: Option<String>,
    checkout_clean: bool,
    fixture_set_matches: bool,
}

#[derive(Clone, Deserialize, Serialize)]
struct EngineInput {
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

#[derive(Deserialize)]
struct FixtureInput {
    source: ArtifactInput,
    valid: bool,
    output_equal: bool,
    /// Engines that cannot run the fixture; they are neither measured nor bars on it.
    #[serde(default)]
    unsupported_engines: BTreeMap<String, String>,
    rounds: Vec<RoundInput>,
}

#[derive(Clone, Deserialize, Serialize)]
struct ArtifactInput {
    path: String,
    size_bytes: Option<u64>,
    modified_unix_ns: Option<u128>,
    sha256: Option<String>,
}

#[derive(Deserialize)]
struct RoundInput {
    round: usize,
    execution_order: Vec<String>,
    samples: BTreeMap<String, SampleInput>,
}

#[derive(Deserialize)]
struct SampleInput {
    status: i32,
    timed_out: bool,
    wall_ns: u128,
    peak_rss_bytes: Option<u64>,
    score: Option<f64>,
    #[serde(default)]
    stdout: String,
}

#[derive(Serialize)]
struct AnalysisReport {
    schema: u32,
    created_unix_ns: u128,
    input_report_path: String,
    input_report_sha256: String,
    suite: crate::Suite,
    jit_mode: crate::JitMode,
    quench_bench_binary: ArtifactInput,
    source_revision: String,
    source_dirty: bool,
    corpus_revision: String,
    timeout_ms: u64,
    host: HostInput,
    engine_provenance: Vec<EngineInput>,
    rounds_per_fixture: usize,
    analysis_runtime: String,
    suite_inputs: Vec<ArtifactInput>,
    bootstrap: BootstrapMethod,
    fixtures: BTreeMap<String, FixtureAnalysis>,
    qualified: bool,
}

#[derive(Serialize)]
struct DiagnosticAnalysisReport {
    schema: u32,
    report_kind: &'static str,
    created_unix_ns: u128,
    input_report_path: String,
    input_report_sha256: String,
    suite: crate::Suite,
    jit_mode: crate::JitMode,
    quench_bench_binary: ArtifactInput,
    source_revision: String,
    source_dirty: bool,
    corpus_revision: String,
    timeout_ms: u64,
    host: HostInput,
    engine_provenance: Vec<EngineInput>,
    rounds_per_fixture: usize,
    analysis_runtime: String,
    suite_inputs: Vec<ArtifactInput>,
    bootstrap: BootstrapMethod,
    fixtures: BTreeMap<String, DiagnosticFixtureAnalysis>,
    /// This experiment is diagnostic and can never qualify a runtime.
    qualified: bool,
}

#[derive(Serialize)]
struct BootstrapMethod {
    name: &'static str,
    interval_level: f64,
    replicates: usize,
    base_seed: String,
    resampling: &'static str,
    statistic: &'static str,
    interval: &'static str,
}

#[derive(Serialize)]
struct FixtureAnalysis {
    input_sha256: String,
    paired_rounds: usize,
    raw_rounds: Vec<RoundEvidence>,
    engine_medians: BTreeMap<String, EngineMedians>,
    comparisons: BTreeMap<String, PairAnalysis>,
    qualified: bool,
}

#[derive(Serialize)]
struct DiagnosticFixtureAnalysis {
    input_sha256: String,
    paired_rounds: usize,
    raw_rounds: Vec<RoundEvidence>,
    engine_medians: BTreeMap<String, EngineMedians>,
    comparisons: BTreeMap<String, DiagnosticPairAnalysis>,
    qualified: bool,
}

#[derive(Serialize)]
struct DiagnosticPairAnalysis {
    candidate_engine: String,
    reference_engine: String,
    score: DiagnosticMetricAnalysis,
    max_rss: DiagnosticMetricAnalysis,
}

#[derive(Serialize)]
struct DiagnosticMetricAnalysis {
    candidate_median: f64,
    reference_median: f64,
    median_ratio: f64,
    paired_ratio_interval_95: [f64; 2],
    interval_excludes_tie: bool,
    seed: String,
}

#[derive(Serialize)]
struct RoundEvidence {
    round: usize,
    execution_order: Vec<String>,
    samples: BTreeMap<String, RawSample>,
}

#[derive(Serialize)]
struct RawSample {
    status: i32,
    timed_out: bool,
    wall_ns: u128,
    score: f64,
    peak_rss_bytes: u64,
}

#[derive(Serialize)]
struct EngineMedians {
    score: f64,
    max_rss_bytes: f64,
}

#[derive(Serialize)]
struct PairAnalysis {
    score: MetricAnalysis,
    max_rss: MetricAnalysis,
    qualified: bool,
}

#[derive(Serialize)]
struct MetricAnalysis {
    quench_median: f64,
    comparator_median: f64,
    median_ratio: f64,
    paired_ratio_interval_95: [f64; 2],
    interval_excludes_tie: bool,
    seed: String,
}

struct EngineSamples {
    scores: Vec<f64>,
    rss_bytes: Vec<f64>,
}

pub(crate) fn run(args: &[String]) -> Result<(), String> {
    let (input_path, output_path) = parse_args(args)?;
    let input_bytes = fs::read(&input_path)
        .map_err(|error| format!("cannot read {}: {error}", input_path.display()))?;
    let input: SuiteInput = serde_json::from_slice(&input_bytes)
        .map_err(|error| format!("invalid benchmark report: {error}"))?;
    if input.jit_wasm_memcpy_ab
        || input.jit_wasm_memcpy_raw_ab
        || input.jit_wasm_memcpy_early_ab
        || input.jit_wasm_compare_branch_ab
        || input.jit_wasm_affine_pair_ab
        || input.jit_wasm_reference_body_cache_ab
        || input.jit_wasm_memcpy_leaf_ab
        || input.jit_wasm_selective_ab
    {
        let mut config = selected_diagnostic_ab_config(
            input.jit_wasm_memcpy_ab,
            input.jit_wasm_memcpy_raw_ab,
            input.jit_wasm_compare_branch_ab,
            input.jit_wasm_memcpy_early_ab,
            input.jit_wasm_affine_pair_ab,
            input.jit_wasm_reference_body_cache_ab,
            input.jit_wasm_memcpy_leaf_ab,
            input.jit_wasm_selective_ab,
        )?;
        if input.jit_wasm_reference_body_cache_ab
            && input.fixtures.len() == 1
            && input.fixtures.keys().next().is_some_and(|path| {
                path.ends_with("/sqlite3-wasm")
            })
        {
            config.fixture_name = "sqlite3-wasm";
        }
        validate_diagnostic_ab(&input, config)?;
        return run_diagnostic_ab_analysis(&input, &input_path, output_path.as_deref(), config);
    }
    validate_suite(&input)?;

    let input_report_sha256 = sha256(&input_path)
        .ok_or_else(|| format!("cannot hash {} with shasum", input_path.display()))?;
    let binary_path = std::env::current_exe()
        .map_err(|error| format!("cannot resolve analyzer executable: {error}"))?;
    let quench_bench_binary = artifact_input(&binary_path)?;
    let mut fixtures = BTreeMap::new();
    let engines = input.suite.engine_names();
    for (fixture_path, fixture) in &input.fixtures {
        let name = Path::new(fixture_path)
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| format!("invalid fixture path: {fixture_path}"))?;
        let measured = engines
            .iter()
            .copied()
            .filter(|engine| !fixture.unsupported_engines.contains_key(*engine))
            .collect::<Vec<_>>();
        fixtures.insert(
            name.to_string(),
            analyze_fixture(fixture_path, fixture, input.rounds_requested, &measured)?,
        );
    }
    let qualified = fixtures.values().all(|fixture| fixture.qualified);
    let corpus_revision = input
        .corpus
        .pinned_revision
        .clone()
        .ok_or_else(|| "benchmark corpus has no pinned revision".to_string())?;
    let engine_provenance = engines
        .iter()
        .map(|name| {
            input
                .engines
                .iter()
                .find(|engine| engine.name == *name)
                .expect("validated engine provenance")
        })
        .map(|engine| EngineInput {
            name: engine.name.clone(),
            executable: engine.executable.clone(),
            executable_sha256: engine.executable_sha256.clone(),
            version: engine.version.clone(),
            argv: engine.argv.clone(),
            environment: engine.environment.clone(),
            inherits_environment: engine.inherits_environment,
            jit_mode: engine.jit_mode.clone(),
            jit_proof_command: engine.jit_proof_command.clone(),
            jit_proof: engine.jit_proof.clone(),
        })
        .collect();
    let host = input.host;
    let analysis_runtime = format!("quench-bench Rust analyzer; {}", host.rustc);
    let report = AnalysisReport {
        schema: 2,
        created_unix_ns: now_ns(),
        input_report_path: input_path.display().to_string(),
        input_report_sha256,
        suite: input.suite,
        jit_mode: input.jit_mode,
        quench_bench_binary,
        source_revision: input.source_revision,
        source_dirty: input.source_dirty,
        corpus_revision,
        timeout_ms: input.timeout_ms,
        host,
        engine_provenance,
        rounds_per_fixture: input.rounds_requested,
        analysis_runtime,
        suite_inputs: input.suite_inputs,
        bootstrap: BootstrapMethod {
            name: "paired percentile bootstrap of median log-ratios, reported as ratios",
            interval_level: BOOTSTRAP_INTERVAL_LEVEL,
            replicates: BOOTSTRAP_REPLICATES,
            base_seed: format!("0x{BOOTSTRAP_BASE_SEED:016x}"),
            resampling: "resample round indices jointly for Quench and comparator",
            statistic: "exp(median(log(Quench / comparator)))",
            interval: "exponentiated linear-interpolated log-ratio percentiles",
        },
        fixtures,
        qualified,
    };
    let bytes = serde_json::to_vec_pretty(&report)
        .map_err(|error| format!("cannot encode analysis report: {error}"))?;
    if let Some(path) = output_path {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| format!("cannot create {}: {error}", path.display()))?;
        use std::io::Write;
        file.write_all(&bytes)
            .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
        println!("wrote {}", path.display());
    } else {
        println!("{}", String::from_utf8(bytes).expect("JSON is UTF-8"));
    }
    if !qualified {
        eprintln!("qualification: failed; see per-fixture comparisons");
        std::process::exit(1);
    }
    let comparison_count = report
        .fixtures
        .values()
        .map(|fixture| fixture.comparisons.len())
        .sum::<usize>();
    eprintln!(
        "qualification: passed all {} selected fixtures and {comparison_count} fixture-engine comparisons for {:?}",
        report.fixtures.len(),
        report.suite
    );
    Ok(())
}

fn parse_args(args: &[String]) -> Result<(PathBuf, Option<PathBuf>), String> {
    let input = args
        .first()
        .map(PathBuf::from)
        .ok_or_else(|| "usage: quench-bench --analyze REPORT [--out JSON]".to_string())?;
    let mut output = None;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--out" if index + 1 < args.len() => {
                output = Some(PathBuf::from(&args[index + 1]));
                index += 2;
            }
            "--out" => return Err("--out requires a path".to_string()),
            other => return Err(format!("unknown analysis option: {other}")),
        }
    }
    Ok((input, output))
}

fn run_diagnostic_ab_analysis(
    input: &SuiteInput,
    input_path: &Path,
    output_path: Option<&Path>,
    config: DiagnosticAbConfig,
) -> Result<(), String> {
    let input_report_sha256 = sha256(input_path)
        .ok_or_else(|| format!("cannot hash {} with shasum", input_path.display()))?;
    let binary_path = std::env::current_exe()
        .map_err(|error| format!("cannot resolve analyzer executable: {error}"))?;
    let quench_bench_binary = artifact_input(&binary_path)?;
    let fixture_path = input.fixtures.keys().next().expect("validated one fixture");
    let fixture = input
        .fixtures
        .get(fixture_path)
        .expect("validated fixture key");
    let fixture_name = Path::new(fixture_path)
        .file_name()
        .and_then(|name| name.to_str())
        .expect("validated fixture name");
    let fixture_analysis =
        analyze_diagnostic_ab_fixture(fixture_path, fixture, input.rounds_requested, config)?;
    let corpus_revision = input
        .corpus
        .pinned_revision
        .clone()
        .ok_or_else(|| "benchmark corpus has no pinned revision".to_string())?;
    let engine_provenance = diagnostic_ab_engines(config)
        .iter()
        .map(|name| {
            input
                .engines
                .iter()
                .find(|engine| engine.name == *name)
                .expect("validated diagnostic engine")
                .clone()
        })
        .collect();
    let host = input.host.clone();
    let report = DiagnosticAnalysisReport {
        schema: 1,
        report_kind: config.report_kind,
        created_unix_ns: now_ns(),
        input_report_path: input_path.display().to_string(),
        input_report_sha256,
        suite: input.suite,
        jit_mode: input.jit_mode,
        quench_bench_binary,
        source_revision: input.source_revision.clone(),
        source_dirty: input.source_dirty,
        corpus_revision,
        timeout_ms: input.timeout_ms,
        host,
        engine_provenance,
        rounds_per_fixture: input.rounds_requested,
        analysis_runtime: format!("quench-bench Rust analyzer; {}", input.host.rustc),
        suite_inputs: input
            .suite_inputs
            .iter()
            .map(clone_artifact_input)
            .collect(),
        bootstrap: BootstrapMethod {
            name: "paired percentile bootstrap of median log-ratios, reported as ratios",
            interval_level: BOOTSTRAP_INTERVAL_LEVEL,
            replicates: BOOTSTRAP_REPLICATES,
            base_seed: format!("0x{BOOTSTRAP_BASE_SEED:016x}"),
            resampling: "resample matching round indices jointly for candidate and reference",
            statistic: "exp(median(log(candidate / reference)))",
            interval: "exponentiated linear-interpolated log-ratio percentiles",
        },
        fixtures: BTreeMap::from([(fixture_name.to_string(), fixture_analysis)]),
        qualified: false,
    };
    let bytes = serde_json::to_vec_pretty(&report)
        .map_err(|error| format!("cannot encode diagnostic analysis report: {error}"))?;
    if let Some(path) = output_path {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|error| format!("cannot create {}: {error}", path.display()))?;
        use std::io::Write;
        file.write_all(&bytes)
            .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
        println!("wrote {}", path.display());
    } else {
        println!("{}", String::from_utf8(bytes).expect("JSON is UTF-8"));
    }
    eprintln!("diagnostic A/B analysis emitted; qualification is always false");
    Ok(())
}

fn clone_artifact_input(input: &ArtifactInput) -> ArtifactInput {
    ArtifactInput {
        path: input.path.clone(),
        size_bytes: input.size_bytes,
        modified_unix_ns: input.modified_unix_ns,
        sha256: input.sha256.clone(),
    }
}

fn validate_diagnostic_ab(input: &SuiteInput, config: DiagnosticAbConfig) -> Result<(), String> {
    if !matches!(input.schema, 5 | 6 | 7 | 9 | 10 | 11 | 12 | 14)
        && input.schema != crate::SUITE_REPORT_SCHEMA
    {
        return Err(format!(
            "unsupported benchmark report schema {}",
            input.schema
        ));
    }
    if input.suite != crate::Suite::JetStream3 || input.jit_mode != crate::JitMode::Jit {
        return Err(format!(
            "{} requires JetStream3 default JIT mode",
            config.report_kind
        ));
    }
    let direct_copy = config.mode == DiagnosticAbMode::DirectMemcpy;
    let raw_i32 = config.mode == DiagnosticAbMode::RawI32Memcpy;
    let early_import = config.mode == DiagnosticAbMode::EarlyImportMemcpy;
    let memcpy_leaf = config.mode == DiagnosticAbMode::NativeMemcpyLeaf;
    let raw_i32_enabled = raw_i32 || early_import || memcpy_leaf;
    let early_import_enabled = early_import || memcpy_leaf;
    let compare_branches = config.mode == DiagnosticAbMode::DirectCompareBranches;
    let affine_pair = config.mode == DiagnosticAbMode::AffinePair;
    let reference_body_cache = config.mode == DiagnosticAbMode::ReferenceBodyCache;
    let selective_handoff = config.mode == DiagnosticAbMode::SelectiveHandoff;
    if input.jit_wasm_memcpy_ab != direct_copy
        || input.jit_wasm_memcpy_raw_ab != raw_i32
        || input.jit_wasm_memcpy_early_ab != early_import
        || input.jit_wasm_compare_branch_ab != compare_branches
        || input.jit_wasm_affine_pair_ab != affine_pair
        || input.jit_wasm_reference_body_cache_ab != reference_body_cache
        || input.jit_wasm_memcpy_leaf_ab != memcpy_leaf
        || input.jit_wasm_selective_ab != selective_handoff
        || input.jit_diagnostics
        || input.jit_wasm_root.is_some()
        || input.jit_wasm_root_only
        || input.jit_wasm_root_first
        || input.jit_wasm_selective
    {
        return Err(format!(
            "{} report contains conflicting diagnostic modes",
            config.report_kind
        ));
    }
    if !input.complete || input.qualification_ready {
        return Err(format!(
            "{} requires a complete diagnostic report with qualification_ready=false",
            config.report_kind
        ));
    }
    if input.rounds_requested < config.minimum_rounds {
        return Err(format!(
            "{} has {} rounds; at least {} are required",
            config.report_kind, input.rounds_requested, config.minimum_rounds,
        ));
    }
    if input.timeout_ms == 0
        || input.host.uname.trim().is_empty()
        || input.host.rustc.trim().is_empty()
        || input.host.process_metrics_backend.trim().is_empty()
    {
        return Err(format!(
            "{} report is missing timeout or host/toolchain provenance",
            config.report_kind
        ));
    }
    if !input.corpus.checkout_clean
        || input.corpus.pinned_revision != input.corpus.checkout_revision
        || input.corpus.fixture_set_matches
    {
        return Err(format!(
            "{} corpus is dirty, unpinned, or has a mismatched fixture-set claim",
            config.report_kind
        ));
    }
    if input.source_revision.len() != 40 || !is_hex(&input.source_revision) {
        return Err(format!(
            "{} report has an invalid source revision",
            config.report_kind
        ));
    }
    let suite_input_paths = input
        .suite_inputs
        .iter()
        .map(|artifact| artifact.path.as_str())
        .collect::<BTreeSet<_>>();
    if suite_input_paths.len() != input.suite_inputs.len()
        || input.suite_inputs.is_empty()
        || input.suite_inputs.iter().any(|artifact| {
            artifact.path.is_empty()
                || !artifact
                    .sha256
                    .as_deref()
                    .is_some_and(|hash| hash.len() == 64 && is_hex(hash))
        })
    {
        return Err(format!(
            "{} report has incomplete suite input provenance",
            config.report_kind
        ));
    }

    let engine_names = input
        .engines
        .iter()
        .map(|engine| engine.name.as_str())
        .collect::<BTreeSet<_>>();
    let expected_engines = diagnostic_ab_engines(config);
    if engine_names.len() != expected_engines.len()
        || input.engines.len() != expected_engines.len()
        || expected_engines
            .iter()
            .any(|name| !engine_names.contains(*name))
    {
        return Err(format!(
            "{} report must contain exactly the two Quench arms, Bun, and Node",
            config.report_kind
        ));
    }
    let engine = |name: &str| {
        input
            .engines
            .iter()
            .find(|engine| engine.name == name)
            .expect("validated diagnostic engine set")
    };
    let off_engine = engine(config.off_engine);
    let on_engine = engine(config.on_engine);
    for quench in [off_engine, on_engine] {
        if quench.executable.is_empty()
            || quench.version.trim().is_empty()
            || quench.executable_sha256.len() != 64
            || !is_hex(&quench.executable_sha256)
            || quench.jit_proof_command.is_empty()
            || quench.inherits_environment
            || quench.jit_mode != "enabled-by-cli-flag"
            || quench.argv.len() != 1
            || quench.argv[0] != "--jit"
            || quench.jit_proof_command.len() != 2
            || quench.jit_proof_command[0] != quench.executable
            || quench.jit_proof_command[1] != "--jit"
            || !quench.jit_proof.contains("--jit")
            || quench
                .environment
                .keys()
                .any(|key| key.starts_with("QUENCH_WASM_JIT_") && key != config.switch_env)
            || (!raw_i32_enabled && quench.environment.contains_key(MEMCPY_RAW_ENV))
            || (!early_import_enabled && quench.environment.contains_key(MEMCPY_EARLY_ENV))
            || (compare_branches && quench.environment.contains_key(MEMCPY_DIRECT_ENV))
        {
            return Err(format!(
                "{} has invalid default-JIT provenance",
                quench.name
            ));
        }
    }
    if off_engine.executable != on_engine.executable
        || off_engine.executable_sha256 != on_engine.executable_sha256
        || off_engine.version != on_engine.version
        || off_engine.argv != on_engine.argv
        || off_engine.jit_mode != on_engine.jit_mode
        || off_engine.jit_proof_command != on_engine.jit_proof_command
    {
        return Err(format!(
            "{} Quench arms must use the same executable and --jit arguments",
            config.report_kind
        ));
    }
    if off_engine
        .environment
        .get(config.switch_env)
        .map_or(true, |value| value != config.off_value)
        || on_engine
            .environment
            .get(config.switch_env)
            .map_or(true, |value| value != config.on_value)
    {
        return Err(format!(
            "{} must explicitly set its experiment environment to 0 and 1",
            config.report_kind
        ));
    }
    for (fixed_key, fixed_value) in [config.fixed_env, config.fixed_env2, config.fixed_env3]
        .into_iter()
        .flatten()
    {
        if off_engine.environment.get(fixed_key).map(String::as_str) != Some(fixed_value)
            || on_engine.environment.get(fixed_key).map(String::as_str) != Some(fixed_value)
        {
            return Err(format!(
                "{} must keep {fixed_key}={fixed_value} in both Quench arms",
                config.report_kind
            ));
        }
    }
    let mut off_environment = off_engine.environment.clone();
    let mut on_environment = on_engine.environment.clone();
    off_environment.remove(config.switch_env);
    on_environment.remove(config.switch_env);
    if off_environment != on_environment {
        return Err(format!(
            "{} Quench environments differ beyond the experiment switch",
            config.report_kind
        ));
    }
    validate_engine(engine("bun"), input.suite, input.jit_mode)?;
    validate_engine(engine("node"), input.suite, input.jit_mode)?;

    if input.fixtures.len() != 1 {
        return Err(format!(
            "{} report must contain only {}",
            config.report_kind, config.fixture_name
        ));
    }
    let (fixture_path, fixture) = input.fixtures.iter().next().expect("one fixture");
    let expected_fixture_path = crate::Suite::JetStream3
        .fixture(config.fixture_name)
        .display()
        .to_string();
    let fixture_name = Path::new(fixture_path)
        .file_name()
        .and_then(|name| name.to_str());
    if fixture_path != &expected_fixture_path
        || fixture_name != Some(config.fixture_name)
        || fixture.source.path != *fixture_path
    {
        return Err(format!(
            "{} report is pinned to the {} fixture",
            config.report_kind, config.fixture_name
        ));
    }
    if !fixture.valid || !fixture.output_equal || !fixture.unsupported_engines.is_empty() {
        return Err(format!(
            "{} requires valid, output-equal samples from all four engines",
            config.report_kind
        ));
    }
    if fixture.rounds.len() != input.rounds_requested {
        return Err(format!(
            "{} contains {} rounds; report requested {}",
            config.fixture_name,
            fixture.rounds.len(),
            input.rounds_requested
        ));
    }
    Ok(())
}

fn analyze_diagnostic_ab_fixture(
    path: &str,
    fixture: &FixtureInput,
    requested_rounds: usize,
    config: DiagnosticAbConfig,
) -> Result<DiagnosticFixtureAnalysis, String> {
    if !fixture.valid || !fixture.output_equal || !fixture.unsupported_engines.is_empty() {
        return Err(format!("{path} is invalid or engine outputs differ"));
    }
    let input_sha256 = fixture
        .source
        .sha256
        .clone()
        .filter(|hash| hash.len() == 64 && is_hex(hash))
        .ok_or_else(|| format!("{path} has no valid fixture SHA-256"))?;
    if requested_rounds < config.minimum_rounds || fixture.rounds.len() != requested_rounds {
        return Err(format!(
            "{path} must contain all requested matching rounds (at least {})",
            config.minimum_rounds
        ));
    }

    let engines = diagnostic_ab_engines(config);
    let mut samples = engines
        .iter()
        .map(|engine| {
            (
                (*engine).to_string(),
                EngineSamples {
                    scores: Vec::with_capacity(requested_rounds),
                    rss_bytes: Vec::with_capacity(requested_rounds),
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut raw_rounds = Vec::with_capacity(requested_rounds);
    let mut expected_semantic_output: Option<String> = None;
    for (index, round) in fixture.rounds.iter().enumerate() {
        if round.round != index {
            return Err(format!("{path} has noncontiguous round identities"));
        }
        let expected_order = engines
            .iter()
            .cycle()
            .skip(index % engines.len())
            .take(engines.len())
            .map(|engine| (*engine).to_string())
            .collect::<Vec<_>>();
        if round.execution_order != expected_order {
            return Err(format!(
                "{path} round {index} did not use the recorded rotating order"
            ));
        }
        if round.samples.len() != engines.len() {
            return Err(format!("{path} round {index} is missing an engine sample"));
        }
        let mut raw_samples = BTreeMap::new();
        for engine in engines {
            let sample = round
                .samples
                .get(engine)
                .ok_or_else(|| format!("{path} round {index} is missing the {engine} sample"))?;
            if sample.status != 0 || sample.timed_out {
                return Err(format!("{path} round {index} has an invalid {engine} exit"));
            }
            if sample.stdout.is_empty() {
                return Err(format!(
                    "{path} round {index} is missing {engine} output for equality checking"
                ));
            }
            let semantic_output = crate::jetstream::semantic_output(&sample.stdout);
            if expected_semantic_output
                .as_ref()
                .is_some_and(|expected| expected != &semantic_output)
            {
                return Err(format!("{path} round {index} has differing engine outputs"));
            }
            expected_semantic_output.get_or_insert(semantic_output);
            let score = sample
                .score
                .filter(|score| score.is_finite() && *score > 0.0)
                .ok_or_else(|| format!("{path} round {index} has an invalid {engine} score"))?;
            if crate::jetstream::score(Path::new(path), &sample.stdout) != Some(score) {
                return Err(format!(
                    "{path} round {index} has a score/output mismatch for {engine}"
                ));
            }
            let rss = sample
                .peak_rss_bytes
                .filter(|rss| *rss > 0)
                .ok_or_else(|| format!("{path} round {index} has invalid {engine} RSS"))?;
            let engine_samples = samples
                .get_mut(engine)
                .expect("all diagnostic engines initialized");
            engine_samples.scores.push(score);
            engine_samples.rss_bytes.push(rss as f64);
            raw_samples.insert(
                (*engine).to_string(),
                RawSample {
                    status: sample.status,
                    timed_out: sample.timed_out,
                    wall_ns: sample.wall_ns,
                    score,
                    peak_rss_bytes: rss,
                },
            );
        }
        raw_rounds.push(RoundEvidence {
            round: round.round,
            execution_order: round.execution_order.clone(),
            samples: raw_samples,
        });
    }
    let engine_medians = samples
        .iter()
        .map(|(name, sample)| {
            (
                name.clone(),
                EngineMedians {
                    score: median(&sample.scores),
                    max_rss_bytes: median(&sample.rss_bytes),
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    let comparisons_to_run = [
        (config.on_engine, config.off_engine),
        (config.on_engine, "bun"),
        (config.on_engine, "node"),
    ];
    let mut comparisons = BTreeMap::new();
    for (candidate, reference) in comparisons_to_run {
        let candidate_samples = samples.get(candidate).expect("known candidate");
        let reference_samples = samples.get(reference).expect("known reference");
        let score = analyze_diagnostic_metric(
            path,
            candidate,
            reference,
            "score",
            &candidate_samples.scores,
            &reference_samples.scores,
        );
        let max_rss = analyze_diagnostic_metric(
            path,
            candidate,
            reference,
            "max_rss",
            &candidate_samples.rss_bytes,
            &reference_samples.rss_bytes,
        );
        comparisons.insert(
            format!("{candidate}_vs_{reference}"),
            DiagnosticPairAnalysis {
                candidate_engine: candidate.into(),
                reference_engine: reference.into(),
                score,
                max_rss,
            },
        );
    }
    Ok(DiagnosticFixtureAnalysis {
        input_sha256,
        paired_rounds: requested_rounds,
        raw_rounds,
        engine_medians,
        comparisons,
        qualified: false,
    })
}

fn analyze_diagnostic_metric(
    path: &str,
    candidate_name: &str,
    reference_name: &str,
    metric: &str,
    candidate: &[f64],
    reference: &[f64],
) -> DiagnosticMetricAnalysis {
    let candidate_median = median(candidate);
    let reference_median = median(reference);
    let pair_name = format!("{candidate_name}_vs_{reference_name}");
    let seed = derive_seed(path, &pair_name, metric);
    let paired_ratio_interval_95 =
        paired_bootstrap_log_ratio_interval(candidate, reference, BOOTSTRAP_REPLICATES, seed);
    DiagnosticMetricAnalysis {
        candidate_median,
        reference_median,
        median_ratio: median_log_ratio(candidate, reference).exp(),
        interval_excludes_tie: paired_ratio_interval_95[0] > 1.0
            || paired_ratio_interval_95[1] < 1.0,
        paired_ratio_interval_95,
        seed: format!("0x{seed:016x}"),
    }
}

fn validate_suite(input: &SuiteInput) -> Result<(), String> {
    if !matches!(input.schema, 5 | 6 | 7 | 9 | 10 | 11 | 12 | 14)
        && input.schema != crate::SUITE_REPORT_SCHEMA
    {
        return Err(format!(
            "unsupported benchmark report schema {}",
            input.schema
        ));
    }
    if input.jit_diagnostics {
        return Err("JIT-diagnostic reports cannot be used for qualification analysis".into());
    }
    if input.jit_wasm_root_only {
        return Err("root-only JIT reports cannot be used for qualification analysis".into());
    }
    if input.jit_wasm_root_first {
        return Err("root-first JIT reports cannot be used for qualification analysis".into());
    }
    if input.jit_wasm_root.is_some() || input.jit_wasm_selective {
        return Err(
            "experimental Wasm tiering reports cannot be used for qualification analysis".into(),
        );
    }
    if input.jit_wasm_memcpy_ab
        || input.jit_wasm_memcpy_raw_ab
        || input.jit_wasm_memcpy_early_ab
        || input.jit_wasm_affine_pair_ab
        || input.jit_wasm_reference_body_cache_ab
        || input.jit_wasm_memcpy_leaf_ab
        || input.jit_wasm_selective_ab
    {
        return Err(
            "Wasm JIT A/B diagnostic reports cannot be used for qualification analysis".into(),
        );
    }
    if input.jit_wasm_compare_branch_ab {
        return Err(
            "Wasm compare-branch A/B diagnostic reports cannot be used for qualification analysis"
                .into(),
        );
    }
    if !input.qualification_ready || !input.complete {
        return Err("benchmark report is incomplete or not qualification_ready".to_string());
    }
    if input.rounds_requested < MIN_QUALIFYING_ROUNDS {
        return Err(format!(
            "benchmark report has {} rounds; at least {MIN_QUALIFYING_ROUNDS} are required",
            input.rounds_requested
        ));
    }
    if input.source_dirty {
        return Err("benchmark source tree was dirty".to_string());
    }
    if input.timeout_ms == 0
        || input.host.uname.trim().is_empty()
        || input.host.rustc.trim().is_empty()
        || input.host.process_metrics_backend.trim().is_empty()
    {
        return Err("benchmark report is missing timeout or host/toolchain provenance".to_string());
    }
    if !input.corpus.checkout_clean
        || input.corpus.pinned_revision != input.corpus.checkout_revision
    {
        return Err("benchmark corpus is dirty or unpinned".to_string());
    }
    let engines = input
        .engines
        .iter()
        .map(|engine| engine.name.as_str())
        .collect::<BTreeSet<_>>();
    let required = input.suite.engine_names();
    let expected = required.iter().copied().collect::<BTreeSet<_>>();
    if engines != expected || input.engines.len() != required.len() {
        return Err(format!(
            "benchmark report does not contain exactly the required engines: {}",
            required.join(", ")
        ));
    }
    for engine in &input.engines {
        validate_engine(engine, input.suite, input.jit_mode)?;
    }
    if input.fixtures.is_empty()
        || input
            .fixtures
            .iter()
            .any(|(path, fixture)| fixture.source.path != *path)
        || !crate::fixture_selection_is_valid(
            input.suite,
            &input.fixtures.keys().map(PathBuf::from).collect::<Vec<_>>(),
        )
    {
        return Err("benchmark report contains an invalid fixture selection".to_string());
    }
    let complete_fixture_set = input.fixtures.len() == input.suite.fixture_names().len();
    if input.corpus.fixture_set_matches != complete_fixture_set {
        return Err(
            "benchmark corpus fixture-set proof does not match the selected fixtures".into(),
        );
    }
    let suite_input_paths = input
        .suite_inputs
        .iter()
        .map(|artifact| artifact.path.as_str())
        .collect::<BTreeSet<_>>();
    if suite_input_paths.len() != input.suite_inputs.len()
        || input.suite_inputs.is_empty()
        || input.suite_inputs.iter().any(|artifact| {
            artifact.path.is_empty()
                || !artifact
                    .sha256
                    .as_deref()
                    .is_some_and(|hash| hash.len() == 64 && is_hex(hash))
        })
    {
        return Err(
            "benchmark report has incomplete or duplicate suite input provenance".to_string(),
        );
    }
    if input.source_revision.len() != 40 || !is_hex(&input.source_revision) {
        return Err("benchmark report has an invalid source revision".to_string());
    }
    Ok(())
}

fn validate_engine(
    engine: &EngineInput,
    suite: crate::Suite,
    jit_mode: crate::JitMode,
) -> Result<(), String> {
    if engine.executable.is_empty()
        || engine.version.trim().is_empty()
        || engine.executable_sha256.len() != 64
        || !is_hex(&engine.executable_sha256)
        || engine.jit_proof_command.is_empty()
        || engine.inherits_environment
    {
        return Err(format!("{} is missing executable provenance", engine.name));
    }
    let proof_valid = match (engine.name.as_str(), suite, jit_mode) {
        ("quench", _, crate::JitMode::NoJit) => {
            engine.jit_mode == "interpreter" && engine.jit_proof.contains("no guest JIT path")
        }
        ("quickjs", _, crate::JitMode::NoJit) => {
            engine.jit_mode == "interpreter" && engine.jit_proof.contains("interpreter")
        }
        ("bun", _, crate::JitMode::NoJit) => {
            engine.jit_mode == "disabled-and-probed"
                && engine
                    .environment
                    .get("BUN_JSC_useJIT")
                    .is_some_and(|v| v == "0")
                && engine.jit_proof.contains("useJIT=false")
        }
        ("node", _, crate::JitMode::NoJit) => {
            engine.jit_mode == "disabled-and-probed"
                && engine.argv.iter().any(|arg| arg == "--jitless")
                && engine.jit_proof.contains("true")
        }
        ("quench", crate::Suite::JetStream3, crate::JitMode::Jit) => {
            engine.jit_mode == "enabled-by-cli-flag"
                && engine.argv.iter().any(|arg| arg == "--jit")
                && engine.jit_proof.contains("--jit")
        }
        ("bun", crate::Suite::JetStream3, crate::JitMode::Jit) => {
            engine.jit_mode == "enabled-and-probed"
                && engine
                    .environment
                    .get("BUN_JSC_useJIT")
                    .is_some_and(|value| value == "1")
                && engine.jit_proof.contains("useJIT=true")
        }
        ("node", crate::Suite::JetStream3, crate::JitMode::Jit) => {
            engine.jit_mode == "enabled-and-probed"
                && !engine.argv.iter().any(|arg| arg == "--jitless")
                && engine.jit_proof.contains("completed compiling")
        }
        _ => false,
    };
    if !proof_valid {
        return Err(format!(
            "{} lacks valid JIT-mode proof for this suite",
            engine.name
        ));
    }
    Ok(())
}

fn artifact_input(path: &Path) -> Result<ArtifactInput, String> {
    let metadata =
        fs::metadata(path).map_err(|error| format!("cannot stat {}: {error}", path.display()))?;
    let modified_unix_ns = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos());
    let hash = sha256(path).ok_or_else(|| format!("cannot hash {}", path.display()))?;
    Ok(ArtifactInput {
        path: path.display().to_string(),
        size_bytes: Some(metadata.len()),
        modified_unix_ns,
        sha256: Some(hash),
    })
}

fn analyze_fixture(
    path: &str,
    fixture: &FixtureInput,
    requested_rounds: usize,
    engines: &[&str],
) -> Result<FixtureAnalysis, String> {
    if !fixture.valid || !fixture.output_equal {
        return Err(format!("{path} is invalid or its engine outputs differ"));
    }
    let input_sha256 = fixture
        .source
        .sha256
        .clone()
        .filter(|hash| hash.len() == 64 && is_hex(hash))
        .ok_or_else(|| format!("{path} has no valid fixture SHA-256"))?;
    if fixture.rounds.len() != requested_rounds {
        return Err(format!(
            "{path} contains {} rounds; report requested {requested_rounds}",
            fixture.rounds.len()
        ));
    }

    let mut samples = engines
        .iter()
        .map(|engine| {
            (
                (*engine).to_string(),
                EngineSamples {
                    scores: Vec::with_capacity(requested_rounds),
                    rss_bytes: Vec::with_capacity(requested_rounds),
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut raw_rounds = Vec::with_capacity(requested_rounds);
    for (index, round) in fixture.rounds.iter().enumerate() {
        if round.round != index {
            return Err(format!("{path} has noncontiguous round identities"));
        }
        let expected_order = engines
            .iter()
            .cycle()
            .skip(index % engines.len())
            .take(engines.len())
            .copied()
            .map(str::to_string)
            .collect::<Vec<_>>();
        if round.execution_order != expected_order {
            return Err(format!(
                "{path} round {index} did not use the recorded rotating order"
            ));
        }
        if round.samples.len() != engines.len() {
            return Err(format!("{path} round {index} is missing an engine sample"));
        }
        let mut raw_samples = BTreeMap::new();
        for engine in engines {
            let sample = round
                .samples
                .get(*engine)
                .ok_or_else(|| format!("{path} round {index} is missing the {engine} sample"))?;
            if sample.status != 0 || sample.timed_out {
                return Err(format!("{path} round {index} has an invalid {engine} exit"));
            }
            let score = sample
                .score
                .filter(|score| score.is_finite())
                .ok_or_else(|| format!("{path} round {index} has an invalid {engine} score"))?;
            let rss = sample
                .peak_rss_bytes
                .filter(|rss| *rss > 0)
                .ok_or_else(|| format!("{path} round {index} has invalid {engine} RSS"))?;
            let engine_samples = samples.get_mut(*engine).expect("all engines initialized");
            engine_samples.scores.push(score);
            engine_samples.rss_bytes.push(rss as f64);
            raw_samples.insert(
                (*engine).to_string(),
                RawSample {
                    status: sample.status,
                    timed_out: sample.timed_out,
                    wall_ns: sample.wall_ns,
                    score,
                    peak_rss_bytes: rss,
                },
            );
        }
        raw_rounds.push(RoundEvidence {
            round: round.round,
            execution_order: round.execution_order.clone(),
            samples: raw_samples,
        });
    }

    let engine_medians = samples
        .iter()
        .map(|(name, sample)| {
            (
                name.clone(),
                EngineMedians {
                    score: median(&sample.scores),
                    max_rss_bytes: median(&sample.rss_bytes),
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    let quench = samples.get(QUENCH).expect("quench samples");
    let mut comparisons = BTreeMap::new();
    for comparator in engines.iter().filter(|engine| **engine != QUENCH) {
        let reference = samples.get(*comparator).expect("comparator samples");
        let score = analyze_metric(path, comparator, "score", &quench.scores, &reference.scores);
        let max_rss = analyze_metric(
            path,
            comparator,
            "max_rss",
            &quench.rss_bytes,
            &reference.rss_bytes,
        );
        let qualified = score.median_ratio > 1.0
            && score.paired_ratio_interval_95[0] > 1.0
            && max_rss.median_ratio < 1.0
            && max_rss.paired_ratio_interval_95[1] < 1.0;
        comparisons.insert(
            (*comparator).to_string(),
            PairAnalysis {
                score,
                max_rss,
                qualified,
            },
        );
    }
    let qualified = comparisons.values().all(|comparison| comparison.qualified);
    Ok(FixtureAnalysis {
        input_sha256,
        paired_rounds: requested_rounds,
        raw_rounds,
        engine_medians,
        comparisons,
        qualified,
    })
}

fn analyze_metric(
    path: &str,
    comparator: &str,
    metric: &str,
    quench: &[f64],
    reference: &[f64],
) -> MetricAnalysis {
    let quench_median = median(quench);
    let comparator_median = median(reference);
    let seed = derive_seed(path, comparator, metric);
    let median_ratio = median_log_ratio(quench, reference).exp();
    let paired_ratio_interval_95 =
        paired_bootstrap_log_ratio_interval(quench, reference, BOOTSTRAP_REPLICATES, seed);
    MetricAnalysis {
        quench_median,
        comparator_median,
        median_ratio,
        interval_excludes_tie: paired_ratio_interval_95[0] > 1.0
            || paired_ratio_interval_95[1] < 1.0,
        paired_ratio_interval_95,
        seed: format!("0x{seed:016x}"),
    }
}

fn median_log_ratio(quench: &[f64], reference: &[f64]) -> f64 {
    median(
        &quench
            .iter()
            .zip(reference)
            .map(|(quench, reference)| (quench / reference).ln())
            .collect::<Vec<_>>(),
    )
}

fn paired_bootstrap_log_ratio_interval(
    quench: &[f64],
    reference: &[f64],
    replicates: usize,
    seed: u64,
) -> [f64; 2] {
    let mut random = XorShift64::new(seed);
    let mut log_ratios = vec![0.0; quench.len()];
    let mut log_ratios_by_replicate = Vec::with_capacity(replicates);
    for _ in 0..replicates {
        for slot in &mut log_ratios {
            let round = random.index(quench.len());
            *slot = (quench[round] / reference[round]).ln();
        }
        log_ratios_by_replicate.push(median(&log_ratios));
    }
    log_ratios_by_replicate.sort_by(f64::total_cmp);
    let tail = (1.0 - BOOTSTRAP_INTERVAL_LEVEL) / 2.0;
    [
        quantile(&log_ratios_by_replicate, tail).exp(),
        quantile(&log_ratios_by_replicate, 1.0 - tail).exp(),
    ]
}

fn median(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let middle = sorted.len() / 2;
    if sorted.len() % 2 == 0 {
        (sorted[middle - 1] + sorted[middle]) / 2.0
    } else {
        sorted[middle]
    }
}

fn quantile(sorted: &[f64], probability: f64) -> f64 {
    let position = probability * (sorted.len() - 1) as f64;
    let lower = position.floor() as usize;
    let upper = position.ceil() as usize;
    let fraction = position - lower as f64;
    sorted[lower] + (sorted[upper] - sorted[lower]) * fraction
}

fn derive_seed(path: &str, comparator: &str, metric: &str) -> u64 {
    let mut hash = BOOTSTRAP_BASE_SEED ^ 0xcbf2_9ce4_8422_2325;
    for byte in path
        .bytes()
        .chain([0])
        .chain(comparator.bytes())
        .chain([0])
        .chain(metric.bytes())
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn is_hex(value: &str) -> bool {
    value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

struct XorShift64 {
    state: u64,
}

impl XorShift64 {
    fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 { BOOTSTRAP_BASE_SEED } else { seed },
        }
    }

    fn next(&mut self) -> u64 {
        let mut value = self.state;
        value ^= value >> 12;
        value ^= value << 25;
        value ^= value >> 27;
        self.state = value;
        value.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn index(&mut self, length: usize) -> usize {
        (self.next() % length as u64) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AFFINE_PAIR_CONFIG, AFFINE_PAIR_ENV, AFFINE_PAIR_OFF, AFFINE_PAIR_ON, ArtifactInput,
        CorpusInput, DIRECT_COMPARE_BRANCHES_CONFIG, DIRECT_COMPARE_BRANCHES_ENV,
        DIRECT_COMPARE_BRANCHES_OFF, DIRECT_COMPARE_BRANCHES_ON, DiagnosticAbConfig,
        DiagnosticAbMode, EngineInput, FixtureInput, HostInput, MEMCPY_DIRECT_CONFIG,
        MEMCPY_DIRECT_ENV, MEMCPY_DIRECT_OFF, MEMCPY_DIRECT_ON, MEMCPY_EARLY_CONFIG,
        MEMCPY_EARLY_ENV, MEMCPY_EARLY_OFF, MEMCPY_EARLY_ON, MEMCPY_LEAF_CONFIG, MEMCPY_LEAF_ENV,
        MEMCPY_LEAF_OFF, MEMCPY_LEAF_ON, MEMCPY_RAW_CONFIG, MEMCPY_RAW_ENV,
        REFERENCE_BODY_CACHE_CONFIG, REFERENCE_BODY_CACHE_ENV, REFERENCE_BODY_CACHE_OFF,
        REFERENCE_BODY_CACHE_ON, RoundInput, SELECTIVE_HANDOFF_CONFIG, SELECTIVE_HANDOFF_ENV,
        SELECTIVE_HANDOFF_OFF, SELECTIVE_HANDOFF_ON, SampleInput, SuiteInput,
        analyze_diagnostic_ab_fixture, median, median_log_ratio,
        paired_bootstrap_log_ratio_interval, selected_diagnostic_ab_config, validate_diagnostic_ab,
        validate_engine, validate_suite,
    };
    use crate::{JitMode, Suite};
    use std::collections::BTreeMap;

    fn engine(
        name: &str,
        jit_mode: &str,
        argv: &[&str],
        environment: &[(&str, &str)],
        jit_proof: &str,
    ) -> EngineInput {
        EngineInput {
            name: name.into(),
            executable: format!("/{name}"),
            executable_sha256: "a".repeat(64),
            version: "test".into(),
            argv: argv.iter().map(|value| (*value).into()).collect(),
            environment: environment
                .iter()
                .map(|(key, value)| ((*key).into(), (*value).into()))
                .collect::<BTreeMap<_, _>>(),
            inherits_environment: false,
            jit_mode: jit_mode.into(),
            jit_proof_command: vec!["probe".into()],
            jit_proof: jit_proof.into(),
        }
    }

    fn memcpy_ab_input() -> SuiteInput {
        memcpy_ab_input_for(&MEMCPY_DIRECT_CONFIG)
    }

    fn memcpy_ab_input_for(config: &DiagnosticAbConfig) -> SuiteInput {
        let mut off_environment = vec![(config.switch_env, config.off_value)];
        let mut on_environment = vec![(config.switch_env, config.on_value)];
        if let Some(fixed) = config.fixed_env {
            off_environment.push(fixed);
            on_environment.push(fixed);
        }
        if let Some(fixed) = config.fixed_env2 {
            off_environment.push(fixed);
            on_environment.push(fixed);
        }
        if let Some(fixed) = config.fixed_env3 {
            off_environment.push(fixed);
            on_environment.push(fixed);
        }
        let mut direct_off = engine(
            config.off_engine,
            "enabled-by-cli-flag",
            &["--jit"],
            &off_environment,
            "process launched with --jit and explicit A/B switch",
        );
        direct_off.executable = "/quench-node".into();
        direct_off.executable_sha256 = "d".repeat(64);
        direct_off.version = "Quench JIT test".into();
        direct_off.jit_proof_command = vec!["/quench-node".into(), "--jit".into()];
        let mut direct_on = direct_off.clone();
        direct_on.name = config.on_engine.into();
        direct_on.environment = on_environment
            .iter()
            .map(|(key, value)| ((*key).into(), (*value).into()))
            .collect();
        direct_on.jit_proof = "process launched with --jit and explicit A/B switch".into();
        let bun = engine(
            "bun",
            "enabled-and-probed",
            &[],
            &[("BUN_JSC_useJIT", "1")],
            "reported useJIT=true",
        );
        let node = engine(
            "node",
            "enabled-and-probed",
            &[],
            &[],
            "completed compiling hot function",
        );
        let fixture_path = format!("quench-bench/jetstream/{}", config.fixture_name);
        let engines = super::diagnostic_ab_engines(*config);
        let mut rounds = Vec::new();
        for round in 0..config.minimum_rounds {
            let execution_order = engines
                .iter()
                .cycle()
                .skip(round % engines.len())
                .take(engines.len())
                .map(|name| (*name).to_string())
                .collect::<Vec<_>>();
            let samples = engines
                .iter()
                .map(|name| {
                    let (score, rss) = if *name == config.off_engine {
                        (100.0, 100)
                    } else if *name == config.on_engine {
                        (110.0, 90)
                    } else {
                        match *name {
                            "bun" => (100.0, 120),
                            "node" => (90.0, 110),
                            _ => unreachable!(),
                        }
                    };
                    (
                        (*name).into(),
                        SampleInput {
                            status: 0,
                            timed_out: false,
                            wall_ns: 1_000,
                            peak_rss_bytes: Some(rss),
                            score: Some(score),
                            stdout: format!(
                                "Running {}\n{} Score {score} pts\n",
                                config.fixture_name, config.fixture_name
                            ),
                        },
                    )
                })
                .collect();
            rounds.push(RoundInput {
                round,
                execution_order,
                samples,
            });
        }
        let fixture = FixtureInput {
            source: ArtifactInput {
                path: fixture_path.clone(),
                size_bytes: Some(1),
                modified_unix_ns: None,
                sha256: Some("e".repeat(64)),
            },
            valid: true,
            output_equal: true,
            unsupported_engines: BTreeMap::new(),
            rounds,
        };
        SuiteInput {
            schema: crate::SUITE_REPORT_SCHEMA,
            suite: crate::Suite::JetStream3,
            jit_mode: crate::JitMode::Jit,
            jit_diagnostics: false,
            jit_wasm_root: None,
            jit_wasm_root_only: false,
            jit_wasm_root_first: false,
            jit_wasm_selective: false,
            jit_wasm_memcpy_ab: config.mode == DiagnosticAbMode::DirectMemcpy,
            jit_wasm_memcpy_raw_ab: config.mode == DiagnosticAbMode::RawI32Memcpy,
            jit_wasm_memcpy_early_ab: config.mode == DiagnosticAbMode::EarlyImportMemcpy,
            jit_wasm_compare_branch_ab: config.mode == DiagnosticAbMode::DirectCompareBranches,
            jit_wasm_affine_pair_ab: config.mode == DiagnosticAbMode::AffinePair,
            jit_wasm_reference_body_cache_ab: config.mode == DiagnosticAbMode::ReferenceBodyCache,
            jit_wasm_memcpy_leaf_ab: config.mode == DiagnosticAbMode::NativeMemcpyLeaf,
            jit_wasm_selective_ab: config.mode == DiagnosticAbMode::SelectiveHandoff,
            rounds_requested: config.minimum_rounds,
            timeout_ms: 1_000,
            source_revision: "a".repeat(40),
            source_dirty: false,
            host: HostInput {
                uname: "Linux".into(),
                rustc: "rustc test".into(),
                model: None,
                memory_bytes: None,
                memory_limit_bytes: None,
                cpu_quota: None,
                process_metrics_backend: "test".into(),
            },
            corpus: CorpusInput {
                pinned_revision: Some("b".repeat(40)),
                checkout_revision: Some("b".repeat(40)),
                checkout_clean: true,
                fixture_set_matches: false,
            },
            engines: vec![direct_off, direct_on, bun, node],
            suite_inputs: vec![ArtifactInput {
                path: "quench-bench/jetstream/cli.js".into(),
                size_bytes: Some(1),
                modified_unix_ns: None,
                sha256: Some("c".repeat(64)),
            }],
            fixtures: BTreeMap::from([(fixture_path, fixture)]),
            complete: true,
            qualification_ready: false,
        }
    }

    #[test]
    fn median_handles_odd_and_even_samples() {
        assert_eq!(median(&[9.0, 1.0, 5.0]), 5.0);
        assert_eq!(median(&[9.0, 1.0, 5.0, 3.0]), 4.0);
    }

    #[test]
    fn paired_bootstrap_preserves_a_constant_ratio() {
        let reference = (0..11).map(f64::from).collect::<Vec<_>>();
        let quench = reference
            .iter()
            .map(|value| value * 1.75)
            .collect::<Vec<_>>();
        let interval = paired_bootstrap_log_ratio_interval(&quench, &reference, 2_000, 0xabc);
        assert!((interval[0] - 1.75).abs() < 1e-12);
        assert!((interval[1] - 1.75).abs() < 1e-12);
    }

    #[test]
    fn paired_log_ratio_uses_matched_rounds_not_a_ratio_of_medians() {
        let quench = [2.0, 20.0, 30.0];
        let reference = [1.0, 2.0, 10.0];
        assert!((median_log_ratio(&quench, &reference).exp() - 3.0).abs() < 1e-12);
        assert_ne!(median(&quench) / median(&reference), 3.0);
    }

    #[test]
    fn analyzer_rejects_root_only_reports_even_if_they_claim_qualification() {
        let input = SuiteInput {
            schema: crate::SUITE_REPORT_SCHEMA,
            suite: Suite::JetStream3,
            jit_mode: JitMode::Jit,
            jit_diagnostics: false,
            jit_wasm_root: None,
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
            rounds_requested: 11,
            timeout_ms: 1_000,
            source_revision: "test".into(),
            source_dirty: false,
            host: HostInput {
                uname: "Linux".into(),
                rustc: "rustc test".into(),
                model: None,
                memory_bytes: None,
                memory_limit_bytes: None,
                cpu_quota: None,
                process_metrics_backend: "test".into(),
            },
            corpus: CorpusInput {
                pinned_revision: None,
                checkout_revision: None,
                checkout_clean: true,
                fixture_set_matches: true,
            },
            engines: Vec::new(),
            suite_inputs: Vec::new(),
            fixtures: BTreeMap::new(),
            complete: true,
            qualification_ready: true,
        };

        assert_eq!(
            validate_suite(&input).unwrap_err(),
            "root-only JIT reports cannot be used for qualification analysis"
        );
    }

    #[test]
    fn analyzer_rejects_root_first_reports_even_if_they_claim_qualification() {
        let input = SuiteInput {
            schema: crate::SUITE_REPORT_SCHEMA,
            suite: Suite::JetStream3,
            jit_mode: JitMode::Jit,
            jit_diagnostics: false,
            jit_wasm_root: None,
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
            rounds_requested: 11,
            timeout_ms: 1_000,
            source_revision: "test".into(),
            source_dirty: false,
            host: HostInput {
                uname: "Linux".into(),
                rustc: "rustc test".into(),
                model: None,
                memory_bytes: None,
                memory_limit_bytes: None,
                cpu_quota: None,
                process_metrics_backend: "test".into(),
            },
            corpus: CorpusInput {
                pinned_revision: None,
                checkout_revision: None,
                checkout_clean: true,
                fixture_set_matches: true,
            },
            engines: Vec::new(),
            suite_inputs: Vec::new(),
            fixtures: BTreeMap::new(),
            complete: true,
            qualification_ready: true,
        };

        assert_eq!(
            validate_suite(&input).unwrap_err(),
            "root-first JIT reports cannot be used for qualification analysis"
        );
    }

    #[test]
    fn analyzer_rejects_memcpy_ab_reports_even_if_they_claim_qualification() {
        let input = SuiteInput {
            schema: crate::SUITE_REPORT_SCHEMA,
            suite: Suite::JetStream3,
            jit_mode: JitMode::Jit,
            jit_diagnostics: false,
            jit_wasm_root: None,
            jit_wasm_root_only: false,
            jit_wasm_root_first: false,
            jit_wasm_selective: false,
            jit_wasm_memcpy_ab: true,
            jit_wasm_memcpy_raw_ab: false,
            jit_wasm_memcpy_early_ab: false,
            jit_wasm_compare_branch_ab: false,
            jit_wasm_affine_pair_ab: false,
            jit_wasm_reference_body_cache_ab: false,
            jit_wasm_memcpy_leaf_ab: false,
            jit_wasm_selective_ab: false,
            rounds_requested: 11,
            timeout_ms: 1_000,
            source_revision: "test".into(),
            source_dirty: false,
            host: HostInput {
                uname: "Linux".into(),
                rustc: "rustc test".into(),
                model: None,
                memory_bytes: None,
                memory_limit_bytes: None,
                cpu_quota: None,
                process_metrics_backend: "test".into(),
            },
            corpus: CorpusInput {
                pinned_revision: None,
                checkout_revision: None,
                checkout_clean: true,
                fixture_set_matches: true,
            },
            engines: Vec::new(),
            suite_inputs: Vec::new(),
            fixtures: BTreeMap::new(),
            complete: true,
            qualification_ready: true,
        };

        assert_eq!(
            validate_suite(&input).unwrap_err(),
            "Wasm JIT A/B diagnostic reports cannot be used for qualification analysis"
        );
    }

    #[test]
    fn memcpy_ab_analysis_validates_provenance_and_reports_paired_nonqualifying_ratios() {
        let input = memcpy_ab_input();
        validate_diagnostic_ab(&input, MEMCPY_DIRECT_CONFIG)
            .expect("four-engine GCC-Wasm diagnostic provenance");
        let fixture = input
            .fixtures
            .values()
            .next()
            .expect("one GCC-Wasm fixture");
        let analysis = analyze_diagnostic_ab_fixture(
            "quench-bench/jetstream/gcc-loops-wasm",
            fixture,
            11,
            MEMCPY_DIRECT_CONFIG,
        )
        .expect("paired diagnostic analysis");

        assert!(!analysis.qualified);
        assert_eq!(analysis.paired_rounds, 11);
        assert_eq!(analysis.comparisons.len(), 3);
        let on_vs_off = analysis
            .comparisons
            .get("quench_memcpy_direct_on_vs_quench_memcpy_direct_off")
            .expect("direct path on/off comparison");
        assert_eq!(on_vs_off.candidate_engine, MEMCPY_DIRECT_ON);
        assert_eq!(on_vs_off.reference_engine, MEMCPY_DIRECT_OFF);
        assert!((on_vs_off.score.median_ratio - 1.1).abs() < 1e-12);
        assert!((on_vs_off.max_rss.median_ratio - 0.9).abs() < 1e-12);
        assert!(
            analysis
                .comparisons
                .contains_key("quench_memcpy_direct_on_vs_bun")
        );
        assert!(
            analysis
                .comparisons
                .contains_key("quench_memcpy_direct_on_vs_node")
        );

        let mut invalid_provenance = memcpy_ab_input();
        invalid_provenance
            .engines
            .iter_mut()
            .find(|engine| engine.name == MEMCPY_DIRECT_ON)
            .unwrap()
            .environment
            .insert(MEMCPY_DIRECT_ENV.into(), "0".into());
        assert!(validate_diagnostic_ab(&invalid_provenance, MEMCPY_DIRECT_CONFIG).is_err());

        let mut mismatched_output = memcpy_ab_input();
        mismatched_output
            .fixtures
            .values_mut()
            .next()
            .unwrap()
            .rounds[0]
            .samples
            .get_mut("bun")
            .unwrap()
            .stdout = "different semantic output\n".into();
        assert!(
            analyze_diagnostic_ab_fixture(
                "quench-bench/jetstream/gcc-loops-wasm",
                mismatched_output.fixtures.values().next().unwrap(),
                11,
                MEMCPY_DIRECT_CONFIG,
            )
            .is_err()
        );
    }

    #[test]
    fn raw_i32_ab_keeps_direct_copy_fixed_and_reports_paired_nonqualifying_ratios() {
        let input = memcpy_ab_input_for(&MEMCPY_RAW_CONFIG);
        validate_diagnostic_ab(&input, MEMCPY_RAW_CONFIG)
            .expect("raw-i32 report keeps the direct path fixed");
        assert!(validate_suite(&input).is_err());
        let fixture = input
            .fixtures
            .values()
            .next()
            .expect("one GCC-Wasm fixture");
        let analysis = analyze_diagnostic_ab_fixture(
            "quench-bench/jetstream/gcc-loops-wasm",
            fixture,
            11,
            MEMCPY_RAW_CONFIG,
        )
        .expect("paired raw-i32 analysis");

        assert!(!analysis.qualified);
        assert_eq!(analysis.paired_rounds, 11);
        let on_vs_off = analysis
            .comparisons
            .get("quench_memcpy_raw_i32_on_vs_quench_memcpy_raw_i32_off")
            .expect("raw-i32 on/off comparison");
        assert!((on_vs_off.score.median_ratio - 1.1).abs() < 1e-12);
        assert!((on_vs_off.max_rss.median_ratio - 0.9).abs() < 1e-12);

        let mut invalid_provenance = memcpy_ab_input_for(&MEMCPY_RAW_CONFIG);
        let quench_off = invalid_provenance
            .engines
            .iter_mut()
            .find(|engine| engine.name == MEMCPY_RAW_CONFIG.off_engine)
            .unwrap();
        quench_off
            .environment
            .insert(MEMCPY_DIRECT_ENV.into(), "0".into());
        assert!(validate_diagnostic_ab(&invalid_provenance, MEMCPY_RAW_CONFIG).is_err());

        let config =
            selected_diagnostic_ab_config(false, true, false, false, false, false, false, false)
                .expect("raw mode selected");
        assert_eq!(config.mode, DiagnosticAbMode::RawI32Memcpy);
        assert!(
            selected_diagnostic_ab_config(true, true, false, false, false, false, false, false)
                .is_err()
        );
        assert_eq!(MEMCPY_RAW_ENV, "QUENCH_WASM_EMSCRIPTEN_MEMCPY_RAW_I32");
    }

    #[test]
    fn early_import_ab_keeps_both_fast_paths_fixed_and_is_never_qualifying() {
        let input = memcpy_ab_input_for(&MEMCPY_EARLY_CONFIG);
        validate_diagnostic_ab(&input, MEMCPY_EARLY_CONFIG)
            .expect("early-import report pins direct-copy and raw-i32 paths");
        assert!(validate_suite(&input).is_err());
        let mut too_few_rounds = memcpy_ab_input_for(&MEMCPY_EARLY_CONFIG);
        too_few_rounds.rounds_requested = 2;
        assert!(validate_diagnostic_ab(&too_few_rounds, MEMCPY_EARLY_CONFIG).is_err());
        let fixture = input
            .fixtures
            .values()
            .next()
            .expect("one GCC-Wasm fixture");
        let analysis = analyze_diagnostic_ab_fixture(
            "quench-bench/jetstream/gcc-loops-wasm",
            fixture,
            3,
            MEMCPY_EARLY_CONFIG,
        )
        .expect("paired early-import analysis");

        assert!(!analysis.qualified);
        assert_eq!(analysis.paired_rounds, 3);
        let on_vs_off = analysis
            .comparisons
            .get("quench_memcpy_early_import_on_vs_quench_memcpy_early_import_off")
            .expect("early-import on/off comparison");
        assert!((on_vs_off.score.median_ratio - 1.1).abs() < 1e-12);
        assert!((on_vs_off.max_rss.median_ratio - 0.9).abs() < 1e-12);

        let mut invalid_provenance = memcpy_ab_input_for(&MEMCPY_EARLY_CONFIG);
        invalid_provenance
            .engines
            .iter_mut()
            .find(|engine| engine.name == MEMCPY_EARLY_ON)
            .unwrap()
            .environment
            .insert(MEMCPY_RAW_ENV.into(), "0".into());
        assert!(validate_diagnostic_ab(&invalid_provenance, MEMCPY_EARLY_CONFIG).is_err());

        let config =
            selected_diagnostic_ab_config(false, false, false, true, false, false, false, false)
                .expect("early-import mode selected");
        assert_eq!(config.mode, DiagnosticAbMode::EarlyImportMemcpy);
        assert_eq!(config.off_engine, MEMCPY_EARLY_OFF);
        assert_eq!(config.on_engine, MEMCPY_EARLY_ON);
        assert_eq!(config.minimum_rounds, 3);
        assert_eq!(
            MEMCPY_EARLY_ENV,
            "QUENCH_WASM_EMSCRIPTEN_MEMCPY_EARLY_IMPORT"
        );
        assert!(
            selected_diagnostic_ab_config(false, true, false, true, false, false, false, false)
                .is_err()
        );
    }

    #[test]
    fn compare_branch_ab_accepts_three_rounds_and_reports_paired_nonqualifying_ratios() {
        let input = memcpy_ab_input_for(&DIRECT_COMPARE_BRANCHES_CONFIG);
        validate_diagnostic_ab(&input, DIRECT_COMPARE_BRANCHES_CONFIG)
            .expect("three-round quicksort direct-branch provenance");
        assert!(validate_suite(&input).is_err());
        let fixture = input
            .fixtures
            .values()
            .next()
            .expect("one QuickSort fixture");
        let analysis = analyze_diagnostic_ab_fixture(
            "quench-bench/jetstream/quicksort-wasm",
            fixture,
            3,
            DIRECT_COMPARE_BRANCHES_CONFIG,
        )
        .expect("paired compare-branch screen analysis");

        assert!(!analysis.qualified);
        assert_eq!(analysis.paired_rounds, 3);
        let on_vs_off = analysis
            .comparisons
            .get("quench_direct_compare_branches_on_vs_quench_direct_compare_branches_off")
            .expect("direct compare-branches on/off comparison");
        assert!((on_vs_off.score.median_ratio - 1.1).abs() < 1e-12);
        assert!((on_vs_off.max_rss.median_ratio - 0.9).abs() < 1e-12);
        assert!(
            analysis
                .comparisons
                .contains_key("quench_direct_compare_branches_on_vs_bun")
        );
        assert!(
            analysis
                .comparisons
                .contains_key("quench_direct_compare_branches_on_vs_node")
        );

        let mut invalid_switch = memcpy_ab_input_for(&DIRECT_COMPARE_BRANCHES_CONFIG);
        invalid_switch
            .engines
            .iter_mut()
            .find(|engine| engine.name == DIRECT_COMPARE_BRANCHES_ON)
            .unwrap()
            .environment
            .insert(DIRECT_COMPARE_BRANCHES_ENV.into(), "0".into());
        assert!(validate_diagnostic_ab(&invalid_switch, DIRECT_COMPARE_BRANCHES_CONFIG).is_err());

        let mut mismatched_output = memcpy_ab_input_for(&DIRECT_COMPARE_BRANCHES_CONFIG);
        mismatched_output
            .fixtures
            .values_mut()
            .next()
            .unwrap()
            .rounds[0]
            .samples
            .get_mut("bun")
            .unwrap()
            .stdout = "different QuickSort output\n".into();
        assert!(
            analyze_diagnostic_ab_fixture(
                "quench-bench/jetstream/quicksort-wasm",
                mismatched_output.fixtures.values().next().unwrap(),
                3,
                DIRECT_COMPARE_BRANCHES_CONFIG,
            )
            .is_err()
        );

        let selected =
            selected_diagnostic_ab_config(false, false, true, false, false, false, false, false)
                .expect("compare-branch mode selected");
        assert_eq!(
            selected.report_kind,
            "jit_wasm_compare_branch_ab_diagnostic"
        );
        assert_eq!(selected.minimum_rounds, 3);
        assert_eq!(selected.off_engine, DIRECT_COMPARE_BRANCHES_OFF);
        assert_eq!(selected.on_engine, DIRECT_COMPARE_BRANCHES_ON);
        assert!(
            selected_diagnostic_ab_config(true, false, true, false, false, false, false, false)
                .is_err()
        );

        let mut different_binary = memcpy_ab_input_for(&DIRECT_COMPARE_BRANCHES_CONFIG);
        different_binary
            .engines
            .iter_mut()
            .find(|engine| engine.name == DIRECT_COMPARE_BRANCHES_ON)
            .unwrap()
            .executable = "/different-quench-node".into();
        assert!(validate_diagnostic_ab(&different_binary, DIRECT_COMPARE_BRANCHES_CONFIG).is_err());
    }

    #[test]
    fn affine_pair_ab_accepts_three_round_screen_and_reports_nonqualifying_ratios() {
        let input = memcpy_ab_input_for(&AFFINE_PAIR_CONFIG);
        validate_diagnostic_ab(&input, AFFINE_PAIR_CONFIG)
            .expect("three-round GCC-Wasm affine-pair provenance");
        assert!(validate_suite(&input).is_err());
        let fixture = input
            .fixtures
            .values()
            .next()
            .expect("one GCC-Wasm fixture");
        let analysis = analyze_diagnostic_ab_fixture(
            "quench-bench/jetstream/gcc-loops-wasm",
            fixture,
            3,
            AFFINE_PAIR_CONFIG,
        )
        .expect("paired affine-pair screen analysis");

        assert!(!analysis.qualified);
        assert_eq!(analysis.paired_rounds, 3);
        let on_vs_off = analysis
            .comparisons
            .get("quench_affine_pair_on_vs_quench_affine_pair_off")
            .expect("affine-pair on/off comparison");
        assert!((on_vs_off.score.median_ratio - 1.1).abs() < 1e-12);
        assert!((on_vs_off.max_rss.median_ratio - 0.9).abs() < 1e-12);
        assert!(
            analysis
                .comparisons
                .contains_key("quench_affine_pair_on_vs_bun")
        );
        assert!(
            analysis
                .comparisons
                .contains_key("quench_affine_pair_on_vs_node")
        );

        let selected =
            selected_diagnostic_ab_config(false, false, false, false, true, false, false, false)
                .expect("affine-pair mode selected");
        assert_eq!(selected.report_kind, "jit_wasm_affine_pair_ab_diagnostic");
        assert_eq!(selected.minimum_rounds, 3);
        assert_eq!(selected.off_engine, AFFINE_PAIR_OFF);
        assert_eq!(selected.on_engine, AFFINE_PAIR_ON);
        assert_eq!(AFFINE_PAIR_ENV, "QUENCH_WASM_JIT_AFFINE_PAIR");
        assert!(
            selected_diagnostic_ab_config(true, false, false, false, true, false, false, false)
                .is_err()
        );

        let mut invalid_switch = memcpy_ab_input_for(&AFFINE_PAIR_CONFIG);
        invalid_switch
            .engines
            .iter_mut()
            .find(|engine| engine.name == AFFINE_PAIR_ON)
            .unwrap()
            .environment
            .insert(AFFINE_PAIR_ENV.into(), "0".into());
        assert!(validate_diagnostic_ab(&invalid_switch, AFFINE_PAIR_CONFIG).is_err());
    }

    #[test]
    fn reference_body_cache_ab_accepts_tsf_screen_and_reports_nonqualifying_ratios() {
        let input = memcpy_ab_input_for(&REFERENCE_BODY_CACHE_CONFIG);
        validate_diagnostic_ab(&input, REFERENCE_BODY_CACHE_CONFIG)
            .expect("three-round TSF-Wasm reference-cache provenance");
        assert!(validate_suite(&input).is_err());
        let fixture = input
            .fixtures
            .values()
            .next()
            .expect("one TSF-Wasm fixture");
        let analysis = analyze_diagnostic_ab_fixture(
            "quench-bench/jetstream/tsf-wasm",
            fixture,
            3,
            REFERENCE_BODY_CACHE_CONFIG,
        )
        .expect("paired reference-body screen analysis");

        assert!(!analysis.qualified);
        assert_eq!(analysis.paired_rounds, 3);
        let on_vs_off = analysis
            .comparisons
            .get("quench_reference_body_cache_on_vs_quench_reference_body_cache_off")
            .expect("reference-body cache on/off comparison");
        assert!((on_vs_off.score.median_ratio - 1.1).abs() < 1e-12);
        assert!((on_vs_off.max_rss.median_ratio - 0.9).abs() < 1e-12);
        assert!(
            analysis
                .comparisons
                .contains_key("quench_reference_body_cache_on_vs_bun")
        );
        assert!(
            analysis
                .comparisons
                .contains_key("quench_reference_body_cache_on_vs_node")
        );

        let selected =
            selected_diagnostic_ab_config(false, false, false, false, false, true, false, false)
                .expect("reference-body cache mode selected");
        assert_eq!(
            selected.report_kind,
            "jit_wasm_reference_body_cache_ab_diagnostic"
        );
        assert_eq!(selected.minimum_rounds, 3);
        assert_eq!(selected.off_engine, REFERENCE_BODY_CACHE_OFF);
        assert_eq!(selected.on_engine, REFERENCE_BODY_CACHE_ON);
        assert_eq!(
            REFERENCE_BODY_CACHE_ENV,
            "QUENCH_WASM_JIT_REFERENCE_BODY_CACHE"
        );
        assert!(
            selected_diagnostic_ab_config(true, false, false, false, false, true, false, false)
                .is_err()
        );

        let mut invalid_switch = memcpy_ab_input_for(&REFERENCE_BODY_CACHE_CONFIG);
        invalid_switch
            .engines
            .iter_mut()
            .find(|engine| engine.name == REFERENCE_BODY_CACHE_ON)
            .unwrap()
            .environment
            .insert(REFERENCE_BODY_CACHE_ENV.into(), "0".into());
        assert!(validate_diagnostic_ab(&invalid_switch, REFERENCE_BODY_CACHE_CONFIG).is_err());
    }

    #[test]
    fn native_memcpy_leaf_ab_accepts_gcc_screen_and_reports_nonqualifying_ratios() {
        let input = memcpy_ab_input_for(&MEMCPY_LEAF_CONFIG);
        validate_diagnostic_ab(&input, MEMCPY_LEAF_CONFIG)
            .expect("three-round GCC-Wasm native-leaf provenance");
        assert!(validate_suite(&input).is_err());
        let fixture = input
            .fixtures
            .values()
            .next()
            .expect("one GCC-Wasm fixture");
        let analysis = analyze_diagnostic_ab_fixture(
            "quench-bench/jetstream/gcc-loops-wasm",
            fixture,
            3,
            MEMCPY_LEAF_CONFIG,
        )
        .expect("paired native-leaf diagnostic analysis");

        assert!(!analysis.qualified);
        assert_eq!(analysis.paired_rounds, 3);
        let on_vs_off = analysis
            .comparisons
            .get("quench_memcpy_native_leaf_on_vs_quench_memcpy_native_leaf_off")
            .expect("native-leaf on/off comparison");
        assert!((on_vs_off.score.median_ratio - 1.1).abs() < 1e-12);
        assert!((on_vs_off.max_rss.median_ratio - 0.9).abs() < 1e-12);
        assert!(
            analysis
                .comparisons
                .contains_key("quench_memcpy_native_leaf_on_vs_bun")
        );
        assert!(
            analysis
                .comparisons
                .contains_key("quench_memcpy_native_leaf_on_vs_node")
        );

        let selected =
            selected_diagnostic_ab_config(false, false, false, false, false, false, true, false)
                .expect("native memcpy-leaf A/B mode selected");
        assert_eq!(
            selected.report_kind,
            "jit_wasm_memcpy_native_leaf_ab_diagnostic"
        );
        assert_eq!(selected.minimum_rounds, 3);
        assert_eq!(selected.off_engine, MEMCPY_LEAF_OFF);
        assert_eq!(selected.on_engine, MEMCPY_LEAF_ON);
        assert_eq!(MEMCPY_LEAF_ENV, "QUENCH_WASM_JIT_EMSCRIPTEN_MEMCPY_LEAF");
        assert!(
            selected_diagnostic_ab_config(true, false, false, false, false, false, true, false)
                .is_err()
        );

        let mut invalid_switch = memcpy_ab_input_for(&MEMCPY_LEAF_CONFIG);
        invalid_switch
            .engines
            .iter_mut()
            .find(|engine| engine.name == MEMCPY_LEAF_ON)
            .unwrap()
            .environment
            .insert(MEMCPY_LEAF_ENV.into(), "0".into());
        assert!(validate_diagnostic_ab(&invalid_switch, MEMCPY_LEAF_CONFIG).is_err());

        let mut missing_early_import = memcpy_ab_input_for(&MEMCPY_LEAF_CONFIG);
        missing_early_import
            .engines
            .iter_mut()
            .find(|engine| engine.name == MEMCPY_LEAF_OFF)
            .unwrap()
            .environment
            .remove(MEMCPY_EARLY_ENV);
        assert!(validate_diagnostic_ab(&missing_early_import, MEMCPY_LEAF_CONFIG).is_err());

        let mut mismatched_raw_i32 = memcpy_ab_input_for(&MEMCPY_LEAF_CONFIG);
        mismatched_raw_i32
            .engines
            .iter_mut()
            .find(|engine| engine.name == MEMCPY_LEAF_ON)
            .unwrap()
            .environment
            .insert(MEMCPY_RAW_ENV.into(), "0".into());
        assert!(validate_diagnostic_ab(&mismatched_raw_i32, MEMCPY_LEAF_CONFIG).is_err());
    }

    #[test]
    fn selective_handoff_ab_accepts_gcc_screen_and_is_never_qualifying() {
        let input = memcpy_ab_input_for(&SELECTIVE_HANDOFF_CONFIG);
        validate_diagnostic_ab(&input, SELECTIVE_HANDOFF_CONFIG)
            .expect("three-round GCC-Wasm selective-handoff provenance");
        assert!(validate_suite(&input).is_err());
        let fixture = input
            .fixtures
            .values()
            .next()
            .expect("one GCC-Wasm fixture");
        let analysis = analyze_diagnostic_ab_fixture(
            "quench-bench/jetstream/gcc-loops-wasm",
            fixture,
            3,
            SELECTIVE_HANDOFF_CONFIG,
        )
        .expect("paired selective-handoff diagnostic analysis");

        assert!(!analysis.qualified);
        assert_eq!(analysis.paired_rounds, 3);
        assert!(
            analysis
                .comparisons
                .contains_key("quench_selective_handoff_on_vs_quench_selective_handoff_off")
        );
        assert!(
            analysis
                .comparisons
                .contains_key("quench_selective_handoff_on_vs_bun")
        );
        assert!(
            analysis
                .comparisons
                .contains_key("quench_selective_handoff_on_vs_node")
        );

        let selected =
            selected_diagnostic_ab_config(false, false, false, false, false, false, false, true)
                .expect("selective-handoff A/B mode selected");
        assert_eq!(
            selected.report_kind,
            "jit_wasm_selective_handoff_ab_diagnostic"
        );
        assert_eq!(selected.minimum_rounds, 3);
        assert_eq!(selected.off_engine, SELECTIVE_HANDOFF_OFF);
        assert_eq!(selected.on_engine, SELECTIVE_HANDOFF_ON);
        assert_eq!(SELECTIVE_HANDOFF_ENV, "QUENCH_WASM_JIT_SELECTIVE");
        assert!(
            selected_diagnostic_ab_config(false, false, false, false, false, false, true, true,)
                .is_err()
        );

        let mut invalid_switch = memcpy_ab_input_for(&SELECTIVE_HANDOFF_CONFIG);
        invalid_switch
            .engines
            .iter_mut()
            .find(|engine| engine.name == SELECTIVE_HANDOFF_ON)
            .unwrap()
            .environment
            .insert(SELECTIVE_HANDOFF_ENV.into(), "0".into());
        assert!(validate_diagnostic_ab(&invalid_switch, SELECTIVE_HANDOFF_CONFIG).is_err());
    }

    #[test]
    fn jetstream_jit_engines_require_enabled_mode_proofs() {
        let engines = [
            engine(
                "quench",
                "enabled-by-cli-flag",
                &["--jit"],
                &[],
                "process launched with --jit",
            ),
            engine(
                "bun",
                "enabled-and-probed",
                &[],
                &[("BUN_JSC_useJIT", "1")],
                "reported useJIT=true",
            ),
            engine(
                "node",
                "enabled-and-probed",
                &[],
                &[],
                "completed compiling hot function",
            ),
        ];
        for engine in &engines {
            validate_engine(engine, Suite::JetStream3, JitMode::Jit).unwrap();
        }

        let no_jit = engine("node", "disabled-and-probed", &["--jitless"], &[], "true");
        assert!(validate_engine(&no_jit, Suite::JetStream3, JitMode::Jit).is_err());
    }
}
