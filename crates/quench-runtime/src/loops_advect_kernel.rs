macro_rules! advect_load_facts {
    ($code:ident; $($name:ident @ $pc:literal),+ $(,)?) => {
        $(let (_, $name) = recognized_static_load($code, $pc)?;)+
    };
}

macro_rules! advect_init_facts {
    ($code:ident; $($name:ident @ $init:literal <- $result:literal),+ $(,)?) => {
        $(let $name = compact_initialization_slot($code, $init, result_register($code, $result)?)?;)+
    };
}

#[derive(Clone, Copy)]
struct AdvectFact {
    d: u16, d0: u16, u: u16, v: u16,
    row_size: u16, wdt: u16, hdt: u16, wp5: u16, hp5: u16,
    j: u16, pos: u16, x: u16, y: u16,
    i0: u16, i1: u16, j0: u16, j1: u16,
    s1: u16, s0: u16, t1: u16, t0: u16, row1: u16, row2: u16,
}

#[derive(Clone, Copy)]
struct AdvectRowsFact {
    inner: AdvectFact,
    outer_slot: u16,
    inner_loop: CountedForFact,
}

impl AdvectFact {
    fn recognize(code: crate::machine::CodeView<'_>, counter: u16) -> Option<Self> {
        Self::recognize_compact(code, counter).or_else(|| Self::recognize_legacy(code, counter))
    }

    fn recognize_compact(code: crate::machine::CodeView<'_>, counter: u16) -> Option<Self> {
        (code.len() == 109).then_some(())?;
        advect_load_facts!(code;
            i @ 0, wdt @ 1, u @ 2, j @ 8, hdt @ 9, v @ 10,
            d @ 68, d0 @ 74, row_size @ 61
        );
        (i == counter).then_some(())?;
        let pos = kernel_update_slot(code, 3)?;
        advect_init_facts!(code;
            x @ 7 <- 6, y @ 15 <- 14, i0 @ 25 <- 24, i1 @ 29 <- 28,
            j0 @ 39 <- 38, j1 @ 43 <- 42, s1 @ 47 <- 46, s0 @ 51 <- 50,
            t1 @ 55 <- 54, t0 @ 59 <- 58, row1 @ 63 <- 62, row2 @ 67 <- 66
        );
        validate_compact_advect_anchors(code, pos, x, y, d, d0)?;
        let wp5 = branch_bound_slot(code, 20, x)?;
        let hp5 = branch_bound_slot(code, 34, y)?;
        Some(Self { d, d0, u, v, row_size, wdt, hdt, wp5, hp5, j, pos, x, y, i0, i1, j0, j1, s1, s0, t1, t0, row1, row2 })
    }

    fn recognize_legacy(code: crate::machine::CodeView<'_>, counter: u16) -> Option<Self> {
        (code.len() == 121).then_some(())?;
        advect_load_facts!(code;
            i @ 0, wdt @ 1, u @ 2, j @ 9, hdt @ 10, v @ 11,
            d @ 80, d0 @ 86, row_size @ 71
        );
        (i == counter).then_some(())?;
        let pos = kernel_update_slot(code, 3)?;
        let x = initialization_slot(code, 7, 8, result_register(code, 6)?)?;
        let y = initialization_slot(code, 16, 17, result_register(code, 15)?)?;
        let i0 = initialization_slot(code, 27, 28, result_register(code, 26)?)?;
        let i1 = initialization_slot(code, 32, 33, result_register(code, 31)?)?;
        let j0 = initialization_slot(code, 43, 44, result_register(code, 42)?)?;
        let j1 = initialization_slot(code, 48, 49, result_register(code, 47)?)?;
        let s1 = initialization_slot(code, 53, 54, result_register(code, 52)?)?;
        let s0 = initialization_slot(code, 58, 59, result_register(code, 57)?)?;
        let t1 = initialization_slot(code, 63, 64, result_register(code, 62)?)?;
        let t0 = initialization_slot(code, 68, 69, result_register(code, 67)?)?;
        let row1 = initialization_slot(code, 73, 74, result_register(code, 72)?)?;
        let row2 = initialization_slot(code, 78, 79, result_register(code, 77)?)?;
        validate_advect_anchors(code, pos, x, y, d, d0)?;
        let wp5 = branch_bound_slot(code, 22, x)?;
        let hp5 = branch_bound_slot(code, 38, y)?;
        Some(Self { d, d0, u, v, row_size, wdt, hdt, wp5, hp5, j, pos, x, y, i0, i1, j0, j1, s1, s0, t1, t0, row1, row2 })
    }
}

