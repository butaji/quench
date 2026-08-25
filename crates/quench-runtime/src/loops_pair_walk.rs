#[derive(Clone, Copy)]
struct PairWalkPlan {
    head: u16,
    pair: u16,
    total: u16,
}

fn run_pair_word_walk(
    fact: CountedForFact,
    body: crate::machine::CodeView<'_>,
    dst: u16,
    per_iteration: &[u16],
    registers: &mut crate::register_file::RegisterFile,
    loop_shape: u64,
) -> Option<crate::completion::Completion> {
    let plan = recognize_pair_word_walk(fact, body, dst, per_iteration)?;
    let environment = crate::locals::current();
    let index = environment.get_number(fact.slot)?;
    let bound = fact.bound.number(&environment)?;
    let total = environment.get_number(plan.total)?;
    let head = pair_object(&environment, plan.head)?;
    let pair = pair_object(&environment, plan.pair)?;
    execute_pair_word_walk(
        fact,
        plan,
        environment,
        registers,
        dst,
        loop_shape,
        head,
        pair,
        index,
        bound,
        total,
    )
}

#[allow(clippy::too_many_arguments)]
fn execute_pair_word_walk(
    fact: CountedForFact,
    plan: PairWalkPlan,
    environment: std::rc::Rc<crate::environment::Environment>,
    registers: &mut crate::register_file::RegisterFile,
    dst: u16,
    loop_shape: u64,
    head: std::rc::Rc<crate::value::ObjectData>,
    pair: std::rc::Rc<crate::value::ObjectData>,
    mut index: f64,
    bound: f64,
    mut total: f64,
) -> Option<crate::completion::Completion> {
    let head_pointer = std::rc::Rc::as_ptr(&head);
    let mut pair_pointer = std::rc::Rc::as_ptr(&pair);
    let shape = head.semantic_layout_id();
    let car = head.hot_properties().position_rev("car")?;
    let cdr = head.hot_properties().position_rev("cdr")?;
    let mut iterations = 0;
    let mut last_reset = false;
    crate::execution_trace::event(crate::execution_trace::Event::CountedForAttempt);
    while counted_comparison(fact.comparison, index, bound) {
        let object = unsafe { &*pair_pointer };
        if object.has_replacement() || object.semantic_layout_id() != shape {
            flush_pair_walk(&environment, plan, fact.slot, pair_pointer, total, index);
            crate::execution_trace::event(crate::execution_trace::Event::CountedForDeopt);
            crate::execution_trace::kernel("pair_word_walk", true);
            return None;
        }
        let properties = object.hot_properties();
        let car_word = properties.slot_word(car)?;
        let cdr_word = properties.slot_word(cdr)?;
        total += car_word.number()?;
        let next = cdr_word.object_or_null_ptr()?;
        last_reset = next.is_none();
        pair_pointer = next.unwrap_or(head_pointer);
        index += fact.step;
        iterations += 1;
        trace_pair_word_iteration(loop_shape, car_word, cdr_word);
    }
    flush_pair_walk(&environment, plan, fact.slot, pair_pointer, total, index);
    if iterations != 0 {
        let value = if last_reset {
            crate::value::Value::Object(head)
        } else {
            crate::value::Value::Undefined
        };
        crate::execute::write_value(registers, dst, value);
    }
    Some(crate::completion::Completion::Normal)
}

fn pair_object(
    environment: &crate::environment::Environment,
    slot: u16,
) -> Option<std::rc::Rc<crate::value::ObjectData>> {
    match crate::locals::resolved_replacement(environment.get(slot)) {
        crate::value::Value::Object(object) => Some(object),
        _ => None,
    }
}

fn flush_pair_walk(
    environment: &crate::environment::Environment,
    plan: PairWalkPlan,
    index_slot: u16,
    pair: *const crate::value::ObjectData,
    total: f64,
    index: f64,
) {
    environment.set(plan.total, crate::value::Value::Number(total));
    environment.set(index_slot, crate::value::Value::Number(index));
    environment.set(plan.pair, object_value(pair));
}

fn object_value(pointer: *const crate::value::ObjectData) -> crate::value::Value {
    // SAFETY: admission retains both the original pair and head graphs for the
    // entire traversal. Incrementing creates the owner transferred to Value.
    unsafe {
        std::rc::Rc::increment_strong_count(pointer);
        crate::value::Value::Object(std::rc::Rc::from_raw(pointer))
    }
}

