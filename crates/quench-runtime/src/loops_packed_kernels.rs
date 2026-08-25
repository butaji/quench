#[derive(Clone, Copy)]
enum PackedLoopFact {
    AddFields { x: u16, source: u16, scale: u16 },
    Fill3 { arrays: [u16; 3] },
    Boundary(BoundaryFact),
    CopyRow(CopyRowFact),
    Divergence(DivergenceFact),
    Projection(ProjectionFact),
}

macro_rules! compact_shape {
    ($code:ident; $($pc:literal => $opcode:ident),+ $(,)?) => {{
        $(compact_fact_opcode!($code, $pc, $opcode);)+
    }};
}

macro_rules! compact_fact_opcode {
    ($code:ident, $pc:literal, StaticLoad) => {
        recognized_static_load($code, $pc).map(|_| ())?
    };
    ($code:ident, $pc:literal, $opcode:ident) => {
        (instruction_opcode($code, $pc)? == crate::ir::Opcode::$opcode).then_some(())?
    };
}

fn instruction_opcode(code: crate::machine::CodeView<'_>, pc: usize) -> Option<crate::ir::Opcode> {
    Some(code.instruction(pc)?.opcode)
}

impl PackedLoopFact {
    fn recognize(code: crate::machine::CodeView<'_>, counter: u16) -> Option<Self> {
        match code.len() {
            15 => recognize_add_fields(code, counter),
            17 => recognize_fill3(code, counter),
            11 | 20 => CopyRowFact::recognize(code).map(Self::CopyRow),
            30 => recognize_vertical_boundary(code, counter)
                .map(Self::Boundary)
                .or_else(|| DivergenceFact::recognize(code).map(Self::Divergence)),
            32 => recognize_negative_vertical_boundary(code, counter)
                .map(Self::Boundary),
            34 => recognize_horizontal_boundary(code, counter).map(Self::Boundary),
            38 => ProjectionFact::recognize(code).map(Self::Projection),
            _ => None,
        }
    }
}

#[derive(Clone, Copy)]
enum BoundaryAxis { Vertical, Horizontal }

#[derive(Clone, Copy)]
enum BoundarySign { Positive, Negative }

#[derive(Clone, Copy)]
struct BoundaryFact {
    array: u16,
    row_size: u16,
    edge: u16,
    axis: BoundaryAxis,
    sign: BoundarySign,
}

fn recognize_vertical_boundary(
    code: crate::machine::CodeView<'_>, counter: u16,
) -> Option<BoundaryFact> {
    compact_shape!(code;
        0 => StaticLoad, 1 => Move, 2 => StaticLoad, 3 => Move,
        4 => StaticLoad, 5 => StaticLoad, 6 => StaticLoad, 7 => Add,
        8 => AGetI, 9 => ASetI, 10 => Move, 11 => StaticLoad, 12 => Move,
        13 => StaticLoad, 14 => StaticLoad, 15 => LoadConst, 16 => Add,
        17 => StaticLoad, 18 => Mul, 19 => Add, 20 => Move, 21 => StaticLoad,
        22 => StaticLoad, 23 => StaticLoad, 24 => StaticLoad, 25 => Mul,
        26 => Add, 27 => AGetI, 28 => ASetI, 29 => Move
    );
    let array = same_static_slots(code, &[0, 4, 11, 21])?;
    same_static_slot(code, &[2, 5, 13, 22], counter)?;
    let row_size = same_static_slots(code, &[6, 17, 24])?;
    let edge = same_static_slots(code, &[14, 23])?;
    constant_one(code, 15)?;
    validate_boundary_sets(code, (3, 2, 8, 9), (20, 19, 27, 28))?;
    Some(BoundaryFact {
        array, row_size, edge, axis: BoundaryAxis::Vertical, sign: BoundarySign::Positive,
    })
}

