use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

const CHECKPOINT_SCHEMA: u32 = 1;
const FIXED_WORK_REPORT_SCHEMA: u64 = 4;
const MIN_CLEAN_PAIRS: usize = 7;
const TOP_UP_ROUNDS: usize = 7;
const MAX_TOP_UP_BATCHES_PER_FIXTURE: usize = 2;
const MIN_INSTRUCTION_FALLBACK_PAIRS: usize = 11;
const INSTRUCTION_AA_BAND_PERCENT: f64 = 0.14;
const MAX_RSS_INCREASE_PERCENT: f64 = 0.5;
const BOOTSTRAP_REPLICATES: usize = 20_000;
const BOOTSTRAP_INTERVAL_PERCENT: f64 = 95.0;
const BOOTSTRAP_SEED: u64 = 0x61_F1_7E_D5_2026_1010;
const BASELINE_ENGINE: &str = "quench_baseline";
const CANDIDATE_ENGINE: &str = "quench_candidate";

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ChangeKind {
    SemanticNeutralMetadataOrDispatch,
    Layout,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum BatchState {
    Planned,
    Complete,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct FileIdentity {
    path: PathBuf,
    sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct EngineIdentity {
    path: PathBuf,
    sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct TopUpBatch {
    fixture: String,
    ordinal: usize,
    rounds: usize,
    report: PathBuf,
    state: BatchState,
    report_sha256: Option<String>,
    runner_exit_code: Option<i32>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Checkpoint {
    schema: u32,
    created_unix_ns: u128,
    updated_unix_ns: u128,
    change_kind: ChangeKind,
    minimum_clean_pairs: usize,
    top_up_rounds: usize,
    maximum_top_up_batches_per_fixture: usize,
    instruction_aa_band_percent: f64,
    maximum_rss_increase_percent: f64,
    measurement_runner: FileIdentity,
    baseline: EngineIdentity,
    candidate: EngineIdentity,
    timeout_ms: u64,
    source_revision: String,
    source_dirty: bool,
    corpus: Value,
    suite_inputs: Value,
    initial_report: FileIdentity,
    top_up_batches: Vec<TopUpBatch>,
}

#[derive(Serialize)]
struct DecisionReport<'a> {
    schema: u32,
    checkpoint: &'a Checkpoint,
    reports: Vec<FileIdentity>,
    fixtures: BTreeMap<String, FixtureDecision>,
    pending_fixtures: Vec<String>,
    all_fixtures_classified: bool,
    overall: String,
}

#[derive(Serialize)]
struct FixtureDecision {
    clean_pairs: usize,
    valid_pairs: usize,
    cycles: MetricDecision,
    instructions: MetricDecision,
    max_rss: MetricDecision,
    classification: String,
}

#[derive(Serialize)]
struct MetricDecision {
    median_percent: Option<f64>,
    interval_95_percent: Option<[f64; 2]>,
    samples: usize,
    status: String,
}

#[derive(Clone, Copy)]
struct Pair {
    clean: bool,
    cycle_percent: Option<f64>,
    instruction_percent: Option<f64>,
    rss_percent: Option<f64>,
}

enum CommandLine {
    Start {
        report: PathBuf,
        checkpoint: PathBuf,
        change_kind: ChangeKind,
    },
    Resume {
        checkpoint: PathBuf,
    },
    Status {
        checkpoint: PathBuf,
    },
}

fn main() {
    if let Err(error) = run(parse_args()) {
        eprintln!("task61-fixed-work-guard: {error}");
        std::process::exit(2);
    }
}

fn run(command: CommandLine) -> Result<(), String> {
    match command {
        CommandLine::Start {
            report,
            checkpoint,
            change_kind,
        } => {
            if checkpoint.exists() {
                return Err(format!(
                    "checkpoint already exists: {}; use resume",
                    checkpoint.display()
                ));
            }
            let initial = read_json(&report)?;
            validate_report(&initial, None)?;
            let mut state = checkpoint_from_report(&report, &initial, change_kind)?;
            state.created_unix_ns = now_ns();
            state.updated_unix_ns = state.created_unix_ns;
            let decision = make_decision(&state)?;
            write_checkpoint(&checkpoint, &state)?;
            write_decision(&checkpoint, &decision)?;
            println!("{}", serde_json::to_string_pretty(&decision).unwrap());
            Ok(())
        }
        CommandLine::Resume { checkpoint } => {
            let state = read_checkpoint(&checkpoint)?;
            validate_checkpoint(&state)?;
            drive(&checkpoint, state)
        }
        CommandLine::Status { checkpoint } => {
            let state = read_checkpoint(&checkpoint)?;
            validate_checkpoint(&state)?;
            let decision = make_decision(&state)?;
            write_decision(&checkpoint, &decision)?;
            println!("{}", serde_json::to_string_pretty(&decision).unwrap());
            Ok(())
        }
    }
}

fn drive(checkpoint_path: &Path, mut state: Checkpoint) -> Result<(), String> {
    validate_checkpoint(&state)?;
    recover_planned_batches(checkpoint_path, &mut state)?;
    for fixture in checkpoint_fixture_names(&state)? {
        let pairs = fixture_pairs(&state, &fixture)?;
        let decision = decide_fixture(&state, &pairs);
        let used_batches = state
            .top_up_batches
            .iter()
            .filter(|batch| batch.fixture == fixture)
            .count();
        if !fixture_needs_top_up(&decision)
            || used_batches >= state.maximum_top_up_batches_per_fixture
        {
            continue;
        }

        let ordinal = used_batches + 1;
        let output = top_up_report_path(checkpoint_path, &fixture, ordinal);
        let batch = TopUpBatch {
            fixture: fixture.clone(),
            ordinal,
            rounds: state.top_up_rounds,
            report: output,
            state: BatchState::Planned,
            report_sha256: None,
            runner_exit_code: None,
        };
        if let Some(parent) = batch.report.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("cannot create top-up directory: {error}"))?;
        }
        state.top_up_batches.push(batch);
        persist(checkpoint_path, &mut state)?;
        let batch_index = state.top_up_batches.len() - 1;
        execute_planned_batch(checkpoint_path, &mut state, batch_index)?;
    }

    persist(checkpoint_path, &mut state)?;
    let decision = make_decision(&state)?;
    write_decision(checkpoint_path, &decision)?;
    println!("{}", serde_json::to_string_pretty(&decision).unwrap());
    Ok(())
}

fn execute_planned_batch(
    checkpoint_path: &Path,
    state: &mut Checkpoint,
    batch_index: usize,
) -> Result<(), String> {
    let batch = state
        .top_up_batches
        .get(batch_index)
        .cloned()
        .ok_or_else(|| "invalid top-up batch index".to_string())?;
    if batch.state == BatchState::Complete {
        return Ok(());
    }
    if batch.report.exists() {
        return complete_batch(checkpoint_path, state, batch_index);
    }

    let fixture_path = fixture_source_path(state, &batch.fixture)?;
    let runner = &state.measurement_runner.path;
    let actual_runner_sha = sha256(runner)?;
    if actual_runner_sha != state.measurement_runner.sha256 {
        return Err(format!(
            "pinned measurement runner changed: expected {}, found {}",
            state.measurement_runner.sha256, actual_runner_sha
        ));
    }
    let status = Command::new(runner)
        .arg(&fixture_path)
        .args(["--fixed-work", "--runs"])
        .arg(batch.rounds.to_string())
        .args(["--timeout-ms"])
        .arg(state.timeout_ms.to_string())
        .args(["--quench"])
        .arg(&state.baseline.path)
        .args(["--quench-peer"])
        .arg(&state.candidate.path)
        .args(["--out"])
        .arg(&batch.report)
        .status()
        .map_err(|error| format!("cannot run pinned benchmark runner: {error}"))?;
    state.top_up_batches[batch_index].runner_exit_code = status.code();
    persist(checkpoint_path, state)?;
    if !batch.report.is_file() {
        return Err(format!(
            "runner exited {status} without writing {}",
            batch.report.display()
        ));
    }
    complete_batch(checkpoint_path, state, batch_index)
}

fn fixture_needs_top_up(decision: &FixtureDecision) -> bool {
    decision.classification == "inconclusive"
}

fn complete_batch(
    checkpoint_path: &Path,
    state: &mut Checkpoint,
    batch_index: usize,
) -> Result<(), String> {
    let batch = state
        .top_up_batches
        .get(batch_index)
        .cloned()
        .ok_or_else(|| "invalid top-up batch index".to_string())?;
    let report_sha256 = sha256(&batch.report)?;
    let report = read_json(&batch.report)?;
    validate_report(&report, Some(state))?;
    if required_u64(&report, "rounds_requested")? != batch.rounds as u64 {
        return Err(format!("top-up round count changed for {}", batch.fixture));
    }
    let fixture = fixture_record(&report, &batch.fixture)
        .ok_or_else(|| format!("top-up report omits {}", batch.fixture))?;
    if !fixture
        .get("output_equal")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Err(format!("top-up output mismatch for {}", batch.fixture));
    }
    let expected_source = fixture_source_path(state, &batch.fixture)?;
    let actual_source = fixture
        .get("source")
        .and_then(|source| source.get("path"))
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .ok_or_else(|| format!("top-up omits source artifact for {}", batch.fixture))?;
    if actual_source != expected_source {
        return Err(format!(
            "top-up fixture input changed for {}",
            batch.fixture
        ));
    }
    let batch = state
        .top_up_batches
        .get_mut(batch_index)
        .ok_or_else(|| "invalid top-up batch index".to_string())?;
    batch.report_sha256 = Some(report_sha256);
    batch.state = BatchState::Complete;
    persist(checkpoint_path, state)?;
    write_decision(checkpoint_path, &make_decision(state)?)
}

fn recover_planned_batches(checkpoint_path: &Path, state: &mut Checkpoint) -> Result<(), String> {
    for index in 0..state.top_up_batches.len() {
        if state.top_up_batches[index].state == BatchState::Planned
            && state.top_up_batches[index].report.exists()
        {
            complete_batch(checkpoint_path, state, index)?;
        }
    }
    Ok(())
}

fn checkpoint_from_report(
    report_path: &Path,
    report: &Value,
    change_kind: ChangeKind,
) -> Result<Checkpoint, String> {
    let runner = report
        .get("measurement_runner")
        .ok_or_else(|| "report is missing measurement_runner".to_string())?;
    let baseline = engine_identity(report, BASELINE_ENGINE)?;
    let candidate = engine_identity(report, CANDIDATE_ENGINE)?;
    let timeout_ms = required_u64(report, "timeout_ms")?;
    let initial_report = FileIdentity {
        path: report_path
            .canonicalize()
            .map_err(|error| format!("cannot resolve report {}: {error}", report_path.display()))?,
        sha256: sha256(report_path)?,
    };
    let checkpoint = Checkpoint {
        schema: CHECKPOINT_SCHEMA,
        created_unix_ns: 0,
        updated_unix_ns: 0,
        change_kind,
        minimum_clean_pairs: MIN_CLEAN_PAIRS,
        top_up_rounds: TOP_UP_ROUNDS,
        maximum_top_up_batches_per_fixture: MAX_TOP_UP_BATCHES_PER_FIXTURE,
        instruction_aa_band_percent: INSTRUCTION_AA_BAND_PERCENT,
        maximum_rss_increase_percent: MAX_RSS_INCREASE_PERCENT,
        measurement_runner: FileIdentity {
            path: required_path(runner, "path")?,
            sha256: required_string(runner, "sha256")?.to_string(),
        },
        baseline,
        candidate,
        timeout_ms,
        source_revision: required_string(report, "source_revision")?.to_string(),
        source_dirty: report
            .get("source_dirty")
            .and_then(Value::as_bool)
            .ok_or_else(|| "report is missing source_dirty".to_string())?,
        corpus: report
            .get("corpus")
            .cloned()
            .ok_or_else(|| "report is missing corpus identity".to_string())?,
        suite_inputs: report
            .get("suite_inputs")
            .cloned()
            .ok_or_else(|| "report is missing suite input identities".to_string())?,
        initial_report,
        top_up_batches: Vec::new(),
    };
    validate_checkpoint(&checkpoint)?;
    Ok(checkpoint)
}

fn make_decision(state: &Checkpoint) -> Result<DecisionReport<'_>, String> {
    validate_checkpoint(state)?;
    let reports = report_paths(state);
    let mut decisions = BTreeMap::new();
    for fixture in checkpoint_fixture_names(state)? {
        let pairs = fixture_pairs(state, &fixture)?;
        decisions.insert(fixture, decide_fixture(state, &pairs));
    }
    let pending_fixtures = decisions
        .iter()
        .filter(|(_, decision)| decision.classification == "inconclusive")
        .map(|(fixture, _)| fixture.clone())
        .collect::<Vec<_>>();
    let all_fixtures_classified = pending_fixtures.is_empty();
    let overall = if decisions.values().any(|fixture| {
        fixture.classification == "regression" || fixture.max_rss.status == "over_budget"
    }) {
        "rejected"
    } else if decisions.values().all(|fixture| {
        matches!(
            fixture.classification.as_str(),
            "cycles" | "instructions_only"
        ) && fixture.max_rss.status == "within_budget"
    }) {
        "passed"
    } else {
        "inconclusive"
    };
    Ok(DecisionReport {
        schema: CHECKPOINT_SCHEMA,
        checkpoint: state,
        reports,
        fixtures: decisions,
        pending_fixtures,
        all_fixtures_classified,
        overall: overall.to_string(),
    })
}

