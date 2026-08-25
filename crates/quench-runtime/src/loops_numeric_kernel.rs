use crate::value::Value;

#[derive(Clone, Copy)]
struct LinearSolveFact {
    x: u16,
    x0: u16,
    current: u16,
    last: u16,
    next: u16,
    last_x: u16,
    a: u16,
    inv_c: u16,
}

#[derive(Clone, Copy)]
struct LinearSolveRowsFact {
    inner: LinearSolveFact,
    outer_slot: u16,
    row_size: u16,
    inner_loop: CountedForFact,
}

impl LinearSolveFact {
    fn recognize(code: crate::machine::CodeView<'_>, _counter: u16) -> Option<Self> {
        (code.len() == 28).then_some(())?;
        let (x_reg, x) = recognized_static_load(code, 0)?;
        let x_copy = kernel_move(code, 1, x_reg)?;
        let (current_reg, current) = recognized_static_load(code, 2)?;
        let x_object = kernel_move(code, 3, x_copy)?;
        let current_key = current_reg;
        let (x0_reg, x0) = recognized_static_load(code, 4)?;
        let (x0_key, current_again) = recognized_static_load(code, 5)?;
        (current_again == current).then_some(())?;
        let first = kernel_alu(code, 6, crate::ir::Opcode::AGetI, x0_reg, x0_key)?;
        let (a_reg, a) = recognized_static_load(code, 7)?;
        let (last_x_reg, last_x) = recognized_static_load(code, 8)?;
        let (right_array, right_x) = recognized_static_load(code, 9)?;
        (right_x == x).then_some(())?;
        let right_key = kernel_update(code, 10, Some(current))?;
        let right = kernel_alu(code, 11, crate::ir::Opcode::AGetI, right_array, right_key)?;
        let sum_right = kernel_alu(code, 12, crate::ir::Opcode::Add, last_x_reg, right)?;
        let (last_array, last_x_slot) = recognized_static_load(code, 13)?;
        (last_x_slot == x).then_some(())?;
        let last_key = kernel_update(code, 14, None)?;
        let last = kernel_update_slot(code, 14)?;
        let last_value = kernel_alu(code, 15, crate::ir::Opcode::AGetI, last_array, last_key)?;
        let sum_last = kernel_alu(code, 16, crate::ir::Opcode::Add, sum_right, last_value)?;
        let (next_array, next_x_slot) = recognized_static_load(code, 17)?;
        (next_x_slot == x).then_some(())?;
        let next_key = kernel_update(code, 18, None)?;
        let next = kernel_update_slot(code, 18)?;
        let next_value = kernel_alu(code, 19, crate::ir::Opcode::AGetI, next_array, next_key)?;
        let sum = kernel_alu(code, 20, crate::ir::Opcode::Add, sum_last, next_value)?;
        let scaled = kernel_alu(code, 21, crate::ir::Opcode::Mul, a_reg, sum)?;
        let added = kernel_alu(code, 22, crate::ir::Opcode::Add, first, scaled)?;
        let (inv_c_reg, inv_c) = recognized_static_load(code, 23)?;
        let result = kernel_alu(code, 24, crate::ir::Opcode::Mul, added, inv_c_reg)?;
        kernel_store(code, 25, x_object, current_key, result)?;
        kernel_checked_store(code, 26, last_x, result)?;
        kernel_move(code, 27, result)?;
        Some(Self {
            x, x0, current, last, next, last_x, a, inv_c,
        })
    }
}

fn kernel_move(code: crate::machine::CodeView<'_>, pc: usize, source: u16) -> Option<u16> {
    let op = code.instruction(pc)?;
    (op.opcode == crate::ir::Opcode::Move && op.b == source).then_some(op.a)
}

fn kernel_alu(code: crate::machine::CodeView<'_>, pc: usize, opcode: crate::ir::Opcode, lhs: u16, rhs: u16) -> Option<u16> {
    let op = code.instruction(pc)?;
    (op.opcode == opcode && op.b == lhs && op.c == rhs).then_some(op.a)
}

fn kernel_update(code: crate::machine::CodeView<'_>, pc: usize, slot: Option<u16>) -> Option<u16> {
    let op = code.instruction(pc)?;
    (op.opcode == crate::ir::Opcode::UpdateLocal && op.flags == 0 && slot.is_none_or(|slot| op.c == slot)).then_some(op.b)
}