fn recognize_negative_vertical_boundary(
    code: crate::machine::CodeView<'_>, counter: u16,
) -> Option<BoundaryFact> {
    compact_shape!(code;
        0 => StaticLoad, 1 => Move, 2 => StaticLoad, 3 => Move,
        4 => StaticLoad, 5 => StaticLoad, 6 => StaticLoad, 7 => Add,
        8 => AGetI, 9 => Slow, 10 => ASetI, 11 => Move,
        12 => StaticLoad, 13 => Move, 14 => StaticLoad, 15 => StaticLoad,
        16 => LoadConst, 17 => Add, 18 => StaticLoad, 19 => Mul,
        20 => Add, 21 => Move, 22 => StaticLoad, 23 => StaticLoad,
        24 => StaticLoad, 25 => StaticLoad, 26 => Mul, 27 => Add,
        28 => AGetI, 29 => Slow, 30 => ASetI, 31 => Move
    );
    let array = same_static_slots(code, &[0, 4, 12, 22])?;
    same_static_slot(code, &[2, 5, 14, 23], counter)?;
    let row_size = same_static_slots(code, &[6, 18, 25])?;
    let edge = same_static_slots(code, &[15, 24])?;
    constant_one(code, 16)?;
    let first_value = validate_negate(code, 8, 9)?;
    let second_value = validate_negate(code, 28, 29)?;
    validate_boundary_set_value(code, 3, 2, first_value, 10)?;
    validate_boundary_set_value(code, 21, 20, second_value, 30)?;
    Some(BoundaryFact {
        array, row_size, edge, axis: BoundaryAxis::Vertical, sign: BoundarySign::Negative,
    })
}

fn recognize_horizontal_boundary(
    code: crate::machine::CodeView<'_>, counter: u16,
) -> Option<BoundaryFact> {
    compact_shape!(code;
        0 => StaticLoad, 1 => Move, 2 => StaticLoad, 3 => StaticLoad,
        4 => Mul, 5 => Move, 6 => StaticLoad, 7 => LoadConst,
        8 => StaticLoad, 9 => StaticLoad, 10 => Mul, 11 => Add,
        12 => AGetI, 13 => ASetI, 14 => Move, 15 => StaticLoad, 16 => Move,
        17 => StaticLoad, 18 => LoadConst, 19 => Add, 20 => StaticLoad,
        21 => StaticLoad, 22 => Mul, 23 => Add, 24 => Move, 25 => StaticLoad,
        26 => StaticLoad, 27 => StaticLoad, 28 => StaticLoad, 29 => Mul,
        30 => Add, 31 => AGetI, 32 => ASetI, 33 => Move
    );
    let array = same_static_slots(code, &[0, 6, 15, 25])?;
    same_static_slot(code, &[2, 8, 20, 27], counter)?;
    let row_size = same_static_slots(code, &[3, 9, 21, 28])?;
    let edge = same_static_slots(code, &[17, 26])?;
    constant_one(code, 7)?;
    constant_one(code, 18)?;
    validate_boundary_sets(code, (5, 4, 12, 13), (24, 23, 31, 32))?;
    Some(BoundaryFact {
        array, row_size, edge, axis: BoundaryAxis::Horizontal, sign: BoundarySign::Positive,
    })
}

fn validate_negate(
    code: crate::machine::CodeView<'_>, get_pc: usize, unary_pc: usize,
) -> Option<u16> {
    let crate::ops::Op::Unary {
        dst, operator: crate::ops::UnaryOp::Minus, src,
    } = code.cold_at(unary_pc)? else { return None };
    (*src == code.instruction(get_pc)?.a).then_some(*dst)
}

fn validate_boundary_set_value(
    code: crate::machine::CodeView<'_>,
    target: usize,
    key: usize,
    value: u16,
    set: usize,
) -> Option<()> {
    let set_op = code.instruction(set)?;
    (set_op.a == code.instruction(target)?.a
        && set_op.b == code.instruction(key)?.a
        && set_op.c == value).then_some(())
}

fn same_static_slots(code: crate::machine::CodeView<'_>, pcs: &[usize]) -> Option<u16> {
    let first = recognized_static_load(code, *pcs.first()?)?.1;
    pcs.iter().all(|pc| recognized_static_load(code, *pc).is_some_and(|(_, slot)| slot == first))
        .then_some(first)
}

fn same_static_slot(code: crate::machine::CodeView<'_>, pcs: &[usize], slot: u16) -> Option<()> {
    pcs.iter().all(|pc| recognized_static_load(code, *pc).is_some_and(|(_, value)| value == slot))
        .then_some(())
}

fn constant_one(code: crate::machine::CodeView<'_>, pc: usize) -> Option<()> {
    matches!(code.constant_at(pc), Some((_, crate::ops::Constant::Number(1.0)))).then_some(())
}

fn validate_boundary_sets(
    code: crate::machine::CodeView<'_>,
    first: (usize, usize, usize, usize),
    second: (usize, usize, usize, usize),
) -> Option<()> {
    for (target, key, get, set) in [first, second] {
        let set_op = code.instruction(set)?;
        (set_op.a == code.instruction(target)?.a
            && set_op.b == code.instruction(key)?.a
            && set_op.c == code.instruction(get)?.a).then_some(())?;
    }
    Some(())
}

