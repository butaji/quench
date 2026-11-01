//! Immutable string-case candidates, resolved once with bounded direct-table lookup.
use super::super::wtf16::{JsString, Units, MAX_STRING_UNITS};
use super::*;
use std::hash::Hasher;

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
        Units::Ascii(bytes) => (
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

#[inline(always)]
fn bucket_index(key: u64, mask: usize) -> usize {
    let mut hash = rustc_hash::FxHasher::default();
    hash.write_u64(key);
    hash.finish() as usize & mask
}

#[derive(Clone, Copy, Default)]
struct Bucket {
    key: u64,
    first: u32,
    count: u16,
}

struct Case {
    index: u16,
    record: u32,
}

pub(super) struct StringCaseFilter {
    start: usize,
    short_lengths: u64,
    max_length: usize,
    buckets: Box<[Bucket]>,
    cases: Box<[Case]>,
}

impl StringCaseFilter {
    fn new(heap: &crate::heap::Heap, start: usize, values: &[Value]) -> Option<Self> {
        let mut groups: rustc_hash::FxHashMap<u64, Vec<Case>> = Default::default();
        let mut short_lengths = 0;
        let mut max_length = 0;
        for (index, &value) in values.iter().enumerate() {
            let record = value.direct_string_index()?;
            // SAFETY: the constant run retains this resolved string identity.
            let text = unsafe { heap.string_record_by_id(record) };
            let length = text.len();
            let key = signature(text, length)?;
            if length < u64::BITS as usize {
                short_lengths |= 1_u64 << length;
            }
            max_length = max_length.max(length);
            let group = groups.entry(key).or_default();
            // Equal constants share a record; retain only their first source position.
            if group.iter().any(|case| case.record == record) {
                continue;
            }
            if group.len() == MAX_CASE_COMPARISONS {
                return None;
            }
            group.push(Case {
                index: index as u16,
                record,
            });
        }
        let capacity = (groups.len() * BUCKET_CAPACITY_FACTOR).next_power_of_two();
        let mut buckets = vec![Bucket::default(); capacity].into_boxed_slice();
        let mut cases = Vec::with_capacity(groups.values().map(Vec::len).sum());
        for (key, group) in groups {
            let base = bucket_index(key, capacity - 1);
            let slot = (0..MAX_BUCKET_PROBES)
                .map(|probe| (base + probe) & (capacity - 1))
                .find(|&slot| buckets[slot].count == EMPTY_BUCKET_COUNT)?;
            buckets[slot] = Bucket {
                key,
                first: cases.len() as u32,
                count: group.len() as u16,
            };
            cases.extend(group);
        }
        Some(Self {
            start,
            short_lengths,
            max_length,
            buckets,
            cases: cases.into_boxed_slice(),
        })
    }

    #[inline(always)]
    fn select<H: Host>(
        &self,
        vm: &Vm<H>,
        value: Value,
        text: &JsString,
        constants: &[Value],
    ) -> u32 {
        let missing = constants.len() as u32;
        let length = text.len();
        if length > self.max_length
            || (length < u64::BITS as usize && self.short_lengths & (1_u64 << length) == 0)
        {
            return missing;
        }
        let Some(key) = signature(text, length) else {
            return vm.string_case_index(value, constants);
        };
        let mask = self.buckets.len() - 1;
        let base = bucket_index(key, mask);
        for probe in 0..MAX_BUCKET_PROBES {
            // SAFETY: masking indexes the nonempty power-of-two bucket array.
            let bucket = unsafe { self.buckets.get_unchecked((base + probe) & mask) };
            if bucket.count == EMPTY_BUCKET_COUNT {
                return missing;
            }
            if bucket.key != key {
                continue;
            }
            let start = bucket.first as usize;
            // SAFETY: construction recorded a complete contiguous group in this array.
            let cases = unsafe {
                self.cases
                    .get_unchecked(start..start + usize::from(bucket.count))
            };
            for case in cases {
                // SAFETY: the original constant run retains every resolved record.
                if unsafe { vm.heap.string_record_by_id(case.record) } == text {
                    return u32::from(case.index);
                }
            }
            return missing;
        }
        // Construction proved every known key is found within this probe budget.
        missing
    }
}

script_handler! {
    fn lane_string_case_filtered<>(cx, r, ip, i, view) {
        // SAFETY: specialization installs this pointer; the view owns the immutable plan.
        let filter = unsafe { &*i.operand.string_case_filter };
        // SAFETY: the original selector's validated run of constants is unchanged.
        let cases = unsafe { (&*(*cx).constants).get_unchecked(filter.start..filter.start + usize::from(i.b)) };
        let vm = unsafe { vm(cx) };
        let value = r.get(i.c);
        let index = match vm.heap.string(value) {
            Some(text) => filter.select(vm, value, text, cases),
            None => u32::from(i.b),
        };
        #[cfg(feature = "profile-lane-exits")]
        super::super::lane_exits::record_detail(format!("case-filter cases={} index={}", i.b, index));
        r.set(i.a, Value::integer(index as i32));
        next!(cx, r, following(ip), view)
    }
}

pub(super) fn specialize<H: Host>(
    heap: &crate::heap::Heap,
    constants: &[Value],
    records: &mut [LaneInstruction],
) -> Box<[StringCaseFilter]> {
    let mut filters = Vec::new();
    let mut sites = Vec::new();
    for (pc, record) in records.iter().enumerate() {
        if record.handler != lane_string_case_index::<H> as *const () {
            continue;
        }
        let start = record.imm() as usize;
        // SAFETY: this handler is installed only after lane_operand validates the range.
        let cases = unsafe { constants.get_unchecked(start..start + usize::from(record.b)) };
        let Some(filter) = StringCaseFilter::new(heap, start, cases) else {
            continue;
        };
        filters.push(filter);
        sites.push(pc);
    }
    let filters = filters.into_boxed_slice();
    for (pc, filter) in sites.into_iter().zip(filters.iter()) {
        records[pc].operand = LaneOperand {
            string_case_filter: filter,
        };
        records[pc].handler = lane_string_case_filtered::<H> as *const ();
    }
    filters
}
