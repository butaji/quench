use super::{
    EngineSpec, FixedWorkPlan, FixedWorkRound, FixedWorkRoundAttempt, FixedWorkSampleKind,
    HostRecord, Sample,
};
use serde::Serialize;
use std::collections::BTreeMap;

pub(super) const MAX_RETRIES: usize = 2;

const IPC_REFERENCE_PERCENTILE: usize = 75;
const MIN_IPC_REFERENCE_SAMPLES: usize = 5;
const MIN_IPC_REFERENCE_RATIO: f64 = 0.90;
const INVOLUNTARY_SWITCH_REFERENCE_PERCENTILE: usize = 25;
const MIN_INVOLUNTARY_SWITCH_REFERENCE_SAMPLES: usize = 5;
const MAX_INVOLUNTARY_SWITCH_RATE_RATIO: f64 = 4.0;
const NANOSECONDS_PER_SECOND: f64 = 1_000_000_000.0;
const MAX_LOAD_PER_LOGICAL_CPU: f64 = 1.0;

#[derive(Clone, Serialize)]
pub(super) struct Policy {
    ipc_reference_percentile: usize,
    min_ipc_reference_samples: usize,
    min_ipc_reference_ratio: f64,
    involuntary_switch_reference_percentile: usize,
    min_involuntary_switch_reference_samples: usize,
    max_involuntary_switch_rate_ratio: f64,
    max_load_per_logical_cpu: f64,
    max_retries: usize,
}

pub(super) const POLICY: Policy = Policy {
    ipc_reference_percentile: IPC_REFERENCE_PERCENTILE,
    min_ipc_reference_samples: MIN_IPC_REFERENCE_SAMPLES,
    min_ipc_reference_ratio: MIN_IPC_REFERENCE_RATIO,
    involuntary_switch_reference_percentile: INVOLUNTARY_SWITCH_REFERENCE_PERCENTILE,
    min_involuntary_switch_reference_samples: MIN_INVOLUNTARY_SWITCH_REFERENCE_SAMPLES,
    max_involuntary_switch_rate_ratio: MAX_INVOLUNTARY_SWITCH_RATE_RATIO,
    max_load_per_logical_cpu: MAX_LOAD_PER_LOGICAL_CPU,
    max_retries: MAX_RETRIES,
};

#[cfg(test)]
pub(super) fn verified_clean_assessment() -> Assessment {
    Assessment {
        clean: true,
        checks_complete: true,
        ipc_checked: true,
        involuntary_switch_rate_checked: true,
        load_checked: true,
        ipc: Some(1.0),
        reference_ipc: Some(1.0),
        ipc_reference_samples: Some(MIN_IPC_REFERENCE_SAMPLES),
        involuntary_switches_per_second: Some(1.0),
        reference_involuntary_switches_per_second: Some(1.0),
        involuntary_switch_reference_samples: Some(MIN_INVOLUNTARY_SWITCH_REFERENCE_SAMPLES),
        load_per_logical_cpu: Some(0.0),
        reasons: Vec::new(),
    }
}

#[derive(Clone, serde::Deserialize, Serialize)]
pub(super) struct Assessment {
    clean: bool,
    checks_complete: bool,
    ipc_checked: bool,
    involuntary_switch_rate_checked: bool,
    load_checked: bool,
    ipc: Option<f64>,
    reference_ipc: Option<f64>,
    ipc_reference_samples: Option<usize>,
    involuntary_switches_per_second: Option<f64>,
    reference_involuntary_switches_per_second: Option<f64>,
    involuntary_switch_reference_samples: Option<usize>,
    load_per_logical_cpu: Option<f64>,
    reasons: Vec<Reason>,
}

#[derive(Clone, serde::Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Reason {
    IpcBelowReference { ratio: f64, minimum_ratio: f64 },
    InvoluntarySwitchRateAboveReference { ratio: f64, maximum_ratio: f64 },
    LoadAtCapacity { load_per_logical_cpu: f64 },
}

#[derive(Default)]
pub(super) struct References {
    ipc: BTreeMap<(String, FixedWorkSampleKind), MetricReference>,
    involuntary_switch_rate: BTreeMap<(String, FixedWorkSampleKind), MetricReference>,
}

struct MetricReference {
    value: f64,
    samples: usize,
}

pub(super) fn assess_rounds(
    rounds: &mut [FixedWorkRound],
    engines: &[EngineSpec],
    plan: FixedWorkPlan,
    host: &HostRecord,
) -> References {
    let references = build_references(rounds, engines, plan);
    for round in rounds {
        annotate_maps(
            &mut round.samples,
            &mut round.setup_only_samples,
            engines,
            &references,
            host,
        );
    }
    references
}