fn recognize_fill3(code: crate::machine::CodeView<'_>, counter: u16) -> Option<PackedLoopFact> {
    compact_shape!(code;
        0 => StaticLoad, 1 => Move, 2 => StaticLoad, 3 => Move,
        4 => StaticLoad, 5 => Move, 6 => StaticLoad, 7 => Move,
        8 => StaticLoad, 9 => Move, 10 => StaticLoad, 11 => Move,
        12 => LoadConst, 13 => ASetI, 14 => ASetI, 15 => ASetI, 16 => Move
    );
    let arrays = [recognized_static_load(code, 0)?.1,
        recognized_static_load(code, 4)?.1, recognized_static_load(code, 8)?.1];
    let indices = [recognized_static_load(code, 2)?.1,
        recognized_static_load(code, 6)?.1, recognized_static_load(code, 10)?.1];
    indices.into_iter().all(|slot| slot == counter).then_some(())?;
    matches!(code.constant_at(12), Some((_, crate::ops::Constant::Number(0.0)))).then_some(())?;
    for (pc, (array_pc, index_pc)) in [13, 14, 15].into_iter().zip([(8, 10), (4, 6), (0, 2)]) {
        let set = code.instruction(pc)?;
        let target = code.instruction(index_pc + 1)?;
        (target.b == code.instruction(array_pc + 1)?.a
            && set.a == target.a
            && set.b == code.instruction(index_pc)?.a
            && set.c == code.instruction(12)?.a).then_some(())?;
    }
    Some(PackedLoopFact::Fill3 { arrays })
}

#[derive(Clone, Copy)]
struct CopyRowFact {
    pairs: [(u16, u16); 2],
    pair_count: usize,
    position: u16,
}

impl CopyRowFact {
    fn recognize(code: crate::machine::CodeView<'_>) -> Option<Self> {
        let pair_count = match code.len() { 11 => 1, 20 => 2, _ => return None };
        let first = recognize_copy_pair(code, 0)?;
        let second = if pair_count == 2 { recognize_copy_pair(code, 9)? } else { first };
        let update_pc = if pair_count == 1 { 9 } else { 18 };
        (instruction_opcode(code, update_pc)? == crate::ir::Opcode::UpdateLocal).then_some(())?;
        let position = kernel_update_slot(code, update_pc)?;
        (first.2 == position && second.2 == position).then_some(())?;
        Some(Self { pairs: [(first.0, first.1), (second.0, second.1)], pair_count, position })
    }
}

fn recognize_copy_pair(code: crate::machine::CodeView<'_>, pc: usize) -> Option<(u16, u16, u16)> {
    [0, 2, 4, 5]
        .into_iter()
        .all(|offset| recognized_static_load(code, pc + offset).is_some())
        .then_some(())?;
    [(1, crate::ir::Opcode::Move), (3, crate::ir::Opcode::Move),
        (6, crate::ir::Opcode::AGetI), (7, crate::ir::Opcode::ASetI),
        (8, crate::ir::Opcode::Move)]
        .into_iter()
        .all(|(offset, opcode)| instruction_opcode(code, pc + offset) == Some(opcode))
        .then_some(())?;
    let (_, destination) = recognized_static_load(code, pc)?;
    let (_, destination_index) = recognized_static_load(code, pc + 2)?;
    let (_, source) = recognized_static_load(code, pc + 4)?;
    let (_, source_index) = recognized_static_load(code, pc + 5)?;
    let get = code.instruction(pc + 6)?;
    let set = code.instruction(pc + 7)?;
    (destination_index == source_index && get.b == code.instruction(pc + 4)?.a).then_some(())?;
    (get.c == code.instruction(pc + 5)?.a && set.c == get.a).then_some(())?;
    (set.a == code.instruction(pc + 3)?.a && set.b == code.instruction(pc + 2)?.a)
        .then_some(())?;
    Some((destination, source, destination_index))
}