fn kernel_update_slot(code: crate::machine::CodeView<'_>, pc: usize) -> Option<u16> {
    let op = code.instruction(pc)?;
    (op.opcode == crate::ir::Opcode::UpdateLocal && op.flags == 0).then_some(op.c)
}

fn kernel_store(code: crate::machine::CodeView<'_>, pc: usize, array: u16, key: u16, value: u16) -> Option<()> {
    let op = code.instruction(pc)?;
    (op.opcode == crate::ir::Opcode::ASetI && op.a == array && op.b == key && op.c == value).then_some(())
}

fn kernel_checked_store(
    code: crate::machine::CodeView<'_>,
    pc: usize,
    slot: u16,
    value: u16,
) -> Option<()> {
    let instruction = code.instruction(pc)?;
    (matches!(instruction.opcode, crate::ir::Opcode::StoreLocal | crate::ir::Opcode::StoreLocalChecked)
        && instruction.a == slot
        && instruction.b == value)
        .then_some(())
}

fn run_linear_solve_kernel(
    loop_fact: CountedForFact,
    body: crate::machine::CodeView<'_>,
    loop_shape: u64,
) -> Option<crate::completion::Completion> {
    if let Some(completion) = run_linear_solve_rows_kernel(loop_fact, body, loop_shape) {
        return Some(completion);
    }
    (loop_fact.timing == CountedStepTiming::AfterBody).then_some(())?;
    let fact = LinearSolveFact::recognize(body, loop_fact.slot)?;
    if let CountedBound::Slot(bound) = loop_fact.bound {
        (![loop_fact.slot, fact.current, fact.last, fact.next, fact.last_x].contains(&bound))
            .then_some(())?;
    }
    let environment = crate::locals::current();
    let Value::Array(x) = crate::locals::resolved_replacement(environment.get(fact.x)) else { return None };
    let Value::Array(x0) = crate::locals::resolved_replacement(environment.get(fact.x0)) else { return None };
    let mut counter = environment.get_number(loop_fact.slot)?;
    let mut current = kernel_index(environment.get_number(fact.current)?)?;
    let mut last = kernel_index(environment.get_number(fact.last)?)?;
    let mut next = kernel_index(environment.get_number(fact.next)?)?;
    let mut last_x = environment.get_number(fact.last_x)?;
    let a = environment.get_number(fact.a)?;
    let inv_c = environment.get_number(fact.inv_c)?;
    let bound = loop_fact.bound.number(&environment)?;
    let iterations = unit_iteration_count(loop_fact, counter, bound)?;
    if iterations == 0 { return Some(crate::completion::Completion::Normal); }
    let end = iterations - 1;
    let max_x = [
        current.checked_add(end)?,
        current.checked_add(iterations)?,
        last.checked_add(iterations)?,
        next.checked_add(iterations)?,
    ]
    .into_iter()
    .max()?;
    let max_x0 = current.checked_add(end)?;
    (x.is_packed_ordinary() && x0.is_packed_ordinary() && max_x < x.header_length() && max_x0 < x0.header_length()).then_some(())?;
    let x_values = x.numeric_cells()?;
    let x0_values = x0.numeric_cells()?;
    crate::execution_trace::numeric_kernel_iterations(
        "counted_packed_f64_jacobi",
        loop_shape,
        iterations,
        4,
        1,
    );
    for _ in 0..iterations {
        let value = (x0_values[current].get() + a * (last_x + x_values[current + 1].get() + x_values[last + 1].get() + x_values[next + 1].get())) * inv_c;
        x_values[current].set(value);
        last_x = value;
        current += 1;
        last += 1;
        next += 1;
        counter += loop_fact.step;
    }
    environment.set(fact.current, Value::Number(current as f64));
    environment.set(fact.last, Value::Number(last as f64));
    environment.set(fact.next, Value::Number(next as f64));
    environment.set(fact.last_x, Value::Number(last_x));
    environment.set(loop_fact.slot, Value::Number(counter));
    Some(crate::completion::Completion::Normal)
}

