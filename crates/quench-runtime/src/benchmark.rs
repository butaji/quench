#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BenchmarkRecord {
    pub workload: String,
    pub commit: String,
    pub samples: u32,
    pub wall_time_ns: u64,
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
    pub fn wall_time_per_operation(&self) -> Option<f64> {
        (self.operations != 0).then(|| self.wall_time_ns as f64 / self.operations as f64)
    }

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

/// Returns whether a 51-bit payload can be carried by a quiet-NaN encoding.
///
/// This is a feasibility probe only. `Value` remains the authoritative
/// representation because heap references and JavaScript numbers still need
/// their complete slow-path semantics.
pub const NAN_BOX_PAYLOAD_BITS: u32 = 51;

#[inline]
pub const fn nan_box_payload_fits(payload: u64) -> bool {
    payload < (1_u64 << NAN_BOX_PAYLOAD_BITS)
}

#[cfg(test)]
mod tests {
    use super::BenchmarkRecord;

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
}

#[cfg(test)]
mod nan_box_tests {
    use super::{nan_box_payload_fits, NAN_BOX_PAYLOAD_BITS};

    #[test]
    fn payload_boundary_is_explicit() {
        assert!(nan_box_payload_fits(0));
        assert!(nan_box_payload_fits((1_u64 << NAN_BOX_PAYLOAD_BITS) - 1));
        assert!(!nan_box_payload_fits(1_u64 << NAN_BOX_PAYLOAD_BITS));
    }
}