pub(super) fn assess_attempt(
    attempt: &mut FixedWorkRoundAttempt,
    engines: &[EngineSpec],
    references: &References,
    host: &HostRecord,
) {
    annotate_maps(
        &mut attempt.samples,
        &mut attempt.setup_only_samples,
        engines,
        references,
        host,
    );
}

pub(super) fn round_is_clean(round: &FixedWorkRound) -> bool {
    maps_are_clean(&round.samples, &round.setup_only_samples)
}

pub(super) fn round_needs_retry(round: &FixedWorkRound) -> bool {
    maps_have_contention(&round.samples, &round.setup_only_samples)
}

pub(super) fn attempt_is_clean(attempt: &FixedWorkRoundAttempt) -> bool {
    maps_are_clean(&attempt.samples, &attempt.setup_only_samples)
}

pub(super) fn sample_is_clean(sample: &Sample) -> bool {
    sample
        .contention
        .as_ref()
        .is_some_and(|assessment| assessment.clean && assessment.checks_complete)
}

fn maps_are_clean(
    samples: &BTreeMap<String, Sample>,
    setup_only_samples: &BTreeMap<String, Sample>,
) -> bool {
    !samples.is_empty()
        && samples.len() == setup_only_samples.len()
        && samples.values().all(sample_is_clean)
        && setup_only_samples.values().all(sample_is_clean)
}

fn maps_have_contention(
    samples: &BTreeMap<String, Sample>,
    setup_only_samples: &BTreeMap<String, Sample>,
) -> bool {
    samples
        .values()
        .chain(setup_only_samples.values())
        .any(sample_has_contention)
}

fn sample_has_contention(sample: &Sample) -> bool {
    sample
        .contention
        .as_ref()
        .is_some_and(|assessment| !assessment.reasons.is_empty())
}

fn annotate_maps(
    samples: &mut BTreeMap<String, Sample>,
    setup_only_samples: &mut BTreeMap<String, Sample>,
    engines: &[EngineSpec],
    references: &References,
    host: &HostRecord,
) {
    for engine in engines {
        let cohort = (engine.executable_sha256.clone(), FixedWorkSampleKind::Work);
        if let Some(sample) = samples.get_mut(engine.name) {
            sample.contention = Some(assess_sample(
                sample,
                references,
                &cohort,
                host.logical_cpus,
            ));
        }
        let cohort = (
            engine.executable_sha256.clone(),
            FixedWorkSampleKind::SetupOnly,
        );
        if let Some(sample) = setup_only_samples.get_mut(engine.name) {
            sample.contention = Some(assess_sample(
                sample,
                references,
                &cohort,
                host.logical_cpus,
            ));
        }
    }
}

fn build_references(
    rounds: &[FixedWorkRound],
    engines: &[EngineSpec],
    plan: FixedWorkPlan,
) -> References {
    let mut ipc_values: BTreeMap<(String, FixedWorkSampleKind), Vec<f64>> = BTreeMap::new();
    let mut switch_rate_values: BTreeMap<(String, FixedWorkSampleKind), Vec<f64>> = BTreeMap::new();
    for round in rounds {
        for engine in engines {
            add_references(
                &mut ipc_values,
                &mut switch_rate_values,
                engine,
                FixedWorkSampleKind::Work,
                round.samples.get(engine.name),
                plan,
                plan.iterations_per_benchmark,
            );
            add_references(
                &mut ipc_values,
                &mut switch_rate_values,
                engine,
                FixedWorkSampleKind::SetupOnly,
                round.setup_only_samples.get(engine.name),
                plan,
                0,
            );
        }
    }
    References {
        ipc: percentile_references(
            ipc_values,
            MIN_IPC_REFERENCE_SAMPLES,
            IPC_REFERENCE_PERCENTILE,
        ),
        involuntary_switch_rate: percentile_references(
            switch_rate_values,
            MIN_INVOLUNTARY_SWITCH_REFERENCE_SAMPLES,
            INVOLUNTARY_SWITCH_REFERENCE_PERCENTILE,
        ),
    }
}

fn add_references(
    ipc_values: &mut BTreeMap<(String, FixedWorkSampleKind), Vec<f64>>,
    switch_rate_values: &mut BTreeMap<(String, FixedWorkSampleKind), Vec<f64>>,
    engine: &EngineSpec,
    kind: FixedWorkSampleKind,
    sample: Option<&Sample>,
    plan: FixedWorkPlan,
    iterations: usize,
) {
    let Some(sample) = sample.filter(|sample| sample.valid_fixed_work(plan, iterations)) else {
        return;
    };
    let cohort = (engine.executable_sha256.clone(), kind);
    if let Some(ipc) = sample_ipc(sample) {
        ipc_values.entry(cohort.clone()).or_default().push(ipc);
    }
    if let Some(rate) = sample_involuntary_switch_rate(sample) {
        switch_rate_values.entry(cohort).or_default().push(rate);
    }
}