fn decide_fixture(state: &Checkpoint, pairs: &[Pair]) -> FixtureDecision {
    let clean_cycles = pairs
        .iter()
        .filter(|pair| pair.clean)
        .filter_map(|pair| pair.cycle_percent)
        .collect::<Vec<_>>();
    let valid_instructions = pairs
        .iter()
        .filter_map(|pair| pair.instruction_percent)
        .collect::<Vec<_>>();
    let valid_rss = pairs
        .iter()
        .filter_map(|pair| pair.rss_percent)
        .collect::<Vec<_>>();

    let mut cycles = metric_decision(&clean_cycles, None, "no_clean_pairs", true);
    if clean_cycles.len() < state.minimum_clean_pairs {
        cycles.status = "insufficient_clean_pairs".to_string();
    }
    let instructions = metric_decision(
        &valid_instructions,
        Some(state.instruction_aa_band_percent),
        "insufficient_instruction_pairs",
        false,
    );
    let rss = metric_decision(
        &valid_rss,
        Some(state.maximum_rss_increase_percent),
        "insufficient_rss_pairs",
        false,
    );

    let classification = if clean_cycles.len() >= state.minimum_clean_pairs {
        if cycles.status == "regression" {
            "regression"
        } else {
            "cycles"
        }
    } else if state.change_kind == ChangeKind::SemanticNeutralMetadataOrDispatch
        && valid_instructions.len() >= MIN_INSTRUCTION_FALLBACK_PAIRS
        && instructions
            .median_percent
            .is_some_and(|median| median <= state.instruction_aa_band_percent)
    {
        "instructions_only"
    } else {
        "inconclusive"
    };

    FixtureDecision {
        clean_pairs: pairs.iter().filter(|pair| pair.clean).count(),
        valid_pairs: pairs
            .iter()
            .filter(|pair| pair.cycle_percent.is_some())
            .count(),
        cycles,
        instructions,
        max_rss: rss,
        classification: classification.to_string(),
    }
}