fn compact_initialization_slot(
    code: crate::machine::CodeView<'_>,
    pc: usize,
    source: u16,
) -> Option<u16> {
    let instruction = code.instruction(pc)?;
    (instruction.opcode == crate::ir::Opcode::InitLocal && instruction.b == source)
        .then_some(instruction.a)
}

fn validate_compact_advect_anchors(
    code: crate::machine::CodeView<'_>, pos: u16, x: u16, y: u16, d: u16, d0: u16,
) -> Option<()> {
    (kernel_update_slot(code, 3)? == pos).then_some(())?;
    (recognized_static_load(code, 22)?.1 == x).then_some(())?;
    (recognized_static_load(code, 36)?.1 == y).then_some(())?;
    (recognized_static_load(code, 68)?.1 == d).then_some(())?;
    (recognized_static_load(code, 74)?.1 == d0).then_some(())?;
    (code.instruction(107)?.opcode == crate::ir::Opcode::ASetI).then_some(())
}

fn result_register(code: crate::machine::CodeView<'_>, pc: usize) -> Option<u16> {
    let instruction = code.instruction(pc)?;
    if !instruction.opcode.is_slow() { return Some(instruction.a); }
    match code.cold(instruction)? {
        Op::Binary { dst, .. } | Op::Const { dst, .. } => Some(*dst),
        _ => None,
    }
}

fn initialization_slot(
    code: crate::machine::CodeView<'_>,
    resolve_pc: usize,
    initialize_pc: usize,
    source: u16,
) -> Option<u16> {
    let Op::ResolveBindingTarget { dst, name } = code.cold_at(resolve_pc)? else { return None };
    let Op::InitializeResolvedBinding { target, slot, name: initialized, src } = code.cold_at(initialize_pc)? else { return None };
    (*target == *dst && initialized == name && *src == source).then_some(*slot)
}

fn branch_bound_slot(code: crate::machine::CodeView<'_>, pc: usize, value: u16) -> Option<u16> {
    let Op::Branch { then_ops, else_ops, .. } = code.cold_at(pc)? else { return None };
    [then_ops.code(), else_ops.code()]
        .into_iter()
        .flatten()
        .find_map(|branch| distinct_branch_slot(branch, value))
}

fn distinct_branch_slot(code: crate::machine::CodeView<'_>, value: u16) -> Option<u16> {
    for pc in 0..code.len() {
        if let Some((_, slot)) = recognized_static_load(code, pc) {
            if slot != value { return Some(slot); }
        }
        if matches!(code.cold_at(pc), Some(Op::Branch { .. })) {
            if let Some(slot) = branch_bound_slot(code, pc, value) { return Some(slot); }
        }
    }
    None
}

fn validate_advect_anchors(
    code: crate::machine::CodeView<'_>, pos: u16, x: u16, y: u16, d: u16, d0: u16,
) -> Option<()> {
    let (_, loaded_pos) = recognized_static_load(code, 12)?;
    let (_, loaded_x) = recognized_static_load(code, 18)?;
    let (_, loaded_y) = recognized_static_load(code, 34)?;
    let (_, output) = recognized_static_load(code, 80)?;
    let (_, input) = recognized_static_load(code, 86)?;
    (loaded_pos == pos && loaded_x == x && loaded_y == y).then_some(())?;
    (output == d && input == d0).then_some(())?;
    (code.instruction(119)?.opcode == crate::ir::Opcode::ASetI).then_some(())
}

