use serde_json::Value as JsonValue;

/// Validate one externally supplied budget metric without coercion.
///
/// `null` is the sole unsupported state. A JSON number must be finite and at
/// or below the limit; missing values and every other JSON type are malformed.
pub fn budget_metric_within(value: Option<&JsonValue>, limit: f64) -> Result<bool, &'static str> {
    if !limit.is_finite() || limit < 0.0 {
        return Err("invalid budget limit");
    }
    let Some(value) = value else {
        return Err("missing metric");
    };
    if value.is_null() {
        return Ok(true);
    }
    let Some(number) = value.as_f64() else {
        return Err("metric must be a finite JSON number or null");
    };
    if !number.is_finite() {
        return Err("metric must be finite");
    }
    Ok(number <= limit)
}

/// Canonical input contract for a representative runtime workload.
///
/// Ownership is transferred to the runner when a `WorkloadSpec` is submitted;
/// the runner owns it until it reaches a terminal outcome. Empty source,
/// zero-sized budgets, and zero operations are invalid rather than special
/// cases, so measurements cannot silently describe no work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadSpec {
    pub name: String,
    pub source: String,
    pub operations: u64,
    pub max_steps: u64,
    pub max_output_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkloadState {
    Pending,
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkloadError {
    EmptyName,
    EmptySource,
    EmbeddedNul,
    ZeroOperations,
    ZeroStepBudget,
    ZeroOutputBudget,
    InvalidTransition,
}

impl WorkloadSpec {
    pub fn validate(&self) -> Result<(), WorkloadError> {
        if self.name.trim().is_empty() {
            return Err(WorkloadError::EmptyName);
        }
        if self.source.is_empty() {
            return Err(WorkloadError::EmptySource);
        }
        if self.name.contains('\0') || self.source.contains('\0') {
            return Err(WorkloadError::EmbeddedNul);
        }
        if self.operations == 0 {
            return Err(WorkloadError::ZeroOperations);
        }
        if self.max_steps == 0 {
            return Err(WorkloadError::ZeroStepBudget);
        }
        if self.max_output_bytes == 0 {
            return Err(WorkloadError::ZeroOutputBudget);
        }
        Ok(())
    }
}

impl WorkloadState {
    /// Advance the owned workload lifecycle; terminal states cannot be reused.
    pub fn transition(&mut self, next: Self) -> Result<(), WorkloadError> {
        let valid = matches!(
            (*self, next),
            (Self::Pending, Self::Running)
                | (Self::Running, Self::Completed)
                | (Self::Running, Self::Failed)
        );
        if !valid {
            return Err(WorkloadError::InvalidTransition);
        }
        *self = next;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BenchmarkRecord {
    pub workload: String,
    pub commit: String,
    pub samples: u32,
    pub cycles: u64,
    pub operations: u64,
    pub branches: u64,
    pub branch_misses: u64,
    pub allocations: u64,
    pub live_bytes: u64,
    pub rss_bytes: u64,
    pub binary_text_bytes: u64,
    pub generated_loc: u64,
    pub handwritten_loc: u64,
}

impl BenchmarkRecord {
    pub fn cycles_per_operation(&self) -> Option<f64> {
        (self.operations != 0).then(|| self.cycles as f64 / self.operations as f64)
    }

    pub fn branches_per_operation(&self) -> Option<f64> {
        (self.operations != 0).then(|| self.branches as f64 / self.operations as f64)
    }

    pub fn branch_misses_per_operation(&self) -> Option<f64> {
        (self.operations != 0).then(|| self.branch_misses as f64 / self.operations as f64)
    }

    pub fn allocations_per_operation(&self) -> Option<f64> {
        (self.operations != 0).then(|| self.allocations as f64 / self.operations as f64)
    }
    pub fn live_bytes_per_operation(&self) -> Option<f64> {
        (self.operations != 0).then(|| self.live_bytes as f64 / self.operations as f64)
    }

    pub fn rss_bytes_per_operation(&self) -> Option<f64> {
        (self.operations != 0).then(|| self.rss_bytes as f64 / self.operations as f64)
    }
}

impl BenchmarkRecord {
    /// Reports whether a record can participate in deterministic comparisons.
    ///
    /// Empty identity fields and zero denominators are invalid measurement
    /// states; metric counters themselves remain opaque observations.
    pub fn is_well_formed(&self) -> bool {
        !self.workload.is_empty()
            && !self.commit.is_empty()
            && self.samples != 0
            && self.operations != 0
    }
}

/// Deterministic layout budget used by the feasibility probe.
///
/// These numbers describe only the bit budget of a candidate encoding. They
/// do not define a second runtime value representation: `Value` remains the
/// sole semantic model.
pub const NAN_BOX_PAYLOAD_BITS: u32 = 51;
pub const NAN_BOX_TAG_BITS: u32 = 3;
pub const NAN_BOX_PAYLOAD_VALUES: u64 = 1_u64 << NAN_BOX_PAYLOAD_BITS;
pub const NAN_BOX_TAG_VALUES: u64 = 1_u64 << NAN_BOX_TAG_BITS;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NanBoxFeasibility {
    pub payload_bits: u32,
    pub tag_bits: u32,
    pub payload_values: u64,
    pub tag_values: u64,
}

pub const NAN_BOX_FEASIBILITY: NanBoxFeasibility = NanBoxFeasibility {
    payload_bits: NAN_BOX_PAYLOAD_BITS,
    tag_bits: NAN_BOX_TAG_BITS,
    payload_values: NAN_BOX_PAYLOAD_VALUES,
    tag_values: NAN_BOX_TAG_VALUES,
};

#[inline]
pub const fn nan_box_payload_fits(payload: u64) -> bool {
    payload < NAN_BOX_PAYLOAD_VALUES
}

#[inline]
pub const fn nan_box_tag_fits(tag: u8) -> bool {
    (tag as u64) < NAN_BOX_TAG_VALUES
}

#[inline]
pub const fn nan_box_feasibility() -> NanBoxFeasibility {
    NAN_BOX_FEASIBILITY
}

#[cfg(test)]
mod nan_box_tests {
    use super::{
        nan_box_feasibility, nan_box_payload_fits, nan_box_tag_fits, NAN_BOX_PAYLOAD_BITS,
        NAN_BOX_PAYLOAD_VALUES, NAN_BOX_TAG_BITS, NAN_BOX_TAG_VALUES,
    };

    #[test]
    fn payload_and_tag_boundaries_are_explicit() {
        assert!(nan_box_payload_fits(0));
        assert!(nan_box_payload_fits(NAN_BOX_PAYLOAD_VALUES - 1));
        assert!(!nan_box_payload_fits(NAN_BOX_PAYLOAD_VALUES));
        assert!(nan_box_tag_fits(0));
        assert!(nan_box_tag_fits((NAN_BOX_TAG_VALUES - 1) as u8));
        assert!(!nan_box_tag_fits(NAN_BOX_TAG_VALUES as u8));
    }

    #[test]
    fn feasibility_metrics_are_deterministic() {
        let metrics = nan_box_feasibility();
        assert_eq!(metrics.payload_bits, NAN_BOX_PAYLOAD_BITS);
        assert_eq!(metrics.tag_bits, NAN_BOX_TAG_BITS);
        assert_eq!(metrics.payload_values, 1_u64 << NAN_BOX_PAYLOAD_BITS);
        assert_eq!(metrics.tag_values, 1_u64 << NAN_BOX_TAG_BITS);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        budget_metric_within, BenchmarkRecord, WorkloadError, WorkloadSpec, WorkloadState,
    };

    #[test]
    fn derives_bytes_per_operation_without_dividing_by_zero() {
        let record = BenchmarkRecord {
            workload: "fixture".into(),
            commit: "test".into(),
            samples: 1,
            wall_time_ns: 100,
            cycles: 0,
            operations: 4,
            branches: 0,
            branch_misses: 0,
            allocations: 0,
            live_bytes: 80,
            rss_bytes: 120,
            binary_text_bytes: 0,
            generated_loc: 0,
            handwritten_loc: 0,
        };
        assert_eq!(record.live_bytes_per_operation(), Some(20.0));
        assert_eq!(record.wall_time_per_operation(), Some(25.0));
        assert_eq!(record.rss_bytes_per_operation(), Some(30.0));
        assert_eq!(
            BenchmarkRecord {
                operations: 0,
                ..record
            }
            .live_bytes_per_operation(),
            None
        );
    }

    fn fixture() -> WorkloadSpec {
        WorkloadSpec {
            name: "json-loop".into(),
            source: "JSON.stringify({ok:true})".into(),
            operations: 100,
            max_steps: 10_000,
            max_output_bytes: 4096,
        }
    }

    #[test]
    fn workload_contract_rejects_invalid_inputs() {
        assert_eq!(fixture().validate(), Ok(()));
        let mut invalid = fixture();
        invalid.operations = 0;
        assert_eq!(invalid.validate(), Err(WorkloadError::ZeroOperations));
        invalid = fixture();
        invalid.source.push('\0');
        assert_eq!(invalid.validate(), Err(WorkloadError::EmbeddedNul));
    }

    #[test]
    fn workload_lifecycle_is_monotonic_and_terminal() {
        let mut state = WorkloadState::Pending;
        assert_eq!(state.transition(WorkloadState::Running), Ok(()));
        assert_eq!(state.transition(WorkloadState::Completed), Ok(()));
        assert_eq!(
            state.transition(WorkloadState::Running),
            Err(WorkloadError::InvalidTransition)
        );
    }

    #[test]
    fn rejects_invalid_identity_and_measurement_states() {
        let record = BenchmarkRecord {
            workload: "fixture".into(),
            commit: "test".into(),
            samples: 1,
            wall_time_ns: 100,
            cycles: 0,
            operations: 4,
            branches: 0,
            branch_misses: 0,
            allocations: 0,
            live_bytes: 80,
            rss_bytes: 120,
            binary_text_bytes: 0,
            generated_loc: 0,
            handwritten_loc: 0,
        };
        assert!(record.is_well_formed());
        assert!(!BenchmarkRecord {
            workload: String::new(),
            ..record.clone()
        }
        .is_well_formed());
        assert!(!BenchmarkRecord {
            commit: String::new(),
            ..record.clone()
        }
        .is_well_formed());
        assert!(!BenchmarkRecord {
            samples: 0,
            ..record.clone()
        }
        .is_well_formed());
        assert!(!BenchmarkRecord {
            operations: 0,
            ..record
        }
        .is_well_formed());
    }
    #[test]
    fn budget_metrics_reject_malformed_nonfinite_and_over_limit() {
        let null = serde_json::json!(null);
        let finite = serde_json::json!(10);
        assert!(serde_json::from_str::<serde_json::Value>("1e999").is_err());
        let malformed = serde_json::json!("10");
        assert_eq!(budget_metric_within(Some(&null), 10.0), Ok(true));
        assert_eq!(budget_metric_within(Some(&finite), 10.0), Ok(true));
        assert_eq!(
            budget_metric_within(Some(&serde_json::json!(11)), 10.0),
            Ok(false)
        );
        assert_eq!(
            budget_metric_within(Some(&malformed), 10.0),
            Err("metric must be a finite JSON number or null")
        );
        assert_eq!(budget_metric_within(None, 10.0), Err("missing metric"));
    }
}
