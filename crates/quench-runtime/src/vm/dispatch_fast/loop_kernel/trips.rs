use super::super::super::operations::NumberRelation;

/// The largest trip estimate: a float holds it exactly, and no kernel runs that long.
const MAX_ESTIMATE: f64 = 1e15;

/// The trip count of `counter REL bound` from `start`, stepping by `step`: how many
/// iterations' tests hold before the first that fails (unbounded when the step moves the
/// counter away from failing; the accesses' bounds then limit the run).
pub(super) fn trip_count(start: f64, step: f64, relation: NumberRelation, bound: f64) -> u64 {
    let holds = |value: f64| match relation {
        NumberRelation::LessThan => value < bound,
        NumberRelation::LessEqual => value <= bound,
        NumberRelation::GreaterThan => value > bound,
        NumberRelation::GreaterEqual => value >= bound,
        NumberRelation::Equal | NumberRelation::NotEqual => false,
    };
    if !holds(start) {
        return 0;
    }
    let toward = matches!(relation, NumberRelation::LessThan | NumberRelation::LessEqual) == (step > 0.0);
    if !toward {
        return u64::MAX;
    }
    // An estimate from the distance, then exact steps around it.
    let mut k = ((bound - start) / step).floor().clamp(0.0, MAX_ESTIMATE) as u64;
    while k > 0 && !holds(start + step * k as f64) {
        k -= 1;
    }
    while holds(start + step * (k + 1) as f64) {
        k += 1;
    }
    k + 1
}

/// The iterations an access at the affine index `first + step * k` stays below `length` for.
pub(super) fn fits(first: i64, step: i64, length: usize) -> u64 {
    let length = length as i64;
    if first < 0 || first >= length {
        0
    } else if step == 0 {
        u64::MAX
    } else if step > 0 {
        ((length - 1 - first) / step + 1) as u64
    } else {
        (first / -step + 1) as u64
    }
}

/// Work-clock units (typical-host nanoseconds) charged per kernel iteration.
pub(super) const WORK_PER_ITERATION: u64 = 4;
