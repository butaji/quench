macro_rules! advect_load_facts {
    ($code:ident; $($name:ident @ $pc:literal),+ $(,)?) => {
        $(let (_, $name) = recognized_static_load($code, $pc)?;)+
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

impl AdvectFact {
    fn recognize(code: crate::machine::CodeView<'_>, counter: u16) -> Option<Self> {
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
) -> Option<crate::completion::Completion> {
    (loop_fact.comparison == crate::ops::BinaryOp::LessEqual && loop_fact.step == 1.0)
        .then_some(())?;
    let fact = AdvectFact::recognize(body, loop_fact.slot)?;
    let environment = crate::locals::current();
    let mut counter = environment.get_number(loop_fact.slot)?;
    let bound = loop_fact.bound.number(&environment)?;
    let iterations = kernel_iteration_count(loop_fact, counter, bound)?;
    if iterations == 0 { return Some(crate::completion::Completion::Normal); }
    let mut pos = kernel_index(environment.get_number(fact.pos)?)?;
    let scalars = AdvectScalars::load(&environment, fact)?;
    let Value::Array(d) = environment.get(fact.d) else { return None };
    let Value::Array(d0) = environment.get(fact.d0) else { return None };
    let Value::Array(u) = environment.get(fact.u) else { return None };
    let Value::Array(v) = environment.get(fact.v) else { return None };
    (!std::rc::Rc::ptr_eq(&d, &d0)
        && !std::rc::Rc::ptr_eq(&d, &u)
        && !std::rc::Rc::ptr_eq(&d, &v))
        .then_some(())?;
    validate_advect_arrays(&d, &d0, &u, &v, pos, iterations, scalars)?;
    let d_words = d.numeric_cells()?;
    let d0_words = d0.numeric_cells()?;
    let u_words = u.numeric_cells()?;
    let v_words = v.numeric_cells()?;
    let first = pos.checked_add(1)?;
    let last = pos.checked_add(iterations)?;
    u_words
        .get(first..=last)?
        .iter()
        .chain(v_words.get(first..=last)?.iter())
        .all(|word| word.get().is_finite())
        .then_some(())?;
    let final_values = execute_advect(
        &d_words, &d0_words, &u_words, &v_words, &mut pos, &mut counter,
        iterations, loop_fact.step, scalars,
    )?;
    final_values.flush(&environment, fact, pos, counter, loop_fact.slot);
    Some(crate::completion::Completion::Normal)
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
    d: &[std::cell::Cell<f64>],
    d0: &[std::cell::Cell<f64>],
    u: &[std::cell::Cell<f64>],
    v: &[std::cell::Cell<f64>],
    pos: &mut usize,
    counter: &mut f64,
    iterations: usize,
    step: f64,
    scalars: AdvectScalars,
) -> Option<AdvectFinal> {
    let mut final_values = AdvectFinal::default();
    for _ in 0..iterations {
        *pos += 1;
        let x = (*counter - scalars.wdt * u[*pos].get()).clamp(0.5, scalars.wp5);
        let y = (scalars.j - scalars.hdt * v[*pos].get()).clamp(0.5, scalars.hp5);
        final_values = interpolate_advect(d0, x, y, scalars.row_size)?;
        d[*pos].set(final_values.value);
        *counter += step;
    }
    Some(final_values)
}

fn interpolate_advect(
    d0: &[std::cell::Cell<f64>], x: f64, y: f64, row_size: usize,
) -> Option<AdvectFinal> {
    let i0 = kernel_index(x.trunc())?;
    let j0 = kernel_index(y.trunc())?;
    let (i1, j1) = (i0.checked_add(1)?, j0.checked_add(1)?);
    let (s1, t1) = (x - i0 as f64, y - j0 as f64);
    let (s0, t0) = (1.0 - s1, 1.0 - t1);
    let (row1, row2) = (j0.checked_mul(row_size)?, j1.checked_mul(row_size)?);
    let at = |i: usize, row: usize| d0.get(i.checked_add(row)?).map(std::cell::Cell::get);
    let value = s0 * (t0 * at(i0, row1)? + t1 * at(i0, row2)?)
        + s1 * (t0 * at(i1, row1)? + t1 * at(i1, row2)?);
    Some(AdvectFinal { x, y, i0, i1, j0, j1, s1, s0, t1, t0, row1, row2, value })
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