fn metric_decision(
    values: &[f64],
    upper_limit: Option<f64>,
    empty_status: &str,
    include_paired_interval: bool,
) -> MetricDecision {
    let interval = include_paired_interval
        .then(|| bootstrap_interval(values))
        .flatten();
    let median = median(values);
    let status = match (median, interval, upper_limit) {
        (None, _, _) => empty_status.to_string(),
        (Some(value), _, Some(limit)) if value > limit => "over_budget".to_string(),
        (Some(_), Some(bounds), Some(limit)) if bounds[0] > limit => "over_budget".to_string(),
        (Some(_), Some(bounds), Some(limit)) if bounds[1] <= limit => "within_budget".to_string(),
        (Some(value), _, Some(limit)) if value <= limit => "within_budget".to_string(),
        (Some(_), Some(bounds), None) if bounds[0] > 0.0 => "regression".to_string(),
        (Some(_), _, None) => "no_detected_regression".to_string(),
        _ => "inconclusive".to_string(),
    };
    MetricDecision {
        median_percent: median,
        interval_95_percent: interval,
        samples: values.len(),
        status,
    }
}

fn fixture_pairs(state: &Checkpoint, fixture_name: &str) -> Result<Vec<Pair>, String> {
    let mut pairs = Vec::new();
    for report_path in report_paths(state) {
        let report = read_json(&report_path.path)?;
        if report_path.path == state.initial_report.path {
            validate_report(&report, None)?;
            validate_report_provenance(&report, state)?;
        } else {
            validate_report(&report, Some(state))?;
        }
        let Some(fixture) = fixture_record(&report, fixture_name) else {
            continue;
        };
        if !fixture
            .get("output_equal")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return Err(format!("engine output mismatch for {fixture_name}"));
        }
        let rounds = fixture
            .get("rounds")
            .and_then(Value::as_array)
            .ok_or_else(|| format!("report has no rounds for {fixture_name}"))?;
        for round in rounds {
            if let Some(pair) = pair_from_round(round)? {
                pairs.push(pair);
            }
        }
    }
    Ok(pairs)
}

