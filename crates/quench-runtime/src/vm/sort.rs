use super::{Host, JsError, ResidualProgram, Value, Vm};
use std::cmp::Ordering;

const INITIAL_RUN_WIDTH: usize = 1;
const MERGE_PASS_GROWTH: usize = 2;

pub(super) fn try_stable_sort_by<T, E>(
    values: &mut [T],
    mut compare: impl FnMut(&T, &T) -> Result<Ordering, E>,
) -> Result<(), E>
where
    T: Copy,
{
    let mut source = values.to_vec();
    let mut target = source.clone();
    let mut run_width = INITIAL_RUN_WIDTH;
    while run_width < source.len() {
        let mut start = 0;
        while start < source.len() {
            let middle = start.saturating_add(run_width).min(source.len());
            let end = middle.saturating_add(run_width).min(source.len());
            merge_runs(&source, &mut target, start, middle, end, &mut compare)?;
            start = end;
        }
        std::mem::swap(&mut source, &mut target);
        run_width = run_width.saturating_mul(MERGE_PASS_GROWTH);
    }
    values.copy_from_slice(&source);
    Ok(())
}

fn merge_runs<T, E>(
    source: &[T],
    target: &mut [T],
    start: usize,
    middle: usize,
    end: usize,
    compare: &mut impl FnMut(&T, &T) -> Result<Ordering, E>,
) -> Result<(), E>
where
    T: Copy,
{
    let mut left = start;
    let mut right = middle;
    for output in start..end {
        target[output] = if left < middle
            && (right == end || compare(&source[left], &source[right])? != Ordering::Greater)
        {
            let value = source[left];
            left += 1;
            value
        } else {
            let value = source[right];
            right += 1;
            value
        };
    }
    Ok(())
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