fn percentile_references(
    values: BTreeMap<(String, FixedWorkSampleKind), Vec<f64>>,
    minimum_samples: usize,
    percentile_value: usize,
) -> BTreeMap<(String, FixedWorkSampleKind), MetricReference> {
    values
        .into_iter()
        .filter_map(|(key, samples)| {
            let count = samples.len();
            (count >= minimum_samples)
                .then(|| {
                    percentile(samples, percentile_value).map(|value| {
                        (
                            key,
                            MetricReference {
                                value,
                                samples: count,
                            },
                        )
                    })
                })
                .flatten()
        })
        .collect()
}

fn assess_sample(
    sample: &Sample,
    references: &References,
    cohort: &(String, FixedWorkSampleKind),
    logical_cpus: Option<usize>,
) -> Assessment {
    let ipc = sample_ipc(sample);
    let ipc_reference = references.ipc.get(cohort);
    let reference_ipc = ipc_reference.map(|reference| reference.value);
    let ipc_reference_samples = ipc_reference.map(|reference| reference.samples);
    let involuntary_switches_per_second = sample_involuntary_switch_rate(sample);
    let switch_rate_reference = references.involuntary_switch_rate.get(cohort);
    let reference_involuntary_switches_per_second =
        switch_rate_reference.map(|reference| reference.value);
    let involuntary_switch_reference_samples =
        switch_rate_reference.map(|reference| reference.samples);
    let load_per_logical_cpu = sample_load(sample)
        .zip(logical_cpus)
        .and_then(|(load, cpus)| (cpus > 0).then_some(load / cpus as f64));
    let mut reasons = Vec::new();
    if let (Some(ipc), Some(reference)) = (ipc, reference_ipc) {
        let ratio = ipc / reference;
        if ratio < MIN_IPC_REFERENCE_RATIO {
            reasons.push(Reason::IpcBelowReference {
                ratio,
                minimum_ratio: MIN_IPC_REFERENCE_RATIO,
            });
        }
    }
    if let Some(reference) = reference_involuntary_switches_per_second {
        if reference > 0.0 {
            if let Some(rate) = involuntary_switches_per_second {
                let ratio = rate / reference;
                if ratio > MAX_INVOLUNTARY_SWITCH_RATE_RATIO {
                    reasons.push(Reason::InvoluntarySwitchRateAboveReference {
                        ratio,
                        maximum_ratio: MAX_INVOLUNTARY_SWITCH_RATE_RATIO,
                    });
                }
            }
        }
    }
    if let Some(load) = load_per_logical_cpu {
        if load >= MAX_LOAD_PER_LOGICAL_CPU {
            reasons.push(Reason::LoadAtCapacity {
                load_per_logical_cpu: load,
            });
        }
    }
    Assessment {
        clean: reasons.is_empty(),
        checks_complete: ipc.is_some()
            && reference_ipc.is_some()
            && involuntary_switches_per_second.is_some()
            && reference_involuntary_switches_per_second.is_some()
            && load_per_logical_cpu.is_some(),
        ipc_checked: ipc.is_some() && reference_ipc.is_some(),
        involuntary_switch_rate_checked: involuntary_switches_per_second.is_some()
            && reference_involuntary_switches_per_second.is_some(),
        load_checked: load_per_logical_cpu.is_some(),
        ipc,
        reference_ipc,
        ipc_reference_samples,
        involuntary_switches_per_second,
        reference_involuntary_switches_per_second,
        involuntary_switch_reference_samples,
        load_per_logical_cpu,
        reasons,
    }
}

fn sample_ipc(sample: &Sample) -> Option<f64> {
    let instructions = sample.instructions?;
    let cycles = sample.cycles?;
    (cycles > 0).then_some(instructions as f64 / cycles as f64)
}

fn sample_involuntary_switch_rate(sample: &Sample) -> Option<f64> {
    let switches = sample.involuntary_context_switches?;
    (sample.wall_ns > 0).then_some(switches as f64 * NANOSECONDS_PER_SECOND / sample.wall_ns as f64)
}