fn pair_from_round(round: &Value) -> Result<Option<Pair>, String> {
    let samples = round
        .get("samples")
        .ok_or_else(|| "round is missing samples".to_string())?;
    let setup = round
        .get("setup_only_samples")
        .ok_or_else(|| "round is missing setup_only_samples".to_string())?;
    let base_work = samples.get(BASELINE_ENGINE);
    let candidate_work = samples.get(CANDIDATE_ENGINE);
    let base_setup = setup.get(BASELINE_ENGINE);
    let candidate_setup = setup.get(CANDIDATE_ENGINE);
    let (Some(base_work), Some(candidate_work), Some(base_setup), Some(candidate_setup)) =
        (base_work, candidate_work, base_setup, candidate_setup)
    else {
        return Ok(None);
    };
    if ![base_work, candidate_work, base_setup, candidate_setup]
        .iter()
        .all(|sample| sample_valid(sample))
    {
        return Ok(None);
    }
    let clean = [base_work, candidate_work, base_setup, candidate_setup]
        .iter()
        .all(|sample| sample_clean(sample));
    let cycles = relative_marginal_percent(
        base_work,
        candidate_work,
        base_setup,
        candidate_setup,
        "cycles",
    );
    let instructions = relative_marginal_percent(
        base_work,
        candidate_work,
        base_setup,
        candidate_setup,
        "instructions",
    );
    let baseline_rss = sample_metric(base_work, "peak_rss_bytes");
    let candidate_rss = sample_metric(candidate_work, "peak_rss_bytes");
    let rss_percent = baseline_rss
        .zip(candidate_rss)
        .filter(|(base, _)| *base > 0.0)
        .map(|(base, candidate)| 100.0 * (candidate / base - 1.0));
    if cycles.is_none() && instructions.is_none() && rss_percent.is_none() {
        return Ok(None);
    }
    Ok(Some(Pair {
        clean,
        cycle_percent: cycles,
        instruction_percent: instructions,
        rss_percent,
    }))
}