fn recognize_add_fields(code: crate::machine::CodeView<'_>, counter: u16) -> Option<PackedLoopFact> {
    compact_shape!(code;
        0 => StaticLoad, 2 => StaticLoad, 4 => Slow, 5 => Slow,
        6 => AGetI, 7 => StaticLoad, 8 => StaticLoad,
        9 => StaticLoad, 10 => AGetI, 11 => Mul, 12 => Add, 13 => ASetI
    );
    let (_, x) = recognized_static_load(code, 0)?;
    let (_, index) = recognized_static_load(code, 2)?;
    let (_, scale) = recognized_static_load(code, 7)?;
    let (_, source) = recognized_static_load(code, 8)?;
    let (_, second_index) = recognized_static_load(code, 9)?;
    let get_x = code.instruction(6)?;
    let get_source = code.instruction(10)?;
    let mul = code.instruction(11)?;
    let add = code.instruction(12)?;
    let set = code.instruction(13)?;
    matches!(code.cold_at(4), Some(Op::RequireObjectCoercible { .. })).then_some(())?;
    matches!(code.cold_at(5), Some(Op::ToPropertyKey { .. })).then_some(())?;
    (index == counter && second_index == counter).then_some(())?;
    (get_x.b == code.instruction(3)?.a && get_source.b == code.instruction(8)?.a).then_some(())?;
    (mul.b == code.instruction(7)?.a && mul.c == get_source.a).then_some(())?;
    (add.b == get_x.a && add.c == mul.a).then_some(())?;
    (set.a == get_x.b && set.b == get_x.c && set.c == add.a).then_some(())?;
    Some(PackedLoopFact::AddFields { x, source, scale })
}

#[derive(Clone, Copy)]
struct DivergenceFact {
    u: u16, v: u16, p: u16, div: u16, h: u16,
    previous_row: u16, prev_value: u16, current_row: u16, next_value: u16, next_row: u16,
}

impl DivergenceFact {
    fn recognize(code: crate::machine::CodeView<'_>) -> Option<Self> {
        compact_shape!(code;
            0 => StaticLoad, 2 => UpdateLocal, 5 => StaticLoad,
            6 => UpdateLocal, 7 => AGetI, 8 => StaticLoad, 9 => UpdateLocal,
            10 => AGetI, 11 => Sub, 12 => StaticLoad, 13 => UpdateLocal,
            14 => AGetI, 15 => Add, 16 => StaticLoad, 17 => UpdateLocal,
            18 => AGetI, 19 => Sub, 20 => Mul, 21 => ASetI,
            23 => StaticLoad, 25 => StaticLoad, 27 => LoadConst, 28 => ASetI
        );
        let (_, div) = recognized_static_load(code, 0)?;
        let (_, h) = recognized_static_load(code, 4)?;
        let (_, u) = recognized_static_load(code, 5)?;
        let (_, second_u) = recognized_static_load(code, 8)?;
        let (_, v) = recognized_static_load(code, 12)?;
        let (_, second_v) = recognized_static_load(code, 16)?;
        let (_, p) = recognized_static_load(code, 23)?;
        let (_, current_row) = recognized_static_load(code, 25)?;
        (u == second_u && v == second_v).then_some(())?;
        let zero = matches!(code.constant_at(27), Some((_, crate::ops::Constant::Number(0.0))));
        zero.then_some(())?;
        let current_update = kernel_update_slot(code, 2)?;
        (current_row == current_update).then_some(())?;
        validate_divergence_graph(code)?;
        Some(Self {
            u, v, p, div, h,
            current_row: current_update,
            next_value: kernel_update_slot(code, 6)?,
            prev_value: kernel_update_slot(code, 9)?,
            next_row: kernel_update_slot(code, 13)?,
            previous_row: kernel_update_slot(code, 17)?,
        })
    }
}

#[derive(Clone, Copy)]
struct ProjectionFact {
    u: u16, v: u16, p: u16, w_scale: u16, h_scale: u16,
    prev_pos: u16, current_pos: u16, next_pos: u16, prev_row: u16, next_row: u16,
}

