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
const ENGINES: &[&str] = &["quench", "quickjs", "bun", "node"];
const COMPARATORS: &[&str] = &["quickjs", "bun", "node"];

#[derive(Deserialize)]
struct SuiteInput {
    schema: u32,
    rounds_requested: usize,
    timeout_ms: u64,
    source_revision: String,
    source_dirty: bool,
    host: HostInput,
    corpus: CorpusInput,
    engines: Vec<EngineInput>,
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
    rounds: Vec<RoundInput>,
}

#[derive(Deserialize)]
struct ArtifactInput {
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
    input_report_sha256: String,
    source_revision: String,
    source_dirty: bool,
    corpus_revision: String,
    timeout_ms: u64,
    host: HostInput,
    engine_provenance: Vec<EngineInput>,
    rounds_per_fixture: usize,
    analysis_runtime: String,
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
    median_delta: f64,
    paired_interval_95: [f64; 2],
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
    let mut fixtures = BTreeMap::new();
    for name in crate::FIXTURES {
        let (fixture_path, fixture) = find_fixture(&input.fixtures, name)?;
        fixtures.insert(
            (*name).to_string(),
            analyze_fixture(fixture_path, fixture, input.rounds_requested)?,
        );
    }
    let qualified = fixtures.values().all(|fixture| fixture.qualified);
    let corpus_revision = input
        .corpus
        .pinned_revision
        .clone()
        .ok_or_else(|| "benchmark corpus has no pinned revision".to_string())?;
    let engine_provenance = ENGINES
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
        schema: 1,
        created_unix_ns: now_ns(),
        input_report_sha256,
        source_revision: input.source_revision,
        source_dirty: input.source_dirty,
        corpus_revision,
        timeout_ms: input.timeout_ms,
        host,
        engine_provenance,
        rounds_per_fixture: input.rounds_requested,
        analysis_runtime,
        bootstrap: BootstrapMethod {
            name: "paired percentile bootstrap of the difference between medians",
            interval_level: BOOTSTRAP_INTERVAL_LEVEL,
            replicates: BOOTSTRAP_REPLICATES,
            base_seed: format!("0x{BOOTSTRAP_BASE_SEED:016x}"),
            resampling: "resample round indices jointly for Quench and comparator",
            statistic: "median(Quench) - median(comparator)",
            interval: "linear-interpolated 2.5th and 97.5th percentiles",
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
    eprintln!("qualification: passed for all eight fixtures and three comparators");
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
    if input.schema != 4 {
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
    let expected = ENGINES.iter().copied().collect::<BTreeSet<_>>();
    if engines != expected || input.engines.len() != ENGINES.len() {
        return Err(
            "benchmark report does not contain exactly the four required engines".to_string(),
        );
    }
    for engine in &input.engines {
        validate_engine(engine)?;
    }
    if input.fixtures.len() != crate::FIXTURES.len() {
        return Err(
            "benchmark report does not contain exactly the eight required fixtures".to_string(),
        );
    }
    if input.source_revision.len() != 40 || !is_hex(&input.source_revision) {
        return Err("benchmark report has an invalid source revision".to_string());
    }
    Ok(())
}

fn validate_engine(engine: &EngineInput) -> Result<(), String> {
    if engine.executable.is_empty()
        || engine.version.trim().is_empty()
        || engine.executable_sha256.len() != 64
        || !is_hex(&engine.executable_sha256)
        || engine.jit_proof_command.is_empty()
        || engine.inherits_environment
    {
        return Err(format!("{} is missing executable provenance", engine.name));
    }
    let proof_valid = match engine.name.as_str() {
        "quench" => {
            engine.jit_mode == "interpreter" && engine.jit_proof.contains("no guest JIT path")
        }
        "quickjs" => engine.jit_mode == "interpreter" && engine.jit_proof.contains("interpreter"),
        "bun" => {
            engine.jit_mode == "disabled-and-probed"
                && engine
                    .environment
                    .get("BUN_JSC_useJIT")
                    .is_some_and(|v| v == "0")
                && engine.jit_proof.contains("useJIT=false")
        }
        "node" => {
            engine.jit_mode == "disabled-and-probed"
                && engine.argv.iter().any(|arg| arg == "--jitless")
                && engine.jit_proof.contains("true")
        }
        _ => false,
    };
    if !proof_valid {
        return Err(format!(
            "{} lacks valid no-JIT/interpreter proof",
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

fn analyze_fixture(
    path: &str,
    fixture: &FixtureInput,
    requested_rounds: usize,
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

    let mut samples = ENGINES
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
        let expected_order = ENGINES
            .iter()
            .cycle()
            .skip(index % ENGINES.len())
            .take(ENGINES.len())
            .copied()
            .map(str::to_string)
            .collect::<Vec<_>>();
        if round.execution_order != expected_order {
            return Err(format!(
                "{path} round {index} did not use the recorded rotating order"
            ));
        }
        if round.samples.len() != ENGINES.len() {
            return Err(format!("{path} round {index} is missing an engine sample"));
        }
        let mut raw_samples = BTreeMap::new();
        for engine in ENGINES {
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
    let quench = samples.get("quench").expect("quench samples");
    let mut comparisons = BTreeMap::new();
    for comparator in COMPARATORS {
        let reference = samples.get(*comparator).expect("comparator samples");
        let score = analyze_metric(path, comparator, "score", &quench.scores, &reference.scores);
        let max_rss = analyze_metric(
            path,
            comparator,
            "max_rss",
            &quench.rss_bytes,
            &reference.rss_bytes,
        );
        let qualified = score.median_delta > 0.0
            && score.paired_interval_95[0] > 0.0
            && max_rss.median_delta < 0.0
            && max_rss.paired_interval_95[1] < 0.0;
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
    let median_delta = quench_median - comparator_median;
    let seed = derive_seed(path, comparator, metric);
    let paired_interval_95 =
        paired_bootstrap_interval(quench, reference, BOOTSTRAP_REPLICATES, seed);
    MetricAnalysis {
        quench_median,
        comparator_median,
        median_delta,
        interval_excludes_tie: paired_interval_95[0] > 0.0 || paired_interval_95[1] < 0.0,
        paired_interval_95,
        seed: format!("0x{seed:016x}"),
    }
}

fn paired_bootstrap_interval(
    quench: &[f64],
    reference: &[f64],
    replicates: usize,
    seed: u64,
) -> [f64; 2] {
    let mut random = XorShift64::new(seed);
    let mut quench_resample = vec![0.0; quench.len()];
    let mut reference_resample = vec![0.0; reference.len()];
    let mut differences = Vec::with_capacity(replicates);
    for _ in 0..replicates {
        for index in 0..quench.len() {
            let round = random.index(quench.len());
            quench_resample[index] = quench[round];
            reference_resample[index] = reference[round];
        }
        differences.push(median(&quench_resample) - median(&reference_resample));
    }
    differences.sort_by(f64::total_cmp);
    let tail = (1.0 - BOOTSTRAP_INTERVAL_LEVEL) / 2.0;
    [
        quantile(&differences, tail),
        quantile(&differences, 1.0 - tail),
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
    use super::{median, paired_bootstrap_interval};

    #[test]
    fn median_handles_odd_and_even_samples() {
        assert_eq!(median(&[9.0, 1.0, 5.0]), 5.0);
        assert_eq!(median(&[9.0, 1.0, 5.0, 3.0]), 4.0);
    }

    #[test]
    fn paired_bootstrap_preserves_a_constant_positive_effect() {
        let reference = (0..11).map(f64::from).collect::<Vec<_>>();
        let quench = reference
            .iter()
            .map(|value| value + 7.0)
            .collect::<Vec<_>>();
        let interval = paired_bootstrap_interval(&quench, &reference, 2_000, 0xabc);
        assert_eq!(interval, [7.0, 7.0]);
    }
}