fn relative_marginal_percent(
    baseline_work: &Value,
    candidate_work: &Value,
    baseline_setup: &Value,
    candidate_setup: &Value,
    field: &str,
) -> Option<f64> {
    let baseline_work = sample_metric(baseline_work, field)?;
    let candidate_work = sample_metric(candidate_work, field)?;
    let baseline_setup = sample_metric(baseline_setup, field)?;
    let candidate_setup = sample_metric(candidate_setup, field)?;
    let baseline_marginal = baseline_work - baseline_setup;
    let candidate_marginal = candidate_work - candidate_setup;
    (baseline_marginal > 0.0).then(|| 100.0 * (candidate_marginal / baseline_marginal - 1.0))
}

fn sample_metric(sample: &Value, field: &str) -> Option<f64> {
    sample
        .get(field)?
        .as_f64()
        .filter(|value| value.is_finite())
}

fn sample_valid(sample: &Value) -> bool {
    sample.get("status").and_then(Value::as_i64) == Some(0)
        && sample.get("timed_out").and_then(Value::as_bool) == Some(false)
        && sample_metric(sample, "peak_rss_bytes").is_some_and(|rss| rss > 0.0)
        && sample_metric(sample, "cycles").is_some()
        && sample_metric(sample, "instructions").is_some()
}

fn sample_clean(sample: &Value) -> bool {
    sample
        .get("contention")
        .and_then(|contention| contention.get("clean"))
        .and_then(Value::as_bool)
        == Some(true)
        && sample
            .get("contention")
            .and_then(|contention| contention.get("checks_complete"))
            .and_then(Value::as_bool)
            == Some(true)
}

fn validate_report(report: &Value, expected: Option<&Checkpoint>) -> Result<(), String> {
    if required_u64(report, "schema")? != FIXED_WORK_REPORT_SCHEMA
        || required_string(report, "measurement_mode")? != "fixed_work_diagnostic"
    {
        return Err("expected a schema-4 fixed-work diagnostic report".into());
    }
    let fixtures = report
        .get("fixtures")
        .and_then(Value::as_object)
        .ok_or_else(|| "report has no fixture map".to_string())?;
    let fixture_names = fixtures
        .keys()
        .filter_map(|key| Path::new(key).file_name()?.to_str().map(str::to_string))
        .collect::<BTreeSet<_>>();
    if fixture_names.len() != fixtures.len() {
        return Err("report contains an unknown fixture key".into());
    }
    let expected_fixtures = suite_fixture_names(report)?;
    if expected.is_none() && fixture_names != expected_fixtures {
        return Err("initial report must contain exactly the eight V8-v7 fixtures".into());
    }
    if expected.is_some()
        && (fixtures.len() != 1
            || fixture_names
                .iter()
                .any(|fixture| !expected_fixtures.contains(fixture)))
    {
        return Err("a top-up report must contain exactly one V8-v7 fixture".into());
    }
    let engines = report
        .get("engines")
        .and_then(Value::as_array)
        .ok_or_else(|| "report has no engine list".to_string())?;
    for engine_name in [BASELINE_ENGINE, CANDIDATE_ENGINE] {
        if !engines
            .iter()
            .any(|engine| engine.get("name").and_then(Value::as_str) == Some(engine_name))
        {
            return Err(format!("report omits engine {engine_name}"));
        }
    }
    if let Some(expected) = expected {
        validate_report_provenance(report, expected)?;
    }
    for fixture in fixtures.values() {
        if fixture.get("output_equal").and_then(Value::as_bool) != Some(true) {
            return Err("fixed-work report has an output mismatch".into());
        }
    }
    Ok(())
}

fn validate_checkpoint(state: &Checkpoint) -> Result<(), String> {
    if state.schema != CHECKPOINT_SCHEMA
        || state.minimum_clean_pairs != MIN_CLEAN_PAIRS
        || state.top_up_rounds != TOP_UP_ROUNDS
        || state.maximum_top_up_batches_per_fixture != MAX_TOP_UP_BATCHES_PER_FIXTURE
        || (state.instruction_aa_band_percent - INSTRUCTION_AA_BAND_PERCENT).abs() > f64::EPSILON
        || (state.maximum_rss_increase_percent - MAX_RSS_INCREASE_PERCENT).abs() > f64::EPSILON
    {
        return Err("checkpoint policy differs from this tool's named guard policy".into());
    }
    for (path, expected_sha) in [
        (&state.initial_report.path, &state.initial_report.sha256),
        (
            &state.measurement_runner.path,
            &state.measurement_runner.sha256,
        ),
        (&state.baseline.path, &state.baseline.sha256),
        (&state.candidate.path, &state.candidate.sha256),
    ] {
        let actual = sha256(path)?;
        if &actual != expected_sha {
            return Err(format!("pinned artifact changed: {}", path.display()));
        }
    }
    let initial = read_json(&state.initial_report.path)?;
    validate_report(&initial, None)?;
    validate_report_provenance(&initial, state)?;
    for batch in &state.top_up_batches {
        if !checkpoint_fixture_names(state)?.contains(&batch.fixture)
            || batch.ordinal == 0
            || batch.ordinal > state.maximum_top_up_batches_per_fixture
            || batch.rounds != state.top_up_rounds
        {
            return Err("checkpoint contains an invalid top-up record".into());
        }
        if batch.state == BatchState::Complete {
            let expected_sha = batch
                .report_sha256
                .as_ref()
                .ok_or_else(|| "completed top-up has no report hash".to_string())?;
            if sha256(&batch.report)? != *expected_sha {
                return Err(format!("top-up report changed: {}", batch.report.display()));
            }
            validate_report(&read_json(&batch.report)?, Some(state))?;
        }
    }
    Ok(())
}