fn run_linear_solve_rows_kernel(
    outer_loop: CountedForFact,
    body: crate::machine::CodeView<'_>,
    outer_shape: u64,
) -> Option<crate::completion::Completion> {
    let (fact, inner_body) = recognize_linear_solve_rows(outer_loop, body)?;
    let environment = crate::locals::current();
    let outer_start = kernel_index(environment.get_number(fact.outer_slot)?)?;
    let outer_bound = outer_loop.bound.number(&environment)?;
    let rows = unit_iteration_count(outer_loop, outer_start as f64, outer_bound)?;
    let inner_start = initialized_number(inner_body.0, fact.inner_loop.slot)?;
    let inner_bound = fact.inner_loop.bound.number(&environment)?;
    let columns = unit_iteration_count(fact.inner_loop, inner_start, inner_bound)?;
    (rows != 0 && columns != 0 && outer_start >= 1).then_some(())?;
    let row_size = kernel_index(environment.get_number(fact.row_size)?)?;
    let Value::Array(x) = crate::locals::resolved_replacement(environment.get(fact.inner.x)) else { return None };
    let Value::Array(x0) = crate::locals::resolved_replacement(environment.get(fact.inner.x0)) else { return None };
    (!std::rc::Rc::ptr_eq(&x, &x0)).then_some(())?;
    validate_linear_solve_rows(&x, &x0, outer_start, rows, row_size, columns)?;
    let mut x_values = x.numeric_kernel_words_mut()?;
    let x0_values = x0.numeric_kernel_words()?;
    let a = environment.get_number(fact.inner.a)?;
    let inv_c = environment.get_number(fact.inner.inv_c)?;
    let final_state = execute_linear_solve_rows(
        &mut x_values, &x0_values, outer_start, rows, row_size, columns, a, inv_c,
    );
    flush_linear_solve_rows(&environment, fact, outer_start + rows, inner_start, columns, final_state);
    trace_linear_solve_rows(outer_shape, inner_body.1, rows, columns);
    Some(crate::completion::Completion::Normal)
}

fn recognize_linear_solve_rows(
    outer_loop: CountedForFact,
    body: crate::machine::CodeView<'_>,
) -> Option<(LinearSolveRowsFact, (crate::machine::CodeView<'_>, crate::machine::CodeView<'_>))> {
    (body.len() == 25 && outer_loop.timing == CountedStepTiming::AfterBody
        && outer_loop.step == 1.0).then_some(())?;
    same_static_slot(body, &[0, 6, 10], outer_loop.slot)?;
    let row_size = same_static_slots(body, &[3, 7, 13])?;
    let last = initialized_compact_slot(body, 5, body.instruction(4)?.a)?;
    let current = initialized_compact_slot(body, 9, body.instruction(8)?.a)?;
    let next = initialized_compact_slot(body, 15, body.instruction(14)?.a)?;
    let last_x = initialized_compact_slot(body, 19, body.instruction(18)?.a)?;
    let crate::ops::Op::Loop { init, test, body: inner, update, post_test, .. } = body.cold_at(23)? else { return None };
    (!*post_test).then_some(())?;
    let (init, test, inner, update) = (init.code()?, test.code()?, inner.code()?, update.code()?);
    let inner_loop = CountedForFact::recognize(test, update)?;
    let fact = LinearSolveFact::recognize(inner, inner_loop.slot)?;
    (fact.last == last && fact.current == current && fact.next == next && fact.last_x == last_x)
        .then_some(())?;
    Some((LinearSolveRowsFact { inner: fact, outer_slot: outer_loop.slot, row_size, inner_loop }, (init, inner)))
}

fn initialized_compact_slot(
    code: crate::machine::CodeView<'_>, pc: usize, source: u16,
) -> Option<u16> {
    let op = code.instruction(pc)?;
    (op.opcode == crate::ir::Opcode::InitLocal && op.b == source).then_some(op.a)
}

fn initialized_number(code: crate::machine::CodeView<'_>, slot: u16) -> Option<f64> {
    for pc in 0..code.len() {
        let Some((register, crate::ops::Constant::Number(number))) = code.constant_at(pc) else { continue };
        for store_pc in pc + 1..code.len() {
            let store = code.instruction(store_pc)?;
            if matches!(store.opcode, crate::ir::Opcode::InitLocal | crate::ir::Opcode::StoreLocal)
                && store.a == slot && store.b == register { return Some(*number); }
        }
    }
    None
}