fn run_advect_kernel(
    loop_fact: CountedForFact,
    body: crate::machine::CodeView<'_>,
    loop_shape: u64,
) -> Option<crate::completion::Completion> {
    if let Some(completion) = run_advect_rows_kernel(loop_fact, body, loop_shape) {
        return Some(completion);
    }
    (loop_fact.comparison == crate::ops::BinaryOp::LessEqual && loop_fact.step == 1.0)
        .then_some(())?;
    let fact = AdvectFact::recognize(body, loop_fact.slot)?;
    let environment = crate::locals::current();
    let mut counter = environment.get_number(loop_fact.slot)?;
    let bound = loop_fact.bound.number(&environment)?;
    let iterations = unit_iteration_count(loop_fact, counter, bound)?;
    if iterations == 0 { return Some(crate::completion::Completion::Normal); }
    let mut pos = kernel_index(environment.get_number(fact.pos)?)?;
    let scalars = AdvectScalars::load(&environment, fact)?;
    let Value::Array(d) = crate::locals::resolved_replacement(environment.get(fact.d)) else { return None };
    let Value::Array(d0) = crate::locals::resolved_replacement(environment.get(fact.d0)) else { return None };
    let Value::Array(u) = crate::locals::resolved_replacement(environment.get(fact.u)) else { return None };
    let Value::Array(v) = crate::locals::resolved_replacement(environment.get(fact.v)) else { return None };
    (!std::rc::Rc::ptr_eq(&d, &d0)
        && !std::rc::Rc::ptr_eq(&d, &u)
        && !std::rc::Rc::ptr_eq(&d, &v))
        .then_some(())?;
    validate_advect_arrays(&d, &d0, &u, &v, pos, iterations, scalars)?;
    let mut d_words = d.numeric_kernel_words_mut()?;
    let d0_words = d0.numeric_kernel_words()?;
    let u_words = u.numeric_kernel_words()?;
    let v_words = v.numeric_kernel_words()?;
    let counter_start = kernel_index(counter)?;
    let counter_end = counter_start.checked_add(iterations)?;
    crate::execution_trace::numeric_kernel_iterations(
        "counted_packed_f64_advect",
        loop_shape,
        iterations,
        6,
        1,
    );
    let final_values = execute_advect(
        &mut d_words, &d0_words, &u_words, &v_words, &mut pos, counter_start,
        iterations, scalars,
    );
    counter = counter_end as f64;
    final_values.flush(&environment, fact, pos, counter, loop_fact.slot);
    Some(crate::completion::Completion::Normal)
}

fn run_advect_rows_kernel(
    outer_loop: CountedForFact,
    body: crate::machine::CodeView<'_>,
    outer_shape: u64,
) -> Option<crate::completion::Completion> {
    let (fact, inner_init, inner_body) = recognize_advect_rows(outer_loop, body)?;
    let environment = crate::locals::current();
    let outer_start = kernel_index(environment.get_number(fact.outer_slot)?)?;
    let rows = unit_iteration_count(
        outer_loop, outer_start as f64, outer_loop.bound.number(&environment)?,
    )?;
    let inner_start = initialized_number(inner_init, fact.inner_loop.slot)?;
    let columns = unit_iteration_count(
        fact.inner_loop, inner_start, fact.inner_loop.bound.number(&environment)?,
    )?;
    (rows != 0 && columns != 0).then_some(())?;
    let mut scalars = AdvectScalars::load(&environment, fact.inner)?;
    let Value::Array(d) = crate::locals::resolved_replacement(environment.get(fact.inner.d)) else { return None };
    let Value::Array(d0) = crate::locals::resolved_replacement(environment.get(fact.inner.d0)) else { return None };
    let Value::Array(u) = crate::locals::resolved_replacement(environment.get(fact.inner.u)) else { return None };
    let Value::Array(v) = crate::locals::resolved_replacement(environment.get(fact.inner.v)) else { return None };
    (!std::rc::Rc::ptr_eq(&d, &d0)
        && !std::rc::Rc::ptr_eq(&d, &u)
        && !std::rc::Rc::ptr_eq(&d, &v)).then_some(())?;
    validate_advect_rows(&d, &d0, &u, &v, outer_start, rows, columns, scalars)?;
    let mut d_words = d.numeric_kernel_words_mut()?;
    let d0_words = d0.numeric_kernel_words()?;
    let u_words = u.numeric_kernel_words()?;
    let v_words = v.numeric_kernel_words()?;
    let counter_start = kernel_index(inner_start)?;
    let mut pos = 0;
    let mut final_values = AdvectFinal::default();
    for row in outer_start..outer_start + rows {
        pos = row * scalars.row_size;
        scalars.j = row as f64;
        final_values = execute_advect(
            &mut d_words, &d0_words, &u_words, &v_words, &mut pos,
            counter_start, columns, scalars,
        );
    }
    final_values.flush(
        &environment, fact.inner, pos, inner_start + columns as f64,
        fact.inner_loop.slot,
    );
    environment.set(fact.outer_slot, Value::Number((outer_start + rows) as f64));
    trace_advect_rows(outer_shape, inner_body, rows, columns);
    Some(crate::completion::Completion::Normal)
}