fn validate_report_provenance(report: &Value, expected: &Checkpoint) -> Result<(), String> {
    let runner = report
        .get("measurement_runner")
        .ok_or_else(|| "report omits measurement runner identity".to_string())?;
    if required_string(runner, "sha256")? != expected.measurement_runner.sha256 {
        return Err("measurement runner SHA changed between report batches".into());
    }
    let engines = report
        .get("engines")
        .and_then(Value::as_array)
        .ok_or_else(|| "report has no engine list".to_string())?;
    for (name, expected_identity) in [
        (BASELINE_ENGINE, &expected.baseline),
        (CANDIDATE_ENGINE, &expected.candidate),
    ] {
        let actual = engines
            .iter()
            .find(|engine| engine.get("name").and_then(Value::as_str) == Some(name))
            .ok_or_else(|| format!("report omits engine {name}"))?;
        if required_string(actual, "executable_sha256")? != expected_identity.sha256
            || PathBuf::from(required_string(actual, "executable")?) != expected_identity.path
        {
            return Err(format!(
                "pinned {name} executable changed between report batches"
            ));
        }
    }
    if required_string(report, "source_revision")? != expected.source_revision
        || report.get("source_dirty").and_then(Value::as_bool) != Some(expected.source_dirty)
        || report.get("corpus") != Some(&expected.corpus)
        || report.get("suite_inputs") != Some(&expected.suite_inputs)
    {
        return Err("source or corpus provenance changed between report batches".into());
    }
    Ok(())
}

fn engine_identity(report: &Value, name: &str) -> Result<EngineIdentity, String> {
    let engines = report
        .get("engines")
        .and_then(Value::as_array)
        .ok_or_else(|| "report has no engine list".to_string())?;
    let engine = engines
        .iter()
        .find(|engine| engine.get("name").and_then(Value::as_str) == Some(name))
        .ok_or_else(|| format!("report omits {name}"))?;
    Ok(EngineIdentity {
        path: PathBuf::from(required_string(engine, "executable")?),
        sha256: required_string(engine, "executable_sha256")?.to_string(),
    })
}

fn fixture_source_path(state: &Checkpoint, fixture_name: &str) -> Result<PathBuf, String> {
    let report = read_json(&state.initial_report.path)?;
    fixture_record(&report, fixture_name)
        .and_then(|fixture| fixture.get("source"))
        .and_then(|source| source.get("path"))
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .ok_or_else(|| format!("initial report omits source path for {fixture_name}"))
}

fn checkpoint_fixture_names(state: &Checkpoint) -> Result<Vec<String>, String> {
    let report = read_json(&state.initial_report.path)?;
    Ok(suite_fixture_names(&report)?.into_iter().collect())
}

fn suite_fixture_names(report: &Value) -> Result<BTreeSet<String>, String> {
    if report
        .get("corpus")
        .and_then(|corpus| corpus.get("fixture_set_matches"))
        .and_then(Value::as_bool)
        != Some(true)
    {
        return Err("report does not cover the pinned V8-v7 fixture set".into());
    }
    let suite_inputs = report
        .get("suite_inputs")
        .and_then(Value::as_array)
        .ok_or_else(|| "report has no suite input list".to_string())?;
    let mut fixtures = BTreeSet::new();
    for input in suite_inputs {
        let path = required_string(input, "path")?;
        let Some(name) = Path::new(path).file_name().and_then(|name| name.to_str()) else {
            return Err(format!("invalid suite input path: {path}"));
        };
        if name != "base.js" {
            fixtures.insert(name.to_string());
        }
    }
    if fixtures.is_empty() {
        return Err("suite input list has no benchmark fixtures".into());
    }
    Ok(fixtures)
}

fn fixture_record<'a>(report: &'a Value, fixture_name: &str) -> Option<&'a Value> {
    report
        .get("fixtures")?
        .as_object()?
        .iter()
        .find(|(key, _)| {
            Path::new(key).file_name().and_then(|name| name.to_str()) == Some(fixture_name)
        })
        .map(|(_, fixture)| fixture)
}

fn report_paths(state: &Checkpoint) -> Vec<FileIdentity> {
    std::iter::once(state.initial_report.clone())
        .chain(state.top_up_batches.iter().filter_map(|batch| {
            (batch.state == BatchState::Complete).then(|| FileIdentity {
                path: batch.report.clone(),
                sha256: batch.report_sha256.clone().unwrap_or_default(),
            })
        }))
        .collect()
}