fn validate_linear_solve_rows(
    x: &crate::value::ArrayData,
    x0: &crate::value::ArrayData,
    start: usize,
    rows: usize,
    row_size: usize,
    columns: usize,
) -> Option<()> {
    let last_row = start.checked_add(rows)?.checked_sub(1)?;
    let max_x = last_row.checked_add(1)?.checked_mul(row_size)?.checked_add(columns)?;
    let max_x0 = last_row.checked_mul(row_size)?.checked_add(columns)?;
    (x.is_packed_ordinary() && x0.is_packed_ordinary()
        && max_x < x.header_length() && max_x0 < x0.header_length()).then_some(())
}

#[derive(Clone, Copy)]
struct LinearSolveFinal { last: usize, current: usize, next: usize, last_x: f64 }

fn execute_linear_solve_rows(
    x: &mut [f64],
    x0: &[f64],
    start: usize,
    rows: usize,
    row_size: usize,
    columns: usize,
    a: f64,
    inv_c: f64,
) -> LinearSolveFinal {
    let mut final_state = LinearSolveFinal { last: 0, current: 0, next: 0, last_x: 0.0 };
    for row in start..start + rows {
        let (mut last, mut current, mut next) =
            ((row - 1) * row_size, row * row_size + 1, (row + 1) * row_size);
        let mut last_x = x[row * row_size];
        for _ in 0..columns {
            let value = (x0[current]
                + a * (last_x + x[current + 1] + x[last + 1] + x[next + 1]))
                * inv_c;
            x[current] = value;
            last_x = value;
            current += 1;
            last += 1;
            next += 1;
        }
        final_state = LinearSolveFinal { last, current, next, last_x };
    }
    final_state
}

fn flush_linear_solve_rows(
    environment: &crate::environment::Environment,
    fact: LinearSolveRowsFact,
    outer_end: usize,
    inner_start: f64,
    columns: usize,
    final_state: LinearSolveFinal,
) {
    let values = [
        (fact.outer_slot, outer_end as f64),
        (fact.inner_loop.slot, inner_start + columns as f64),
        (fact.inner.last, final_state.last as f64),
        (fact.inner.current, final_state.current as f64),
        (fact.inner.next, final_state.next as f64),
        (fact.inner.last_x, final_state.last_x),
    ];
    for (slot, value) in values { environment.set(slot, Value::Number(value)); }
}

fn trace_linear_solve_rows(
    outer_shape: u64,
    inner_body: crate::machine::CodeView<'_>,
    rows: usize,
    columns: usize,
) {
    let inner_shape = crate::execution_trace::loop_shape(inner_body);
    crate::execution_trace::loop_shape_entries(inner_shape, rows.saturating_sub(1));
    crate::execution_trace::counted_loop_iterations(outer_shape, rows);
    crate::execution_trace::numeric_kernel_iterations(
        "counted_packed_f64_jacobi", inner_shape, rows.saturating_mul(columns), 4, 1,
    );
}

fn kernel_index(value: f64) -> Option<usize> {
    (value >= 0.0 && value <= usize::MAX as f64 && value.fract() == 0.0).then_some(value as usize)
}

fn unit_iteration_count(fact: CountedForFact, index: f64, bound: f64) -> Option<usize> {
    (fact.timing == CountedStepTiming::AfterBody
        && fact.step == 1.0
        && index.is_finite()
        && bound.is_finite())
    .then_some(())?;
    let count = match fact.comparison {
        crate::ops::BinaryOp::LessThan if bound > index => (bound - index).ceil(),
        crate::ops::BinaryOp::LessEqual if bound >= index => (bound - index).floor() + 1.0,
        crate::ops::BinaryOp::LessThan | crate::ops::BinaryOp::LessEqual => 0.0,
        _ => return None,
    };
    (count <= u32::MAX as f64).then(|| count as usize)
}

fn kernel_iteration_count(fact: CountedForFact, mut index: f64, bound: f64) -> Option<usize> {
    let mut count = 0usize;
    loop {
        if fact.timing == CountedStepTiming::BeforeTest {
            index += fact.step;
        }
        if !counted_comparison(fact.comparison, index, bound) {
            break;
        }
        count = count.checked_add(1)?;
        (count <= u32::MAX as usize).then_some(())?;
        if fact.timing == CountedStepTiming::AfterBody {
            index += fact.step;
        }
    }
    Some(count)
}
