//! Canonical host hardware-counter snapshot contract.
//!
//! Counters are owned by the profiling tool/host and copied into this immutable
//! snapshot at the boundary. `None` means the host cannot provide that counter;
//! it never means zero. A snapshot is valid for one measurement interval only.
//! The JSON representation uses the field names in [`COUNTER_NAMES`], with
//! unavailable values encoded as `null`; this is the sole wire representation.

/// Stable wire-order names for the counter fields.
pub const COUNTER_NAMES: [&str; 8] = [
    "cycles",
    "instructions",
    "branches",
    "branch_misses",
    "cache_misses",
    "tlb_faults",
    "allocations",
    "copies",
];

/// Version of the JSON counter snapshot contract.
pub const COUNTER_SCHEMA_VERSION: u64 = 1;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HardwareCounters {
    pub cycles: Option<u64>,
    pub instructions: Option<u64>,
    pub branches: Option<u64>,
    pub branch_misses: Option<u64>,
    pub cache_misses: Option<u64>,
    pub tlb_faults: Option<u64>,
    pub allocations: Option<u64>,
    pub copies: Option<u64>,
}

impl HardwareCounters {
    /// True only when every requested counter was observed by the host.
    #[inline]
    pub const fn is_complete(&self) -> bool {
        self.cycles.is_some()
            && self.instructions.is_some()
            && self.branches.is_some()
            && self.branch_misses.is_some()
            && self.cache_misses.is_some()
            && self.tlb_faults.is_some()
            && self.allocations.is_some()
            && self.copies.is_some()
    }

    /// Missing counters are unavailable, not zero-valued measurements.
    #[inline]
    pub const fn unavailable_count(&self) -> usize {
        8 - self.cycles.is_some() as usize
            - self.instructions.is_some() as usize
            - self.branches.is_some() as usize
            - self.branch_misses.is_some() as usize
            - self.cache_misses.is_some() as usize
            - self.tlb_faults.is_some() as usize
            - self.allocations.is_some() as usize
            - self.copies.is_some() as usize
    }

    /// Serialize the canonical counter fields and schema metadata.
    ///
    /// `null` is emitted for unavailable counters, preserving the distinction
    /// from an observed zero. The returned value is owned by the caller.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "cycles": self.cycles,
            "instructions": self.instructions,
            "branches": self.branches,
            "branch_misses": self.branch_misses,
            "cache_misses": self.cache_misses,
            "tlb_faults": self.tlb_faults,
            "allocations": self.allocations,
            "copies": self.copies,
            "counter_contract": {
                "version": COUNTER_SCHEMA_VERSION,
                "fields": COUNTER_NAMES,
                "unavailable_count": self.unavailable_count(),
            },
        })
    }
}

/// The evidence gate for manual prefetch. Unavailable counters never
/// authorize a prefetch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrefetchApproval {
    pub approved: bool,
    pub unavailable_required: usize,
}

impl HardwareCounters {
    /// Evaluate the profiling contract for manual prefetch.
    ///
    /// `cycles` and `cache_misses` are the minimum independent hardware
    /// signals. `None` is unavailable, not zero and not an approval.
    #[inline]
    pub const fn prefetch_approval(&self) -> PrefetchApproval {
        let unavailable_required =
            self.cycles.is_none() as usize + self.cache_misses.is_none() as usize;
        PrefetchApproval {
            approved: unavailable_required == 0,
            unavailable_required,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{HardwareCounters, COUNTER_SCHEMA_VERSION};

    #[test]
    fn unavailable_required_counters_block_prefetch() {
        let approval = HardwareCounters {
            cycles: Some(10),
            ..Default::default()
        }
        .prefetch_approval();
        assert!(!approval.approved);
        assert_eq!(approval.unavailable_required, 1);
    }

    #[test]
    fn observed_zero_required_counters_are_available() {
        let approval = HardwareCounters {
            cycles: Some(0),
            cache_misses: Some(0),
            ..Default::default()
        }
        .prefetch_approval();
        assert!(approval.approved);
        assert_eq!(approval.unavailable_required, 0);
    }

    #[test]
    fn default_snapshot_marks_every_counter_unavailable() {
        let counters = HardwareCounters::default();
        assert!(!counters.is_complete());
        assert_eq!(counters.unavailable_count(), 8);
    }

    #[test]
    fn zero_is_distinct_from_unavailable() {
        let counters = HardwareCounters {
            cycles: Some(0),
            ..Default::default()
        };
        assert_eq!(counters.cycles, Some(0));
        assert_eq!(counters.unavailable_count(), 7);
    }

    #[test]
    fn json_schema_preserves_null_and_zero() {
        let json = HardwareCounters {
            cycles: Some(0),
            cache_misses: None,
            ..Default::default()
        }
        .to_json();
        assert_eq!(json["cycles"], 0);
        assert!(json["cache_misses"].is_null());
        assert_eq!(json["counter_contract"]["version"], COUNTER_SCHEMA_VERSION);
        assert_eq!(
            json["counter_contract"]["fields"].as_array().unwrap().len(),
            8
        );
        assert_eq!(json["counter_contract"]["unavailable_count"], 7);
    }
    #[test]
    fn completeness_requires_all_counter_families() {
        let counters = HardwareCounters {
            cycles: Some(1),
            instructions: Some(2),
            branches: Some(3),
            branch_misses: Some(4),
            cache_misses: Some(5),
            tlb_faults: Some(6),
            allocations: Some(7),
            copies: Some(8),
        };
        assert!(counters.is_complete());
        assert_eq!(counters.unavailable_count(), 0);
    }
}