impl ProjectionFact {
    fn recognize(code: crate::machine::CodeView<'_>) -> Option<Self> {
        compact_shape!(code;
            0 => StaticLoad, 2 => UpdateLocal, 6 => AGetI,
            7 => StaticLoad, 8 => StaticLoad, 9 => UpdateLocal,
            10 => AGetI, 11 => StaticLoad, 12 => UpdateLocal, 13 => AGetI,
            14 => Sub, 15 => Mul, 16 => Sub, 17 => ASetI,
            19 => StaticLoad, 21 => StaticLoad, 25 => AGetI,
            26 => StaticLoad, 27 => StaticLoad, 28 => UpdateLocal,
            29 => AGetI, 30 => StaticLoad, 31 => UpdateLocal, 32 => AGetI,
            33 => Sub, 34 => Mul, 35 => Sub, 36 => ASetI
        );
        let (_, u) = recognized_static_load(code, 0)?;
        let (_, w_scale) = recognized_static_load(code, 7)?;
        let (_, p) = recognized_static_load(code, 8)?;
        let (_, second_p) = recognized_static_load(code, 11)?;
        let (_, v) = recognized_static_load(code, 19)?;
        let (_, current_pos) = recognized_static_load(code, 21)?;
        let (_, h_scale) = recognized_static_load(code, 26)?;
        let (_, third_p) = recognized_static_load(code, 27)?;
        let (_, fourth_p) = recognized_static_load(code, 30)?;
        (p == second_p && p == third_p && p == fourth_p).then_some(())?;
        (current_pos == kernel_update_slot(code, 2)?).then_some(())?;
        validate_projection_graph(code)?;
        Some(Self {
            u, v, p, w_scale, h_scale, current_pos,
            next_pos: kernel_update_slot(code, 9)?,
            prev_pos: kernel_update_slot(code, 12)?,
            next_row: kernel_update_slot(code, 28)?,
            prev_row: kernel_update_slot(code, 31)?,
        })
    }
}

fn validate_divergence_graph(code: crate::machine::CodeView<'_>) -> Option<()> {
    let i = |pc| code.instruction(pc);
    let graph = i(7)?.b == i(5)?.a && i(7)?.c == i(6)?.b
        && i(10)?.b == i(8)?.a && i(10)?.c == i(9)?.b
        && i(11)?.b == i(7)?.a && i(11)?.c == i(10)?.a
        && i(14)?.b == i(12)?.a && i(14)?.c == i(13)?.b
        && i(18)?.b == i(16)?.a && i(18)?.c == i(17)?.b
        && i(20)?.b == i(4)?.a && i(21)?.c == i(20)?.a
        && i(21)?.b == i(2)?.b && i(28)?.b == i(25)?.a;
    graph.then_some(())
}

fn validate_projection_graph(code: crate::machine::CodeView<'_>) -> Option<()> {
    let i = |pc| code.instruction(pc);
    let graph = i(6)?.b == i(3)?.a
        && i(10)?.b == i(8)?.a && i(10)?.c == i(9)?.b
        && i(13)?.b == i(11)?.a && i(13)?.c == i(12)?.b
        && i(14)?.b == i(10)?.a && i(14)?.c == i(13)?.a
        && i(15)?.b == i(7)?.a && i(15)?.c == i(14)?.a
        && i(16)?.b == i(6)?.a && i(16)?.c == i(15)?.a
        && i(17)?.a == i(3)?.a && i(17)?.c == i(16)?.a
        && i(25)?.b == i(22)?.a
        && i(29)?.b == i(27)?.a && i(29)?.c == i(28)?.b
        && i(32)?.b == i(30)?.a && i(32)?.c == i(31)?.b
        && i(33)?.b == i(29)?.a && i(33)?.c == i(32)?.a
        && i(34)?.b == i(26)?.a && i(34)?.c == i(33)?.a
        && i(35)?.b == i(25)?.a && i(35)?.c == i(34)?.a
        && i(36)?.a == i(22)?.a && i(36)?.c == i(35)?.a;
    graph.then_some(())
}

fn run_packed_loop_kernel(
    loop_fact: CountedForFact,
    body: crate::machine::CodeView<'_>,
    loop_shape: u64,
) -> Option<crate::completion::Completion> {
    (loop_fact.timing == CountedStepTiming::AfterBody).then_some(())?;
    let fact = PackedLoopFact::recognize(body, loop_fact.slot)?;
    let environment = crate::locals::current();
    let counter = environment.get_number(loop_fact.slot)?;
    let iterations = unit_iteration_count(loop_fact, counter, loop_fact.bound.number(&environment)?)?;
    match fact {
        PackedLoopFact::AddFields { x, source, scale } => {
            run_add_fields(&environment, x, source, scale, counter, iterations, loop_fact)?
        }
        PackedLoopFact::Fill3 { arrays } => {
            run_fill3(&environment, arrays, counter, iterations, loop_fact)?
        }
        PackedLoopFact::Boundary(boundary) => {
            run_boundary(&environment, boundary, counter, iterations, loop_fact)?
        }
        PackedLoopFact::CopyRow(fact) => run_copy_row(&environment, fact, counter, iterations, loop_fact)?,
        PackedLoopFact::Divergence(fact) => run_divergence(&environment, fact, counter, iterations, loop_fact)?,
        PackedLoopFact::Projection(fact) => run_projection(&environment, fact, counter, iterations, loop_fact)?,
    }
    let (id, gets, sets) = fact.trace_fact();
    crate::execution_trace::numeric_kernel_iterations(id, loop_shape, iterations, gets, sets);
    Some(crate::completion::Completion::Normal)
}

