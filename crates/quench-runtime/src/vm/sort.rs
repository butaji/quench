use super::{Host, JsError, ResidualProgram, Value, Vm};
use std::cmp::Ordering;

/// The shortest run the sort merges: shorter natural runs are extended by binary insertion
/// first, as TimSort does (V8's `Array.prototype.sort`, CPython's `list.sort`), so random
/// input merges few runs and ordered input keeps its single run.
const MIN_RUN_LENGTH: usize = 32;

/// Sorts `values` stably by `compare`, which may fail or be inconsistent: whatever it
/// answers, the result is a permutation of `values`.
///
/// A natural merge sort: each maximal non-descending run, or strictly descending run
/// reversed (which keeps equal values in order), is extended to `MIN_RUN_LENGTH` by binary
/// insertion, then adjacent runs merge pairwise until one remains. Already ordered or
/// reversed input costs one comparison per value.
pub(super) fn try_stable_sort_by<T, E>(
    values: &mut [T],
    mut compare: impl FnMut(&T, &T) -> Result<Ordering, E>,
) -> Result<(), E>
where
    T: Copy,
{
    let mut runs = Vec::new();
    let mut start = 0;
    while start < values.len() {
        let mut end = natural_run_end(values, start, &mut compare)?;
        let extended = start.saturating_add(MIN_RUN_LENGTH).min(values.len());
        if end < extended {
            binary_insertion(values, start, end, extended, &mut compare)?;
            end = extended;
        }
        runs.push(end);
        start = end;
    }
    let mut scratch = Vec::with_capacity(values.len());
    while runs.len() > 1 {
        let mut merged = Vec::with_capacity(runs.len().div_ceil(2));
        let mut start = 0;
        for pair in runs.chunks(2) {
            let end = pair[pair.len() - 1];
            if let [middle, _] = *pair {
                merge_runs(values, &mut scratch, start, middle, end, &mut compare)?;
            }
            merged.push(end);
            start = end;
        }
        runs = merged;
    }
    Ok(())
}

/// The end of the run starting at `start`: its maximal non-descending prefix, or its
/// maximal strictly descending prefix, reversed in place.
fn natural_run_end<T, E>(
    values: &mut [T],
    start: usize,
    compare: &mut impl FnMut(&T, &T) -> Result<Ordering, E>,
) -> Result<usize, E> {
    let mut end = start + 1;
    if end == values.len() {
        return Ok(end);
    }
    let descending = compare(&values[end - 1], &values[end])? == Ordering::Greater;
    end += 1;
    while end < values.len()
        && (compare(&values[end - 1], &values[end])? == Ordering::Greater) == descending
    {
        end += 1;
    }
    if descending {
        values[start..end].reverse();
    }
    Ok(end)
}

/// Inserts `values[sorted..end]` one by one into the sorted `values[start..sorted]`, each
/// after every value not greater than it.
fn binary_insertion<T: Copy, E>(
    values: &mut [T],
    start: usize,
    sorted: usize,
    end: usize,
    compare: &mut impl FnMut(&T, &T) -> Result<Ordering, E>,
) -> Result<(), E> {
    for next in sorted..end {
        let value = values[next];
        let (mut low, mut high) = (start, next);
        while low < high {
            let middle = low + (high - low) / 2;
            if compare(&values[middle], &value)? == Ordering::Greater {
                high = middle;
            } else {
                low = middle + 1;
            }
        }
        values.copy_within(low..next, low + 1);
        values[low] = value;
    }
    Ok(())
}

/// Merges the sorted `values[start..middle]` and `values[middle..end]` in place through
/// `scratch`, a left value first on a tie; runs already in order stay as they are. A failed
/// comparison stops the merge with `values` still a permutation: the unmerged left values
/// fill exactly the gap before the unmerged right ones.
fn merge_runs<T: Copy, E>(
    values: &mut [T],
    scratch: &mut Vec<T>,
    start: usize,
    middle: usize,
    end: usize,
    compare: &mut impl FnMut(&T, &T) -> Result<Ordering, E>,
) -> Result<(), E> {
    if compare(&values[middle - 1], &values[middle])? != Ordering::Greater {
        return Ok(());
    }
    scratch.clear();
    scratch.extend_from_slice(&values[start..middle]);
    let (mut left, mut right, mut output) = (0, middle, start);
    let mut outcome = Ok(());
    while left < scratch.len() && right < end {
        match compare(&scratch[left], &values[right]) {
            Ok(Ordering::Greater) => {
                values[output] = values[right];
                right += 1;
            }
            Ok(_) => {
                values[output] = scratch[left];
                left += 1;
            }
            Err(error) => {
                outcome = Err(error);
                break;
            }
        }
        output += 1;
    }
    let rest = &scratch[left..];
    values[output..output + rest.len()].copy_from_slice(rest);
    outcome
}

