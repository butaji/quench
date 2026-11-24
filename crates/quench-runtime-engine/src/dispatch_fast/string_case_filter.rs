//! Immutable string-case candidates, resolved once with bounded direct-table lookup.
#[allow(unused_imports)]
use crate::ext_traits::*;
use quench_runtime::vm::wtf16::{JsString, Units, MAX_STRING_UNITS};
use super::*;
use std::hash::Hasher;

// Construction runs once, as the lane view is derived; only `select` runs per operation.
#[path = "../string_case_derivation.rs"]
pub(crate) mod derivation;
pub(crate) use derivation::specialize;

/// The low word contains two edge units; the high word contains the UTF-16 length.
const CASE_LENGTH_SHIFT: u32 = 2 * u16::BITS;
const EMPTY_EDGE_UNIT: u16 = 0;
const EMPTY_BUCKET_COUNT: u16 = 0;
/// Half-full tables bound metadata to fewer than four buckets per distinct signature.
const BUCKET_CAPACITY_FACTOR: usize = 2;
/// At most eight bucket reads per query. If construction cannot meet that cost,
/// specialization is declined and the original selector remains authoritative.
const MAX_BUCKET_PROBES: usize = 8;
/// Bound full string comparisons as well as table probes. Larger collision groups keep
/// the original selector instead of adding an unbounded optimized-path scan.
const MAX_CASE_COMPARISONS: usize = 8;
const _: () = assert!(MAX_STRING_UNITS <= u32::MAX as usize);

#[inline(always)]
fn signature(text: &JsString, length: usize) -> Option<u64> {
    let (first, last) = match text.existing_flat()?.view() {
        Units::Ascii(bytes) | Units::Latin1(bytes) => (
            u16::from(bytes.first().copied().unwrap_or(EMPTY_EDGE_UNIT as u8)),
            u16::from(bytes.last().copied().unwrap_or(EMPTY_EDGE_UNIT as u8)),
        ),
        Units::Wide(units) => (
            units.first().copied().unwrap_or(EMPTY_EDGE_UNIT),
            units.last().copied().unwrap_or(EMPTY_EDGE_UNIT),
        ),
    };
    Some(((length as u64) << CASE_LENGTH_SHIFT) | (u64::from(first) << u16::BITS) | u64::from(last))
}

/// The bit of `StringCaseFilter::short_lengths` that records `length`; none past its word.
#[inline(always)]
fn short_length_bit(length: usize) -> Option<u64> {
    (length < u64::BITS as usize).then(|| 1_u64 << length)
}

#[inline(always)]
fn bucket_index(key: u64, mask: usize) -> usize {
    let mut hash = rustc_hash::FxHasher::default();
    hash.write_u64(key);
    hash.finish() as usize & mask
}

use quench_runtime::vm::engine_types::Bucket;

use quench_runtime::vm::engine_types::Case;

pub(crate) use quench_runtime::vm::engine_types::StringCaseFilter;

#[quench_macros::ext(StringCaseFilterOps1)]
impl StringCaseFilter {
    #[inline(always)]
    fn select(&self, vm: &Vm, value: Value, text: &JsString, constants: &[Value]) -> u32 {
        let missing = constants.len() as u32;
        let length = text.len();
        if length > self.max_length || short_length_bit(length).is_some_and(|bit| self.short_lengths & bit == 0) {
            return missing;
        }
        let Some(key) = signature(text, length) else {
            return vm.string_case_index(value, constants);
        };
        for case in self.group(key) {
            // SAFETY: the original constant run retains every resolved record.
            if unsafe { vm.heap_reads().string_record_by_id(case.record) } == text {
                return u32::from(case.index);
            }
        }
        missing
    }

    /// The cases whose signature is `key`, none when no bucket holds it. Construction
    /// proved every known key is found within the probe budget.
    #[inline(always)]
    fn group(&self, key: u64) -> &[Case] {
        let mask = self.buckets.len() - 1;
        let base = bucket_index(key, mask);
        for probe in 0..MAX_BUCKET_PROBES {
            // SAFETY: masking indexes the nonempty power-of-two bucket array.
            let bucket = unsafe { self.buckets.get_unchecked((base + probe) & mask) };
            if bucket.count == EMPTY_BUCKET_COUNT {
                break;
            }
            if bucket.key == key {
                let start = bucket.first as usize;
                // SAFETY: construction recorded a complete contiguous group in this array.
                return unsafe { self.cases.get_unchecked(start..start + usize::from(bucket.count)) };
            }
        }
        &[]
    }
}

/// The index of `value` among the `count` program constants from `start`, as a `StringCaseIndex` row
/// answers it: the record's filter where `filter` is present (`StringCaseFilter::select`), else the VM's
/// selector (`Vm::string_case_index`). A filtered row's constants are the filter's own run.
///
/// # Safety
/// `cx` is the running lane's context; `filter` is null or the record's filter, which the lane's
/// constants retain.
#[inline(always)]
pub(crate) unsafe fn case_index_in_lane(cx: *mut (), filter: *const StringCaseFilter, value: Value, start: usize, count: usize) -> u32 {
    let cx = cx.cast::<LaneContext>();
    // SAFETY: the original selector's validated run of constants is unchanged.
    let cases = unsafe { (&*(*cx).constants).get_unchecked(start..start + count) };
    let vm = unsafe { vm(cx) };
    let index = match unsafe { filter.as_ref() } {
        Some(filter) => match vm.heap_reads().string(value) {
            Some(text) => filter.select(vm, value, text, cases),
            None => count as u32,
        },
        None => vm.string_case_index(value, cases),
    };
    #[cfg(feature = "profile-lane-exits")]
    quench_runtime::vm::lane_exits::record_detail(format!("case-filter cases={} index={}", count, index));
    index
}