impl PackedLoopFact {
    fn trace_fact(self) -> (&'static str, usize, usize) {
        match self {
            Self::AddFields { .. } => ("counted_packed_f64_add_fields", 2, 1),
            Self::Fill3 { .. } => ("counted_packed_f64_fill3", 0, 3),
            Self::Boundary(_) => ("counted_packed_f64_boundary", 2, 2),
            Self::CopyRow(fact) => ("counted_packed_f64_copy", fact.pair_count, fact.pair_count),
            Self::Divergence(_) => ("counted_packed_f64_divergence", 4, 2),
            Self::Projection(_) => ("counted_packed_f64_projection", 6, 2),
        }
    }
}

fn run_boundary(
    environment: &crate::environment::Environment,
    fact: BoundaryFact,
    counter: f64,
    iterations: usize,
    loop_fact: CountedForFact,
) -> Option<()> {
    (loop_fact.comparison == crate::ops::BinaryOp::LessEqual && loop_fact.step == 1.0)
        .then_some(())?;
    let start = kernel_index(counter)?;
    let end = start.checked_add(iterations)?;
    let row_size = kernel_index(environment.get_number(fact.row_size)?)?;
    let edge = kernel_index(environment.get_number(fact.edge)?)?;
    let array = packed_array(environment, fact.array)?;
    let mut words = array.numeric_kernel_words_mut()?;
    validate_boundary_range(words.len(), fact.axis, start, end, row_size, edge)?;
    for index in start..end {
        let (dst1, src1, dst2, src2) = boundary_indices(fact.axis, index, row_size, edge);
        match fact.sign {
            BoundarySign::Positive => {
                words[dst1] = words[src1];
                words[dst2] = words[src2];
            }
            BoundarySign::Negative => {
                words[dst1] = -words[src1];
                words[dst2] = -words[src2];
            }
        }
    }
    environment.set(loop_fact.slot, Value::Number(end as f64));
    Some(())
}

fn boundary_indices(
    axis: BoundaryAxis, index: usize, row_size: usize, edge: usize,
) -> (usize, usize, usize, usize) {
    match axis {
        BoundaryAxis::Vertical => (index, index + row_size,
            index + (edge + 1) * row_size, index + edge * row_size),
        BoundaryAxis::Horizontal => (index * row_size, 1 + index * row_size,
            edge + 1 + index * row_size, edge + index * row_size),
    }
}

fn validate_boundary_range(
    len: usize, axis: BoundaryAxis, start: usize, end: usize, row_size: usize, edge: usize,
) -> Option<()> {
    (start < end && row_size != 0).then_some(())?;
    let index = end - 1;
    let last = match axis {
        BoundaryAxis::Vertical => index.checked_add(edge.checked_add(1)?.checked_mul(row_size)?)?,
        BoundaryAxis::Horizontal => edge.checked_add(1)?.checked_add(index.checked_mul(row_size)?)?,
    };
    (last < len).then_some(())
}

fn run_fill3(
    environment: &crate::environment::Environment,
    arrays: [u16; 3],
    counter: f64,
    iterations: usize,
    loop_fact: CountedForFact,
) -> Option<()> {
    (loop_fact.comparison == crate::ops::BinaryOp::LessThan && loop_fact.step == 1.0)
        .then_some(())?;
    let start = kernel_index(counter)?;
    let end = start.checked_add(iterations)?;
    for slot in arrays {
        let array = packed_array(environment, slot)?;
        let mut words = array.numeric_kernel_words_mut()?;
        words.get_mut(start..end)?.fill(0.0);
    }
    environment.set(loop_fact.slot, Value::Number(end as f64));
    Some(())
}

