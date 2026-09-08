//! Ordered dense-F64 recurrence facts derived from structured residual loops.

use crate::{machine::CodeView, value::Value};

#[path = "stencil_ordered_neighbor_nested.rs"]
mod nested;
use nested::{select_repeated, RepeatedRecurrence};

#[path = "stencil_ordered_neighbor_select.rs"]
mod selection;
use selection::{exact_i32_value, select, Bound, IndexedLoad, NumericExpr, OrderedRecurrence};

const MAX_ORDERED_RECURRENCE_ELEMENTS: usize = 4096;

pub(crate) fn execute_structured(
    init: CodeView<'_>,
    test: CodeView<'_>,
    body: CodeView<'_>,
    update: CodeView<'_>,
    dst: u16,
    per_iteration: &[u16],
    registers: &mut crate::register_file::RegisterFile,
) -> Option<Result<crate::completion::Completion, crate::execute::VmError>> {
    if let Some(repeated) = select_repeated(init, test, body, update) {
        iteration_slots_are_private(per_iteration, repeated.pass_slot)?;
        return execute_repeated(repeated, dst, registers, body);
    }
    let recurrence = select(init, test, body, update)?;
    iteration_slots_are_private(per_iteration, recurrence.index_slot)?;
    let result = crate::locals::with_current_ref(|environment| {
        let environment = environment?;
        execute(&recurrence, environment, dst, registers)
    })?;
    crate::execution_trace::stencil_observation(body, 0, "ordered_f64_recurrence", true);
    crate::execution_trace::event(crate::execution_trace::Event::LeafHit);
    #[cfg(test)]
    {
        crate::test_execution_profile::portable_recipe();
        crate::test_execution_profile::dynamic_region_route(["ordered_f64_recurrence"]);
    }
    Some(Ok(result))
}

fn iteration_slots_are_private(slots: &[u16], index_slot: u16) -> Option<()> {
    (slots.is_empty() || slots == [index_slot]).then_some(())
}

fn execute_repeated(
    repeated: RepeatedRecurrence,
    dst: u16,
    registers: &mut crate::register_file::RegisterFile,
    body: CodeView<'_>,
) -> Option<Result<crate::completion::Completion, crate::execute::VmError>> {
    let result = crate::locals::with_current_ref(|environment| {
        let environment = environment?;
        execute_repeated_in(&repeated, environment, dst, registers)
    })?;
    crate::execution_trace::stencil_observation(body, 0, "repeated_ordered_f64_recurrence", true);
    crate::execution_trace::event(crate::execution_trace::Event::LeafHit);
    #[cfg(test)]
    {
        crate::test_execution_profile::portable_recipe();
        crate::test_execution_profile::dynamic_region_route(["repeated_ordered_f64_recurrence"]);
    }
    Some(Ok(result))
}

fn execute_repeated_in(
    repeated: &RepeatedRecurrence,
    environment: &crate::environment::Environment,
    dst: u16,
    registers: &mut crate::register_file::RegisterFile,
) -> Option<crate::completion::Completion> {
    let pass_end = end(repeated.pass_bound, repeated.pass_inclusive, environment)?;
    bounded_count(repeated.pass_start, pass_end)?;
    let recurrence_end = repeated.recurrence.end(environment)?;
    bounded_count(repeated.recurrence.start, recurrence_end)?;
    validate_accesses(&repeated.recurrence, environment, recurrence_end)?;
    for pass in repeated.pass_start..pass_end {
        let last = execute_range(&repeated.recurrence, environment, recurrence_end)?;
        let last = last_value(last);
        crate::execute::write_value(registers, repeated.inner_dst, last.clone());
        crate::execute::write_value(registers, repeated.body_dst, last);
        environment.set(repeated.pass_slot, Value::Number(f64::from(pass + 1)));
    }
    crate::execute::write_value(registers, dst, Value::Undefined);
    Some(crate::completion::Completion::Normal)
}

fn execute(
    recurrence: &OrderedRecurrence,
    environment: &crate::environment::Environment,
    dst: u16,
    registers: &mut crate::register_file::RegisterFile,
) -> Option<crate::completion::Completion> {
    let end = recurrence.end(environment)?;
    bounded_count(recurrence.start, end)?;
    validate_accesses(recurrence, environment, end)?;
    let last = execute_range(recurrence, environment, end)?;
    crate::execute::write_value(registers, dst, last_value(last));
    Some(crate::completion::Completion::Normal)
}

fn execute_range(
    recurrence: &OrderedRecurrence,
    environment: &crate::environment::Environment,
    end: i32,
) -> Option<Option<f64>> {
    let mut index = recurrence.start;
    let mut last = None;
    while index < end {
        let value = evaluate(&recurrence.value, environment, index)?;
        store(recurrence.destination, environment, index, value).then_some(())?;
        last = Some(value);
        index += 1;
    }
    environment.set(recurrence.index_slot, Value::Number(f64::from(end)));
    Some(last)
}