impl<H: Host> Vm<H> {
    /// The comparator's order of `left` and `right`, which the caller keeps reachable.
    pub(super) fn compare_sort_callback(
        &mut self,
        p: &ResidualProgram,
        comparator: Value,
        left: Value,
        right: Value,
    ) -> Result<Ordering, JsError> {
        let result = self.call_value(p, comparator, Value::UNDEFINED, &[left, right])?;
        let number = self.with_call_roots([result], |vm| vm.to_number(p, result))?;
        Ok(if number < 0.0 {
            Ordering::Less
        } else if number > 0.0 {
            Ordering::Greater
        } else {
            Ordering::Equal
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A deterministic sequence of `(key, position)` pairs with repeated keys, so stability
    /// shows: keys come from a linear congruential generator (Knuth's MMIX constants).
    fn keyed(len: usize, distinct_keys: u64) -> Vec<(u64, usize)> {
        const MULTIPLIER: u64 = 6364136223846793005;
        const INCREMENT: u64 = 1442695040888963407;
        let mut state = 0_u64;
        (0..len)
            .map(|position| {
                state = state.wrapping_mul(MULTIPLIER).wrapping_add(INCREMENT);
                ((state >> 33) % distinct_keys, position)
            })
            .collect()
    }

    fn by_key(left: &(u64, usize), right: &(u64, usize)) -> Result<Ordering, ()> {
        Ok(left.0.cmp(&right.0))
    }

    #[test]
    fn sorts_stably_like_the_standard_stable_sort() {
        for len in [0, 1, 2, 3, MIN_RUN_LENGTH - 1, MIN_RUN_LENGTH, MIN_RUN_LENGTH + 1, 1000, 4099] {
            for distinct_keys in [1, 2, 7, 1 << 20] {
                let mut values = keyed(len, distinct_keys);
                let mut expected = values.clone();
                expected.sort_by_key(|&(key, _)| key);
                try_stable_sort_by(&mut values, by_key).unwrap();
                assert_eq!(values, expected, "len {len}, keys {distinct_keys}");
            }
        }
    }

    #[test]
    fn ordered_and_reversed_input_take_one_comparison_per_value() {
        const LEN: usize = 50_000;
        for reversed in [false, true] {
            let mut values: Vec<usize> = (0..LEN).collect();
            if reversed {
                values.reverse();
            }
            let mut comparisons = 0;
            try_stable_sort_by(&mut values, |left, right| {
                comparisons += 1;
                Ok::<_, ()>(left.cmp(right))
            })
            .unwrap();
            assert!(values.iter().copied().eq(0..LEN));
            assert!(comparisons < LEN, "{comparisons} comparisons, reversed {reversed}");
        }
    }

    #[test]
    fn a_failing_or_inconsistent_comparator_leaves_a_permutation() {
        const LEN: usize = 777;
        for fail_after in [0, 1, 40, 500, 3000, 6000] {
            let mut values: Vec<usize> = keyed(LEN, 1 << 20).into_iter().map(|(key, _)| key as usize).collect();
            let mut expected = values.clone();
            expected.sort();
            let mut calls = 0;
            let _ = try_stable_sort_by(&mut values, |left, right| {
                calls += 1;
                if calls > fail_after { Err(()) } else { Ok(left.cmp(right)) }
            });
            values.sort();
            assert_eq!(values, expected, "failing after {fail_after}");
        }
        let mut values: Vec<usize> = (0..LEN).collect();
        let mut flip = false;
        try_stable_sort_by(&mut values, |_, _| {
            flip = !flip;
            Ok::<_, ()>(if flip { Ordering::Less } else { Ordering::Greater })
        })
        .unwrap();
        values.sort();
        assert!(values.iter().copied().eq(0..LEN));
    }
}