fn run_copy_row(
    environment: &crate::environment::Environment,
    fact: CopyRowFact, counter: f64, iterations: usize, loop_fact: CountedForFact,
) -> Option<()> {
    (loop_fact.comparison == crate::ops::BinaryOp::LessThan && loop_fact.step == 1.0).then_some(())?;
    let start = kernel_index(environment.get_number(fact.position)?)?;
    let end = start.checked_add(iterations)?;
    let first = arrays2(environment, fact.pairs[0])?;
    let second = arrays2(environment, fact.pairs[1])?;
    let cells = [(first.0.numeric_cells()?, first.1.numeric_cells()?),
        (second.0.numeric_cells()?, second.1.numeric_cells()?)];
    cells.iter().take(fact.pair_count).all(|(dst, src)| end <= dst.len() && end <= src.len()).then_some(())?;
    for index in start..end {
        for (dst, src) in cells.iter().take(fact.pair_count) {
            dst[index].set(src[index].get());
        }
    }
    environment.set(fact.position, Value::Number(end as f64));
    environment.set(loop_fact.slot, Value::Number(counter + iterations as f64));
    Some(())
}

fn arrays2(
    environment: &crate::environment::Environment, slots: (u16, u16),
) -> Option<(std::rc::Rc<crate::value::ArrayData>, std::rc::Rc<crate::value::ArrayData>)> {
    Some((packed_array(environment, slots.0)?, packed_array(environment, slots.1)?))
}

fn packed_array(environment: &crate::environment::Environment, slot: u16) -> Option<std::rc::Rc<crate::value::ArrayData>> {
    let Value::Array(array) = crate::locals::resolved_replacement(environment.get(slot)) else { return None };
    array.is_packed_ordinary().then_some(array)
}

fn run_add_fields(
    environment: &crate::environment::Environment,
    x_slot: u16, source_slot: u16, scale_slot: u16,
    counter: f64, iterations: usize, loop_fact: CountedForFact,
) -> Option<()> {
    (loop_fact.comparison == crate::ops::BinaryOp::LessThan && loop_fact.step == 1.0).then_some(())?;
    let start = kernel_index(counter)?;
    let end = start.checked_add(iterations)?;
    let scale = environment.get_number(scale_slot)?;
    let x = packed_array(environment, x_slot)?;
    let source = packed_array(environment, source_slot)?;
    if std::rc::Rc::ptr_eq(&x, &source) {
        let mut words = x.numeric_kernel_words_mut()?;
        (end <= words.len()).then_some(())?;
        for value in &mut words[start..end] {
            *value += scale * *value;
        }
    } else {
        let mut x_words = x.numeric_kernel_words_mut()?;
        let source_words = source.numeric_kernel_words()?;
        (end <= x_words.len() && end <= source_words.len()).then_some(())?;
        for (value, source) in x_words[start..end].iter_mut().zip(&source_words[start..end]) {
            *value += scale * source;
        }
    }
    environment.set(loop_fact.slot, Value::Number(counter + iterations as f64));
    Some(())
}

fn run_divergence(
    environment: &crate::environment::Environment,
    fact: DivergenceFact, counter: f64, iterations: usize, loop_fact: CountedForFact,
) -> Option<()> {
    unit_inclusive_loop(loop_fact)?;
    let mut current = kernel_index(environment.get_number(fact.current_row)?)?;
    let mut next_value = kernel_index(environment.get_number(fact.next_value)?)?;
    let mut prev_value = kernel_index(environment.get_number(fact.prev_value)?)?;
    let mut next_row = kernel_index(environment.get_number(fact.next_row)?)?;
    let mut previous_row = kernel_index(environment.get_number(fact.previous_row)?)?;
    let h = environment.get_number(fact.h)?;
    let (u, v, p, div) = arrays4(environment, fact.u, fact.v, fact.p, fact.div)?;
    (!std::rc::Rc::ptr_eq(&div, &u) && !std::rc::Rc::ptr_eq(&div, &v)
        && !std::rc::Rc::ptr_eq(&p, &u) && !std::rc::Rc::ptr_eq(&p, &v))
        .then_some(())?;
    let (uw, vw, pw, dw) = (u.numeric_cells()?, v.numeric_cells()?, p.numeric_cells()?, div.numeric_cells()?);
    validate_divergence_bounds(uw.len(), vw.len(), pw.len(), dw.len(), [next_value, prev_value, next_row, previous_row], current, iterations)?;
    for _ in 0..iterations {
        current += 1; next_value += 1; prev_value += 1; next_row += 1; previous_row += 1;
        dw[current].set(h * (uw[next_value].get() - uw[prev_value].get() + vw[next_row].get() - vw[previous_row].get()));
        pw[current].set(0.0);
    }
    flush_indices(environment, loop_fact, counter, iterations, &[(fact.current_row,current),(fact.next_value,next_value),(fact.prev_value,prev_value),(fact.next_row,next_row),(fact.previous_row,previous_row)]);
    Some(())
}