#[inline(always)]
fn trace_pair_word_iteration(
    loop_shape: u64,
    car: &crate::register_file::SlotWord,
    cdr: &crate::register_file::SlotWord,
) {
    crate::execution_trace::event(crate::execution_trace::Event::LoopIteration);
    crate::execution_trace::loop_shape_iteration(loop_shape);
    crate::execution_trace::event(crate::execution_trace::Event::CountedForHit);
    crate::execution_trace::event(crate::execution_trace::Event::NamedPropertyHit);
    car.trace_named_payload("own");
    crate::execution_trace::event(crate::execution_trace::Event::NamedPropertyHit);
    cdr.trace_named_payload("own");
    crate::execution_trace::kernel("pair_word_walk", false);
}

fn recognize_pair_word_walk(
    fact: CountedForFact,
    body: crate::machine::CodeView<'_>,
    dst: u16,
    per_iteration: &[u16],
) -> Option<PairWalkPlan> {
    (per_iteration == [fact.slot]
        && fact.timing == CountedStepTiming::AfterBody
        && fact.step == 1.0
        && body.len() == 16)
        .then_some(())?;
    let op = |pc| body.instruction(pc).unwrap();
    let (sum, pair, car, add, store_sum, result_sum) = (op(0), op(1), op(2), op(3), op(4), op(5));
    let (pair_again, cdr, store_pair, result_pair) = (op(6), op(7), op(8), op(9));
    let head = recognize_pair_reset(body, pair.b, dst)?;
    validate_pair_accesses(
        body,
        dst,
        sum,
        pair,
        car,
        add,
        store_sum,
        result_sum,
        pair_again,
        cdr,
        store_pair,
        result_pair,
    )?;
    Some(PairWalkPlan {
        head,
        pair: pair.b,
        total: sum.b,
    })
}

#[allow(clippy::too_many_arguments)]
fn validate_pair_accesses(
    body: crate::machine::CodeView<'_>,
    dst: u16,
    sum: crate::ir::Instruction,
    pair: crate::ir::Instruction,
    car: crate::ir::Instruction,
    add: crate::ir::Instruction,
    store_sum: crate::ir::Instruction,
    result_sum: crate::ir::Instruction,
    pair_again: crate::ir::Instruction,
    cdr: crate::ir::Instruction,
    store_pair: crate::ir::Instruction,
    result_pair: crate::ir::Instruction,
) -> Option<()> {
    (is_local_load(sum)
        && is_local_load(pair)
        && car.opcode == crate::ir::Opcode::GetN
        && car.b == pair.a
        && body.metadata_at(2)?.name.as_deref() == Some("car")
        && add.opcode == crate::ir::Opcode::Add
        && (add.b, add.c) == (sum.a, car.a)
        && is_local_store(store_sum)
        && (store_sum.a, store_sum.b) == (sum.b, add.a)
        && result_sum.opcode == crate::ir::Opcode::Move
        && (result_sum.a, result_sum.b) == (dst, add.a)
        && is_local_load(pair_again)
        && pair_again.b == pair.b
        && cdr.opcode == crate::ir::Opcode::GetN
        && cdr.b == pair_again.a
        && body.metadata_at(7)?.name.as_deref() == Some("cdr")
        && is_local_store(store_pair)
        && (store_pair.a, store_pair.b) == (pair.b, cdr.a)
        && result_pair.opcode == crate::ir::Opcode::Move
        && (result_pair.a, result_pair.b) == (dst, cdr.a))
        .then_some(())
}

fn recognize_pair_reset(body: crate::machine::CodeView<'_>, pair: u16, dst: u16) -> Option<u16> {
    let loaded = body.instruction(10)?;
    let (_, crate::ops::Constant::Null) = body.constant_at(11)? else {
        return None;
    };
    let (condition, crate::ops::BinaryOp::StrictEqual, lhs, rhs) = body.binary_at(12)? else {
        return None;
    };
    let (undefined, crate::ops::Constant::Undefined) = body.constant_at(13)? else {
        return None;
    };
    let crate::ops::Op::Branch {
        condition: branch,
        then_ops,
        else_ops,
    } = body.cold_at(14)?
    else {
        return None;
    };
    let then_ops = then_ops.code()?;
    let reset = then_ops.instruction(0)?;
    let result = body.instruction(15)?;
    (is_local_load(loaded)
        && loaded.b == pair
        && (lhs, rhs) == (loaded.a, body.instruction(11)?.a)
        && *branch == condition
        && then_ops.len() == 1
        && reset.opcode == crate::ir::Opcode::Move
        && reset.flags == 1
        && reset.a == undefined
        && reset.c == pair
        && else_ops.code()?.is_empty()
        && result.opcode == crate::ir::Opcode::Move
        && (result.a, result.b) == (dst, undefined))
        .then_some(reset.b)
}