fn recognize_advect_rows(
    outer_loop: CountedForFact,
    body: crate::machine::CodeView<'_>,
) -> Option<(AdvectRowsFact, crate::machine::CodeView<'_>, crate::machine::CodeView<'_>)> {
    (body.len() == 7 && outer_loop.timing == CountedStepTiming::AfterBody
        && outer_loop.step == 1.0).then_some(())?;
    let (_, outer_slot) = recognized_static_load(body, 0)?;
    let (_, row_size) = recognized_static_load(body, 1)?;
    (outer_slot == outer_loop.slot).then_some(())?;
    let pos = compact_initialization_slot(body, 3, result_register(body, 2)?)?;
    let Op::Loop { init, test, body: inner, update, post_test, .. } = body.cold_at(5)? else { return None };
    (!*post_test).then_some(())?;
    let (init, test, inner, update) = (init.code()?, test.code()?, inner.code()?, update.code()?);
    let inner_loop = CountedForFact::recognize(test, update)?;
    let inner_fact = AdvectFact::recognize(inner, inner_loop.slot)?;
    (inner_fact.j == outer_slot && inner_fact.pos == pos && inner_fact.row_size == row_size)
        .then_some(())?;
    Some((AdvectRowsFact { inner: inner_fact, outer_slot, inner_loop }, init, inner))
}

fn validate_advect_rows(
    d: &crate::value::ArrayData,
    d0: &crate::value::ArrayData,
    u: &crate::value::ArrayData,
    v: &crate::value::ArrayData,
    start: usize,
    rows: usize,
    columns: usize,
    scalars: AdvectScalars,
) -> Option<()> {
    let last_row = start.checked_add(rows)?.checked_sub(1)?;
    let first_pos = start.checked_mul(scalars.row_size)?;
    let last_pos = last_row.checked_mul(scalars.row_size)?.checked_add(columns)?;
    validate_advect_arrays(
        d, d0, u, v, first_pos, last_pos.checked_sub(first_pos)?, scalars,
    )
}

fn trace_advect_rows(
    outer_shape: u64,
    inner_body: crate::machine::CodeView<'_>,
    rows: usize,
    columns: usize,
) {
    let inner_shape = crate::execution_trace::loop_shape(inner_body);
    crate::execution_trace::loop_shape_entries(inner_shape, rows.saturating_sub(1));
    crate::execution_trace::counted_loop_iterations(outer_shape, rows);
    crate::execution_trace::numeric_kernel_iterations(
        "counted_packed_f64_advect", inner_shape, rows.saturating_mul(columns), 6, 1,
    );
}

#[derive(Clone, Copy)]
struct AdvectScalars {
    j: f64, wdt: f64, hdt: f64, wp5: f64, hp5: f64, row_size: usize,
}

impl AdvectScalars {
    fn load(environment: &crate::environment::Environment, fact: AdvectFact) -> Option<Self> {
        let values = Self {
            j: environment.get_number(fact.j)?,
            wdt: environment.get_number(fact.wdt)?,
            hdt: environment.get_number(fact.hdt)?,
            wp5: environment.get_number(fact.wp5)?,
            hp5: environment.get_number(fact.hp5)?,
            row_size: kernel_index(environment.get_number(fact.row_size)?)?,
        };
        (values.j.is_finite()
            && values.wdt.is_finite()
            && values.hdt.is_finite()
            && values.wp5.is_finite()
            && values.hp5.is_finite()
            && values.wp5 >= 0.5
            && values.hp5 >= 0.5
            && values.row_size != 0)
            .then_some(values)
    }
}

