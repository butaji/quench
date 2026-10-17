use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use crate::{now_ns, sha256};

const MIN_QUALIFYING_ROUNDS: usize = 11;
const BOOTSTRAP_REPLICATES: usize = 100_000;
const BOOTSTRAP_INTERVAL_LEVEL: f64 = 0.95;
const BOOTSTRAP_BASE_SEED: u64 = 0x51A7_BEEF_CAFE_2C31;
const QUENCH: &str = "quench";

#[derive(Deserialize)]
struct SuiteInput {
    schema: u32,
    #[serde(default)]
    suite: crate::Suite,
    #[serde(default)]
    jit_mode: crate::JitMode,
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

#[derive(Deserialize, Serialize)]
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

#[derive(Deserialize, Serialize)]
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

#[derive(Deserialize, Serialize)]
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
    validate_suite(&input)?;

    let input_report_sha256 = sha256(&input_path)
        .ok_or_else(|| format!("cannot hash {} with shasum", input_path.display()))?;
    let binary_path = std::env::current_exe()
        .map_err(|error| format!("cannot resolve analyzer executable: {error}"))?;
    let quench_bench_binary = artifact_input(&binary_path)?;
    let mut fixtures = BTreeMap::new();
    let engines = input.suite.engine_names();
    for name in input.suite.fixture_names() {
        let (fixture_path, fixture) = find_fixture(&input.fixtures, name)?;
        let measured = engines
            .iter()
            .copied()
            .filter(|engine| !fixture.unsupported_engines.contains_key(*engine))
            .collect::<Vec<_>>();
        fixtures.insert(
            (*name).to_string(),
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
        "qualification: passed all {} fixtures and {comparison_count} fixture-engine comparisons for {:?}",
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

fn validate_suite(input: &SuiteInput) -> Result<(), String> {
    if input.schema != 5 && input.schema != crate::SUITE_REPORT_SCHEMA {
        return Err(format!(
            "unsupported benchmark report schema {}",
            input.schema
        ));
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
        || !input.corpus.fixture_set_matches
        || input.corpus.pinned_revision != input.corpus.checkout_revision
    {
        return Err(
            "benchmark corpus is dirty, unpinned, or has a fixture-set mismatch".to_string(),
        );
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
    if input.fixtures.len() != input.suite.fixture_names().len() {
        return Err(format!(
            "benchmark report does not contain exactly the {} required fixtures",
            input.suite.fixture_names().len()
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

fn find_fixture<'a>(
    fixtures: &'a BTreeMap<String, FixtureInput>,
    expected_name: &str,
) -> Result<(&'a str, &'a FixtureInput), String> {
    let mut matches = fixtures.iter().filter(|(path, _)| {
        Path::new(path.as_str())
            .file_name()
            .is_some_and(|name| name == expected_name)
    });
    let first = matches
        .next()
        .ok_or_else(|| format!("benchmark report is missing {expected_name}"))?;
    if matches.next().is_some() {
        return Err(format!(
            "benchmark report has duplicate {expected_name} fixtures"
        ));
    }
    Ok((first.0.as_str(), first.1))
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
        EngineInput, median, median_log_ratio, paired_bootstrap_log_ratio_interval, validate_engine,
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