fn run_projection(
    environment: &crate::environment::Environment,
    fact: ProjectionFact, counter: f64, iterations: usize, loop_fact: CountedForFact,
) -> Option<()> {
    unit_inclusive_loop(loop_fact)?;
    let mut current = kernel_index(environment.get_number(fact.current_pos)?)?;
    let mut next_pos = kernel_index(environment.get_number(fact.next_pos)?)?;
    let mut prev_pos = kernel_index(environment.get_number(fact.prev_pos)?)?;
    let mut next_row = kernel_index(environment.get_number(fact.next_row)?)?;
    let mut prev_row = kernel_index(environment.get_number(fact.prev_row)?)?;
    let w_scale = environment.get_number(fact.w_scale)?;
    let h_scale = environment.get_number(fact.h_scale)?;
    let (u, v, p) = arrays3(environment, fact.u, fact.v, fact.p)?;
    (!std::rc::Rc::ptr_eq(&p, &u) && !std::rc::Rc::ptr_eq(&p, &v)).then_some(())?;
    let (uw, vw, pw) = (u.numeric_cells()?, v.numeric_cells()?, p.numeric_cells()?);
    validate_projection_bounds(uw.len(), vw.len(), pw.len(), [next_pos, prev_pos, next_row, prev_row], current, iterations)?;
    for _ in 0..iterations {
        current += 1; next_pos += 1; prev_pos += 1; next_row += 1; prev_row += 1;
        uw[current].set(uw[current].get() - w_scale * (pw[next_pos].get() - pw[prev_pos].get()));
        vw[current].set(vw[current].get() - h_scale * (pw[next_row].get() - pw[prev_row].get()));
    }
    flush_indices(environment, loop_fact, counter, iterations, &[(fact.current_pos,current),(fact.next_pos,next_pos),(fact.prev_pos,prev_pos),(fact.next_row,next_row),(fact.prev_row,prev_row)]);
    Some(())
}

fn unit_inclusive_loop(fact: CountedForFact) -> Option<()> {
    (fact.comparison == crate::ops::BinaryOp::LessEqual && fact.step == 1.0).then_some(())
}

fn arrays3(environment: &crate::environment::Environment, a: u16, b: u16, c: u16) -> Option<(std::rc::Rc<crate::value::ArrayData>, std::rc::Rc<crate::value::ArrayData>, std::rc::Rc<crate::value::ArrayData>)> {
    Some((packed_array(environment, a)?, packed_array(environment, b)?, packed_array(environment, c)?))
}

fn arrays4(environment: &crate::environment::Environment, a: u16, b: u16, c: u16, d: u16) -> Option<(std::rc::Rc<crate::value::ArrayData>, std::rc::Rc<crate::value::ArrayData>, std::rc::Rc<crate::value::ArrayData>, std::rc::Rc<crate::value::ArrayData>)> {
    Some((packed_array(environment, a)?, packed_array(environment, b)?, packed_array(environment, c)?, packed_array(environment, d)?))
}

fn validate_divergence_bounds(
    u_len: usize, v_len: usize, p_len: usize, div_len: usize,
    indices: [usize; 4], current: usize, iterations: usize,
) -> Option<()> {
    let current_end = current.checked_add(iterations)?;
    let lengths = [u_len, u_len, v_len, v_len];
    let fits = current_end < p_len && current_end < div_len
        && indices.into_iter().zip(lengths).all(|(index, len)| index.checked_add(iterations).is_some_and(|end| end < len));
    fits.then_some(())
}

fn validate_projection_bounds(
    u_len: usize, v_len: usize, p_len: usize,
    indices: [usize; 4], current: usize, iterations: usize,
) -> Option<()> {
    let current_end = current.checked_add(iterations)?;
    let fits = current_end < u_len && current_end < v_len
        && indices.into_iter().all(|index| index.checked_add(iterations).is_some_and(|end| end < p_len));
    fits.then_some(())
}

fn flush_indices(
    environment: &crate::environment::Environment,
    fact: CountedForFact, counter: f64, iterations: usize, indices: &[(u16, usize)],
) {
    for &(slot, value) in indices {
        environment.set(slot, Value::Number(value as f64));
    }
    environment.set(fact.slot, Value::Number(counter + iterations as f64));
}