fn last_value(value: Option<f64>) -> Value {
    value.map_or(Value::Undefined, Value::Number)
}

fn bounded_count(start: i32, end: i32) -> Option<usize> {
    let count = usize::try_from(end.checked_sub(start)?).ok()?;
    (count <= MAX_ORDERED_RECURRENCE_ELEMENTS).then_some(count)
}

fn validate_accesses(
    recurrence: &OrderedRecurrence,
    environment: &crate::environment::Environment,
    end: i32,
) -> Option<()> {
    let mut loads = Vec::new();
    collect_loads(&recurrence.value, &mut loads);
    loads.push(recurrence.destination);
    loads
        .into_iter()
        .try_for_each(|load| validate_access(load, environment, recurrence.start, end))
}

fn collect_loads(expr: &NumericExpr, loads: &mut Vec<IndexedLoad>) {
    match expr {
        NumericExpr::Load(load) => loads.push(*load),
        NumericExpr::Add(left, right) => {
            collect_loads(left, loads);
            collect_loads(right, loads);
        }
        NumericExpr::Multiply(value, _) | NumericExpr::Divide(value, _) => {
            collect_loads(value, loads)
        }
        NumericExpr::Local(_) | NumericExpr::Constant(_) | NumericExpr::Index { .. } => {}
    }
}

fn validate_access(
    load: IndexedLoad,
    environment: &crate::environment::Environment,
    start: i32,
    end: i32,
) -> Option<()> {
    if start == end {
        return Some(());
    }
    let first = usize::try_from(start.checked_add(load.offset)?).ok()?;
    let last = usize::try_from(end.checked_sub(1)?.checked_add(load.offset)?).ok()?;
    let value = slot_value(environment, load.array_slot);
    let length = numeric_length(&value)?;
    (first < length && last < length).then_some(())
}

fn numeric_length(value: &Value) -> Option<usize> {
    match value {
        Value::Array(array) if array.is_plain_dense_access() && array.is_dense_numeric_data() => {
            Some(array.logical_len())
        }
        Value::Float64Array(array)
            if !crate::arrays::typed_array_is_detached(value)
                && !crate::typed_array_prototype::is_out_of_bounds(value) =>
        {
            Some(array.logical_len())
        }
        _ => None,
    }
}

impl OrderedRecurrence {
    fn end(&self, environment: &crate::environment::Environment) -> Option<i32> {
        end(self.bound, self.inclusive, environment)
    }
}

fn end(
    bound: Bound,
    inclusive: bool,
    environment: &crate::environment::Environment,
) -> Option<i32> {
    let bound = match bound {
        Bound::Constant(value) => value,
        Bound::Local { slot, offset } => {
            exact_i32_value(environment.get_number(slot)?)?.checked_add(offset)?
        }
    };
    bound.checked_add(i32::from(inclusive))
}

fn evaluate(
    expr: &NumericExpr,
    environment: &crate::environment::Environment,
    index: i32,
) -> Option<f64> {
    match expr {
        NumericExpr::Load(load) => load_value(*load, environment, index),
        NumericExpr::Add(left, right) => {
            Some(evaluate(left, environment, index)? + evaluate(right, environment, index)?)
        }
        NumericExpr::Multiply(value, factor) => Some(evaluate(value, environment, index)? * factor),
        NumericExpr::Divide(value, divisor) => Some(evaluate(value, environment, index)? / divisor),
        NumericExpr::Constant(value) => Some(*value),
        NumericExpr::Local(_) | NumericExpr::Index { .. } => None,
    }
}

fn load_value(
    load: IndexedLoad,
    environment: &crate::environment::Environment,
    index: i32,
) -> Option<f64> {
    let index = usize::try_from(index.checked_add(load.offset)?).ok()?;
    match slot_value(environment, load.array_slot) {
        Value::Array(array) if array.is_plain_dense_access() => array.dense_number_at(index),
        Value::Float64Array(array)
            if !crate::arrays::typed_array_is_detached(&Value::Float64Array(array.clone())) =>
        {
            array.get(index)
        }
        _ => None,
    }
}

fn store(
    load: IndexedLoad,
    environment: &crate::environment::Environment,
    index: i32,
    value: f64,
) -> bool {
    let Some(index) = index
        .checked_add(load.offset)
        .and_then(|value| usize::try_from(value).ok())
    else {
        return false;
    };
    match slot_value(environment, load.array_slot) {
        Value::Array(array) if array.is_plain_dense_access() => {
            array.set_existing_f64(index, value)
        }
        Value::Float64Array(array)
            if !crate::arrays::typed_array_is_detached(&Value::Float64Array(array.clone())) =>
        {
            array.set(index, value)
        }
        _ => false,
    }
}

fn slot_value(environment: &crate::environment::Environment, slot: u16) -> Value {
    crate::locals::resolved_replacement(environment.get(slot))
}