fn validate_advect_arrays(
    d: &crate::value::ArrayData,
    d0: &crate::value::ArrayData,
    u: &crate::value::ArrayData,
    v: &crate::value::ArrayData,
    pos: usize,
    iterations: usize,
    scalars: AdvectScalars,
) -> Option<()> {
    let end = pos.checked_add(iterations)?;
    let max_i = kernel_index(scalars.wp5.trunc())?.checked_add(1)?;
    let max_j = kernel_index(scalars.hp5.trunc())?.checked_add(1)?;
    let input_end = max_j.checked_mul(scalars.row_size)?.checked_add(max_i)?;
    (d.is_packed_ordinary() && d0.is_packed_ordinary()).then_some(())?;
    (u.is_packed_ordinary() && v.is_packed_ordinary()).then_some(())?;
    (end < d.header_length() && end < u.header_length()).then_some(())?;
    (end < v.header_length() && input_end < d0.header_length()).then_some(())
}

fn execute_advect(
    d: &mut [f64],
    d0: &[f64],
    u: &[f64],
    v: &[f64],
    pos: &mut usize,
    counter_start: usize,
    iterations: usize,
    scalars: AdvectScalars,
) -> AdvectFinal {
    let mut final_values = AdvectFinal::default();
    for offset in 0..iterations {
        *pos += 1;
        let counter = (counter_start + offset) as f64;
        let x = (counter - scalars.wdt * u[*pos]).clamp(0.5, scalars.wp5);
        let y = (scalars.j - scalars.hdt * v[*pos]).clamp(0.5, scalars.hp5);
        final_values = interpolate_advect(d0, x, y, scalars.row_size);
        d[*pos] = final_values.value;
    }
    final_values
}

fn interpolate_advect(
    d0: &[f64], x: f64, y: f64, row_size: usize,
) -> AdvectFinal {
    let i0 = x as usize;
    let j0 = y as usize;
    let (i1, j1) = (i0 + 1, j0 + 1);
    let (s1, t1) = (x - i0 as f64, y - j0 as f64);
    let (s0, t0) = (1.0 - s1, 1.0 - t1);
    let (row1, row2) = (j0 * row_size, j1 * row_size);
    // SAFETY: `validate_advect_arrays` proves the maximum clamped row and
    // column, and finite velocity guards make the clamps and casts total.
    let at = |index: usize| unsafe { *d0.get_unchecked(index) };
    let value = s0 * (t0 * at(i0 + row1) + t1 * at(i0 + row2))
        + s1 * (t0 * at(i1 + row1) + t1 * at(i1 + row2));
    AdvectFinal { x, y, i0, i1, j0, j1, s1, s0, t1, t0, row1, row2, value }
}

#[derive(Clone, Copy, Default)]
struct AdvectFinal {
    x: f64, y: f64, i0: usize, i1: usize, j0: usize, j1: usize,
    s1: f64, s0: f64, t1: f64, t0: f64, row1: usize, row2: usize, value: f64,
}

impl AdvectFinal {
    fn flush(
        self,
        environment: &crate::environment::Environment,
        fact: AdvectFact,
        pos: usize,
        counter: f64,
        counter_slot: u16,
    ) {
        let numbers = [
            (fact.pos, pos as f64),
            (fact.x, self.x), (fact.y, self.y),
            (fact.i0, self.i0 as f64), (fact.i1, self.i1 as f64),
            (fact.j0, self.j0 as f64), (fact.j1, self.j1 as f64),
            (fact.s1, self.s1), (fact.s0, self.s0),
            (fact.t1, self.t1), (fact.t0, self.t0),
            (fact.row1, self.row1 as f64), (fact.row2, self.row2 as f64),
            (counter_slot, counter),
        ];
        for (slot, number) in numbers {
            environment.set(slot, Value::Number(number));
        }
    }
}