fn top_up_report_path(checkpoint: &Path, fixture: &str, ordinal: usize) -> PathBuf {
    let parent = checkpoint.parent().unwrap_or_else(|| Path::new("."));
    let stem = checkpoint
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("task61-gate");
    parent
        .join(format!("{stem}-topups"))
        .join(format!("{fixture}-batch-{ordinal}.json"))
}

fn persist(path: &Path, state: &mut Checkpoint) -> Result<(), String> {
    state.updated_unix_ns = now_ns();
    write_checkpoint(path, state)
}

fn write_decision(checkpoint_path: &Path, decision: &DecisionReport<'_>) -> Result<(), String> {
    let path = checkpoint_path.with_extension("decision.json");
    let temporary = path.with_extension("decision.tmp");
    fs::write(&temporary, serde_json::to_vec_pretty(decision).unwrap())
        .map_err(|error| format!("cannot write decision {}: {error}", temporary.display()))?;
    fs::rename(&temporary, &path)
        .map_err(|error| format!("cannot replace decision {}: {error}", path.display()))
}

fn write_checkpoint(path: &Path, state: &Checkpoint) -> Result<(), String> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create checkpoint directory: {error}"))?;
    }
    let temporary = path.with_extension("checkpoint.tmp");
    fs::write(&temporary, serde_json::to_vec_pretty(state).unwrap())
        .map_err(|error| format!("cannot write {}: {error}", temporary.display()))?;
    fs::rename(&temporary, path)
        .map_err(|error| format!("cannot replace checkpoint {}: {error}", path.display()))
}

fn read_checkpoint(path: &Path) -> Result<Checkpoint, String> {
    let bytes =
        fs::read(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|error| format!("invalid checkpoint JSON: {error}"))
}

fn read_json(path: &Path) -> Result<Value, String> {
    let bytes =
        fs::read(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("invalid JSON in {}: {error}", path.display()))
}

fn required_string<'a>(value: &'a Value, field: &str) -> Result<&'a str, String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing string field {field}"))
}

fn required_path(value: &Value, field: &str) -> Result<PathBuf, String> {
    Ok(PathBuf::from(required_string(value, field)?))
}

fn required_u64(value: &Value, field: &str) -> Result<u64, String> {
    value
        .get(field)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("missing integer field {field}"))
}

fn sha256(path: &Path) -> Result<String, String> {
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
        })
        .ok_or_else(|| format!("cannot hash {}", path.display()))?;
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .next()
        .map(str::to_string)
        .ok_or_else(|| format!("hash output is empty for {}", path.display()))
}

fn median(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut ordered = values.to_vec();
    ordered.sort_by(f64::total_cmp);
    let middle = ordered.len() / 2;
    Some(if ordered.len() % 2 == 0 {
        (ordered[middle - 1] + ordered[middle]) / 2.0
    } else {
        ordered[middle]
    })
}

fn bootstrap_interval(values: &[f64]) -> Option<[f64; 2]> {
    if values.is_empty() {
        return None;
    }
    let lower_quantile = (100.0 - BOOTSTRAP_INTERVAL_PERCENT) / 200.0;
    let upper_quantile = 1.0 - lower_quantile;
    let lower = (lower_quantile * (BOOTSTRAP_REPLICATES - 1) as f64).round() as usize;
    let upper = (upper_quantile * (BOOTSTRAP_REPLICATES - 1) as f64).round() as usize;
    let mut random = BOOTSTRAP_SEED ^ values.len() as u64;
    let mut samples = Vec::with_capacity(BOOTSTRAP_REPLICATES);
    let mut draw = Vec::with_capacity(values.len());
    for _ in 0..BOOTSTRAP_REPLICATES {
        draw.clear();
        for _ in 0..values.len() {
            draw.push(values[next_random(&mut random) as usize % values.len()]);
        }
        samples.push(median(&draw).unwrap());
    }
    samples.sort_by(f64::total_cmp);
    Some([samples[lower], samples[upper]])
}

fn next_random(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

fn now_ns() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos())
}

