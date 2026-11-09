//! Derivation of a lane view's string-case filters: once per view, never per operation.
//! Candidates group by signature in source order; each group takes one bucket of an
//! open-addressed table within the probe budget, or the original selector stays.
use super::*;

/// Each constant's case with its signature in source order, and the lengths a query
/// matching one of them may have.
struct Candidates {
    cases: Vec<(u64, Case)>,
    short_lengths: u64,
    max_length: usize,
}

/// The candidates of `values`, or `None` when one is no resolved flat string.
fn candidates(heap: &crate::heap::Heap, values: &[Value]) -> Option<Candidates> {
    let mut candidates = Candidates { cases: Vec::with_capacity(values.len()), short_lengths: 0, max_length: 0 };
    for (index, &value) in values.iter().enumerate() {
        let record = value.direct_string_index()?;
        // SAFETY: the constant run retains this resolved string identity.
        let text = unsafe { heap.string_record_by_id(record) };
        let length = text.len();
        candidates.cases.push((signature(text, length)?, Case { index: index as u16, record }));
        candidates.short_lengths |= short_length_bit(length).unwrap_or(0);
        candidates.max_length = candidates.max_length.max(length);
    }
    Some(candidates)
}

/// Places `bucket` at the first empty slot its probe sequence reaches, or `None` past
/// the probe budget.
fn place(buckets: &mut [Bucket], bucket: Bucket) -> Option<()> {
    let mask = buckets.len() - 1;
    let base = bucket_index(bucket.key, mask);
    let slot = (0..MAX_BUCKET_PROBES)
        .map(|probe| (base + probe) & mask)
        .find(|&slot| buckets[slot].count == EMPTY_BUCKET_COUNT)?;
    buckets[slot] = bucket;
    Some(())
}

/// Appends `group`'s distinct records to `cases`, each at its first source position, and
/// returns its bucket, or `None` when more than the comparison budget remain.
fn kept_group(group: &[(u64, Case)], cases: &mut Vec<Case>) -> Option<Bucket> {
    let first = cases.len();
    for &(_, case) in group {
        // Equal constants share a record; retain only their first source position.
        if !cases[first..].iter().any(|kept| kept.record == case.record) {
            cases.push(case);
        }
    }
    let count = cases.len() - first;
    (count <= MAX_CASE_COMPARISONS).then(|| Bucket { key: group[0].0, first: first as u32, count: count as u16 })
}

impl StringCaseFilter {
    fn new(heap: &crate::heap::Heap, start: usize, values: &[Value]) -> Option<Self> {
        let Candidates { mut cases, short_lengths, max_length } = candidates(heap, values)?;
        // Stable, so each group keeps its source order.
        cases.sort_by_key(|&(key, _)| key);
        let same_signature = |left: &(u64, Case), right: &(u64, Case)| left.0 == right.0;
        let capacity = (cases.chunk_by(same_signature).count() * BUCKET_CAPACITY_FACTOR).next_power_of_two();
        let mut buckets = vec![Bucket::default(); capacity].into_boxed_slice();
        let mut kept = Vec::with_capacity(cases.len());
        for group in cases.chunk_by(same_signature) {
            place(&mut buckets, kept_group(group, &mut kept)?)?;
        }
        Some(Self { start, short_lengths, max_length, buckets, cases: kept.into_boxed_slice() })
    }
}

/// Replaces each validated string-case selector of `records` whose constants admit a
/// filter with the filtered selector, and returns the filters the records point into.
pub(in super::super) fn specialize<H: Host>(
    heap: &crate::heap::Heap,
    constants: &[Value],
    decoded: &[crate::bytecode::WideInstruction],
    records: &mut [LaneInstruction],
) -> Box<[StringCaseFilter]> {
    let mut filters = Vec::new();
    let mut sites = Vec::new();
    for (pc, record) in records.iter().enumerate() {
        // The site's own opcode names it a case selector; a handler's address does not.
        if !decoded.get(pc).is_some_and(|instruction| instruction.op().is_string_case_index()) {
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
        records[pc].operand = LaneOperand { string_case_filter: filter };
        records[pc].handler = lane_string_case_filtered::<H> as *const ();
    }
    filters
}