fn sample_load(sample: &Sample) -> Option<f64> {
    [sample.host_before.as_ref(), sample.host_after.as_ref()]
        .into_iter()
        .flatten()
        .filter_map(|snapshot| snapshot.load_average.as_ref())
        .map(|load| load.one_minute)
        .reduce(f64::max)
}

fn percentile(mut values: Vec<f64>, percentile_value: usize) -> Option<f64> {
    values.sort_by(f64::total_cmp);
    let index = values.len().checked_sub(1)? * percentile_value / 100;
    values.get(index).copied()
}

#[cfg(test)]
mod tests {
    use super::{assess_sample, percentile, MetricReference, References, MIN_IPC_REFERENCE_RATIO};
    use crate::{FixedWorkSampleKind, HostSnapshot, LoadAverage, Sample};
    use std::collections::BTreeMap;

    const COHORT: (String, FixedWorkSampleKind) = (String::new(), FixedWorkSampleKind::Work);

    #[test]
    fn ipc_reference_rejects_the_measured_contention_band() {
        let reference = percentile(vec![5.7, 5.8, 5.9, 6.0, 5.75], 75).unwrap();
        let references = References {
            ipc: BTreeMap::from([(COHORT.clone(), metric_reference(reference))]),
            ..References::default()
        };
        let quiet = sample(5.7, 100, 1_000_000_000, None);
        let loaded = sample(4.8, 2_000, 1_000_000_000, None);
        let quiet_assessment = assess_sample(&quiet, &references, &COHORT, Some(10));
        let loaded_assessment = assess_sample(&loaded, &references, &COHORT, Some(10));

        assert!(quiet_assessment.clean);
        assert!(!loaded_assessment.clean);
        assert_eq!(MIN_IPC_REFERENCE_RATIO, 0.90);
    }

    #[test]
    fn switch_rate_reference_rejects_the_measured_contention_band() {
        let quiet_rates = [78.0, 95.0, 156.0, 174.0, 249.0];
        let reference = percentile(quiet_rates.to_vec(), 25).unwrap();
        let references = References {
            involuntary_switch_rate: BTreeMap::from([(
                COHORT.clone(),
                metric_reference(reference),
            )]),
            ..References::default()
        };
        let quiet = sample(5.8, 174, 1_000_000_000, None);
        let loaded = sample(5.8, 761, 1_000_000_000, None);
        let quiet_assessment = assess_sample(&quiet, &references, &COHORT, Some(10));
        let loaded_assessment = assess_sample(&loaded, &references, &COHORT, Some(10));

        assert!(quiet_assessment.clean);
        assert!(quiet_assessment.involuntary_switch_rate_checked);
        assert!(!loaded_assessment.clean);
        assert!(matches!(
            loaded_assessment.reasons.as_slice(),
            [super::Reason::InvoluntarySwitchRateAboveReference { .. }]
        ));
    }

    #[test]
    fn saturated_load_is_flagged_even_when_counters_are_typical() {
        let sample = sample(5.8, 100, 1_000_000_000, Some(10.0));
        let references = References {
            ipc: BTreeMap::from([(COHORT.clone(), metric_reference(5.8))]),
            involuntary_switch_rate: BTreeMap::from([(COHORT.clone(), metric_reference(100.0))]),
        };
        let assessment = assess_sample(&sample, &references, &COHORT, Some(10));

        assert!(!assessment.clean);
        assert!(matches!(
            assessment.reasons.as_slice(),
            [super::Reason::LoadAtCapacity { .. }]
        ));
    }

    fn metric_reference(value: f64) -> MetricReference {
        MetricReference { value, samples: 5 }
    }

    fn sample(
        ipc: f64,
        involuntary_context_switches: u64,
        wall_ns: u128,
        one_minute_load: Option<f64>,
    ) -> Sample {
        let cycles = 1_000;
        let snapshot = || {
            one_minute_load.map(|one_minute| HostSnapshot {
                captured_unix_ns: 0,
                load_average: Some(LoadAverage {
                    one_minute,
                    five_minutes: one_minute,
                    fifteen_minutes: one_minute,
                }),
                load_average_error: None,
                top_cpu_consumers: Vec::new(),
                process_list_error: None,
            })
        };
        Sample {
            status: 0,
            timed_out: false,
            wall_ns,
            peak_rss_bytes: Some(1),
            score: None,
            instructions: Some((ipc * cycles as f64) as u64),
            cycles: Some(cycles),
            page_faults: None,
            page_reclaims: None,
            involuntary_context_switches: Some(involuntary_context_switches),
            host_before: snapshot(),
            host_after: snapshot(),
            contention: None,
            stdout: String::new(),
            stderr: String::new(),
        }
    }
}