fn parse_args() -> CommandLine {
    let mut args = env::args().skip(1);
    let command = args.next().unwrap_or_default();
    let mut report = None;
    let mut checkpoint = None;
    let mut change_kind = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--initial-report" => report = Some(required_arg(&mut args, "--initial-report")),
            "--checkpoint" => checkpoint = Some(required_arg(&mut args, "--checkpoint")),
            "--change-kind" => {
                change_kind = Some(match required_arg(&mut args, "--change-kind").as_str() {
                    "semantic-neutral-metadata-or-dispatch" => {
                        ChangeKind::SemanticNeutralMetadataOrDispatch
                    }
                    "layout" => ChangeKind::Layout,
                    value => usage(&format!("unknown change kind: {value}")),
                })
            }
            "--help" | "-h" => usage(""),
            _ => usage(&format!("unknown argument: {arg}")),
        }
    }
    match command.as_str() {
        "start" => CommandLine::Start {
            report: report
                .unwrap_or_else(|| usage("start requires --initial-report"))
                .into(),
            checkpoint: checkpoint
                .unwrap_or_else(|| usage("start requires --checkpoint"))
                .into(),
            change_kind: change_kind.unwrap_or_else(|| usage("start requires --change-kind")),
        },
        "resume" => CommandLine::Resume {
            checkpoint: checkpoint
                .unwrap_or_else(|| usage("resume requires --checkpoint"))
                .into(),
        },
        "status" => CommandLine::Status {
            checkpoint: checkpoint
                .unwrap_or_else(|| usage("status requires --checkpoint"))
                .into(),
        },
        _ => usage("expected start, resume, or status"),
    }
}

fn required_arg(args: &mut impl Iterator<Item = String>, option: &str) -> String {
    args.next()
        .unwrap_or_else(|| usage(&format!("missing value for {option}")))
}

fn usage(message: &str) -> ! {
    eprintln!(
        "{message}\nusage:\n  task61-fixed-work-guard start --initial-report REPORT --checkpoint STATE --change-kind semantic-neutral-metadata-or-dispatch|layout\n  task61-fixed-work-guard resume --checkpoint STATE  # run only needed fixture top-ups\n  task61-fixed-work-guard status --checkpoint STATE"
    );
    std::process::exit(2)
}

#[cfg(test)]
mod tests {
    use super::{
        bootstrap_interval, decide_fixture, fixture_needs_top_up, ChangeKind, Checkpoint, Pair,
    };

    fn state(change_kind: ChangeKind) -> Checkpoint {
        Checkpoint {
            schema: 1,
            created_unix_ns: 0,
            updated_unix_ns: 0,
            change_kind,
            minimum_clean_pairs: 7,
            top_up_rounds: 7,
            maximum_top_up_batches_per_fixture: 2,
            instruction_aa_band_percent: 0.14,
            maximum_rss_increase_percent: 0.5,
            measurement_runner: super::FileIdentity {
                path: "runner".into(),
                sha256: "runner-sha".into(),
            },
            baseline: super::EngineIdentity {
                path: "baseline".into(),
                sha256: "baseline-sha".into(),
            },
            candidate: super::EngineIdentity {
                path: "candidate".into(),
                sha256: "candidate-sha".into(),
            },
            timeout_ms: 300_000,
            source_revision: "revision".into(),
            source_dirty: true,
            corpus: serde_json::json!({"revision": "corpus"}),
            suite_inputs: serde_json::json!([]),
            initial_report: super::FileIdentity {
                path: "report".into(),
                sha256: "report-sha".into(),
            },
            top_up_batches: Vec::new(),
        }
    }

    #[test]
    fn semantic_neutral_metadata_can_use_instruction_fallback() {
        let candidate = state(ChangeKind::SemanticNeutralMetadataOrDispatch);
        let pairs = (0..11)
            .map(|_| Pair {
                clean: false,
                cycle_percent: Some(4.0),
                instruction_percent: Some(0.05),
                rss_percent: Some(0.1),
            })
            .collect::<Vec<_>>();
        let decision = decide_fixture(&candidate, &pairs);
        assert_eq!(decision.classification, "instructions_only");
        assert!(!fixture_needs_top_up(&decision));
    }

    #[test]
    fn layout_change_cannot_use_instruction_fallback() {
        let candidate = state(ChangeKind::Layout);
        let pairs = (0..11)
            .map(|_| Pair {
                clean: false,
                cycle_percent: Some(4.0),
                instruction_percent: Some(0.05),
                rss_percent: Some(0.1),
            })
            .collect::<Vec<_>>();
        let decision = decide_fixture(&candidate, &pairs);
        assert_eq!(decision.classification, "inconclusive");
        assert!(fixture_needs_top_up(&decision));
    }

    #[test]
    fn clean_pairs_drive_cycle_intervals_and_noisy_pairs_do_not() {
        let candidate = state(ChangeKind::Layout);
        let mut pairs = (0..7)
            .map(|_| Pair {
                clean: true,
                cycle_percent: Some(-1.0),
                instruction_percent: Some(-1.0),
                rss_percent: Some(0.1),
            })
            .collect::<Vec<_>>();
        pairs.push(Pair {
            clean: false,
            cycle_percent: Some(50.0),
            instruction_percent: Some(50.0),
            rss_percent: Some(50.0),
        });
        let decision = decide_fixture(&candidate, &pairs);
        assert_eq!(decision.cycles.samples, 7);
        assert_eq!(decision.classification, "cycles");
    }

    #[test]
    fn bootstrap_is_deterministic_and_brackets_a_constant_delta() {
        let values = [-1.0, -1.0, -1.0, -1.0, -1.0, -1.0, -1.0];
        assert_eq!(bootstrap_interval(&values), Some([-1.0, -1.0]));
    }
}
