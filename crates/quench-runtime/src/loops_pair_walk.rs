#[derive(Clone, Copy)]
struct PairWalkPlan {
    head: u16,
    pair: u16,
    total: u16,
    car_pc: usize,
    cdr_pc: usize,
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
    let car_name = body.metadata_at(plan.car_pc)?.name.as_deref()?;
    let cdr_name = body.metadata_at(plan.cdr_pc)?.name.as_deref()?;
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
        car_name,
        cdr_name,
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
    car_name: &str,
    cdr_name: &str,
) -> Option<crate::completion::Completion> {
    let head_pointer = std::rc::Rc::as_ptr(&head);
    let mut pair_pointer = std::rc::Rc::as_ptr(&pair);
    let shape = head.semantic_layout_id();
    let car = head.hot_properties().position_rev(car_name)?;
    let cdr = head.hot_properties().position_rev(cdr_name)?;
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

fn without_object_coercions(code: crate::machine::CodeView<'_>) -> Vec<usize> {
    (0..code.len())
        .filter(|&pc| {
            match code.cold_at(pc) {
                Some(crate::ops::Op::RequireObjectCoercible { .. }) => {
                    matches!(code.cold_at(pc + 1), Some(crate::ops::Op::ToPropertyKey { .. }))
                }
                _ => true,
            }
        })
        .collect()
}

fn recognize_pair_word_walk(
    fact: CountedForFact,
    body: crate::machine::CodeView<'_>,
    dst: u16,
    per_iteration: &[u16],
) -> Option<PairWalkPlan> {
    (per_iteration == [fact.slot]
        && fact.timing == CountedStepTiming::AfterBody
        && fact.step == 1.0)
        .then_some(())?;
    let pcs = without_object_coercions(body);
    let p = |vpc: usize| pcs.get(vpc).copied();
    let op = |vpc| body.instruction(p(vpc)?);
    let (sum, pair, car, add, store_sum, result_sum) = (op(0)?, op(1)?, op(2)?, op(3)?, op(4)?, op(5)?);
    let (pair_again, cdr, store_pair, result_pair) = (op(6)?, op(7)?, op(8)?, op(9)?);
    let head = recognize_pair_reset(body, &pcs, pair.b, dst)?;
    validate_pair_accesses(
        body,
        p(2)?,
        p(7)?,
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
        car_pc: p(2)?,
        cdr_pc: p(7)?,
    })
}

#[allow(clippy::too_many_arguments)]
fn validate_pair_accesses(
    body: crate::machine::CodeView<'_>,
    car_pc: usize,
    cdr_pc: usize,
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
        && body.metadata_at(car_pc)?.name.is_some()
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
        && body.metadata_at(cdr_pc)?.name.is_some()
        && is_local_store(store_pair)
        && (store_pair.a, store_pair.b) == (pair.b, cdr.a)
        && result_pair.opcode == crate::ir::Opcode::Move
        && (result_pair.a, result_pair.b) == (dst, cdr.a))
        .then_some(())
}

fn recognize_pair_reset(
    body: crate::machine::CodeView<'_>,
    pcs: &[usize],
    pair: u16,
    dst: u16,
) -> Option<u16> {
    let p = |vpc: usize| pcs.get(vpc).copied();
    let loaded = body.instruction(p(10)?)?;
    let (_, crate::ops::Constant::Null) = body.constant_at(p(11)?)? else {
        return None;
    };
    let (condition, crate::ops::BinaryOp::StrictEqual, lhs, rhs) = body.binary_at(p(12)?)? else {
        return None;
    };
    let (undefined, crate::ops::Constant::Undefined) = body.constant_at(p(13)?)? else {
        return None;
    };
    let crate::ops::Op::Branch {
        condition: branch,
        then_ops,
        else_ops,
    } = body.cold_at(p(14)?)?
    else {
        return None;
    };
    let then_ops = then_ops.code()?;
    let result = body.instruction(p(15)?)?;
    (is_local_load(loaded)
        && loaded.b == pair
        && (lhs, rhs) == (loaded.a, body.instruction(p(11)?)?.a)
        && *branch == condition
        && else_ops.code()?.is_empty()
        && result.opcode == crate::ir::Opcode::Move
        && (result.a, result.b) == (dst, undefined))
        .then_some(())?;
    reset_head_slot(then_ops, pair, undefined)
}

/// `if (pair === null) pair = head` then-arm: either one MoveLocal of the
/// saved head into the pair slot, or load-head / store-pair / move-to-dst.
fn reset_head_slot(
    then_ops: crate::machine::CodeView<'_>,
    pair: u16,
    undefined: u16,
) -> Option<u16> {
    let pcs = without_object_coercions(then_ops);
    if let [pc] = pcs.as_slice() {
        let reset = then_ops.instruction(*pc)?;
        return (reset.opcode == crate::ir::Opcode::Move
            && reset.flags == 1
            && reset.a == undefined
            && reset.c == pair)
            .then_some(reset.b);
    }
    let load = then_ops.instruction(*pcs.first()?)?;
    let store = then_ops.instruction(*pcs.get(1)?)?;
    let moved = then_ops.instruction(*pcs.get(2)?)?;
    (is_local_load(load)
        && is_local_store(store)
        && store.a == pair
        && store.b == load.a
        && moved.opcode == crate::ir::Opcode::Move
        && (moved.a, moved.b) == (undefined, load.a)
        && pcs.len() == 3)
        .then_some(load.b)
}

#[cfg(test)]
mod pair_walk_dump {
    use crate::ops::Op;

    fn collect_loops<'a>(
        code: crate::machine::CodeView<'a>,
        out: &mut Vec<(
            crate::machine::CodeView<'a>,
            crate::machine::CodeView<'a>,
            crate::machine::CodeView<'a>,
            crate::machine::CodeView<'a>,
            u16,
            Vec<u16>,
            bool,
        )>,
    ) {
        for pc in 0..code.len() {
            match code.cold_at(pc) {
                Some(Op::Loop {
                    init,
                    test,
                    body,
                    update,
                    dst,
                    per_iteration,
                    post_test,
                    ..
                }) => {
                    if let (Some(init), Some(test), Some(body), Some(update)) =
                        (init.code(), test.code(), body.code(), update.code())
                    {
                        out.push((
                            init,
                            test,
                            body,
                            update,
                            *dst,
                            per_iteration.clone(),
                            *post_test,
                        ));
                        collect_loops(init, out);
                        collect_loops(test, out);
                        collect_loops(body, out);
                        collect_loops(update, out);
                    }
                }
                Some(Op::MakeFunction { body, .. } | Op::MakeFunctionWithKind { body, .. }) => {
                    if let Some(code) = body.code() {
                        collect_loops(code, out);
                    }
                }
                Some(Op::Conditional {
                    consequent,
                    alternate,
                    ..
                }) => {
                    for fragment in [consequent, alternate] {
                        if let Some(code) = fragment.code() {
                            collect_loops(code, out);
                        }
                    }
                }
                Some(Op::Branch {
                    then_ops, else_ops, ..
                }) => {
                    for fragment in [then_ops, else_ops] {
                        if let Some(code) = fragment.code() {
                            collect_loops(code, out);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn dump_code(label: &str, code: crate::machine::CodeView<'_>) -> String {
        let mut lines = vec![format!(
            "{label} len={} filtered={:?}",
            code.len(),
            super::without_object_coercions(code)
        )];
        for pc in 0..code.len() {
            let op = code.instruction(pc);
            let name = code.metadata_at(pc).and_then(|meta| meta.name.clone());
            let constant = code.constant_at(pc).map(|(_, value)| format!("{value:?}"));
            let cold = code.cold_at(pc).map(|op| op.variant_name());
            let binary = code.binary_at(pc);
            lines.push(format!(
                "  {pc}: op={op:?} name={name:?} const={constant:?} cold={cold:?} binary={binary:?}"
            ));
        }
        lines.join("\n")
    }

    #[test]
    fn pair_car_cdr_body_is_dumped_against_recognizer() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/lanes/pair-car-cdr.js"
        );
        let source = std::fs::read_to_string(path).expect("pair-car-cdr micro");
        let program = crate::reduce::reduce_source(&source).expect("reduce pair-car-cdr");
        let mut loops = Vec::new();
        collect_loops(program.code(), &mut loops);
        let mut dump = vec![format!("loops={}", loops.len())];
        let mut admitted = 0;
        for (index, (init, test, body, update, dst, per_iteration, post_test)) in
            loops.iter().enumerate()
        {
            let fact = super::CountedForFact::recognize(*test, *update);
            let recognized = fact.and_then(|fact| {
                super::recognize_pair_word_walk(fact, *body, *dst, per_iteration)
            });
            if recognized.is_some() {
                admitted += 1;
            }
            dump.push(format!(
                "loop[{index}] dst={dst} post_test={post_test} per_iteration={per_iteration:?} fact={fact:?} admitted={}",
                recognized.is_some()
            ));
            dump.push(dump_code("init", *init));
            dump.push(dump_code("test", *test));
            dump.push(dump_code("body", *body));
            dump.push(dump_code("update", *update));
            for pc in 0..body.len() {
                if let Some(Op::Branch {
                    condition,
                    then_ops,
                    else_ops,
                }) = body.cold_at(pc)
                {
                    dump.push(format!("body[{pc}] Branch condition={condition}"));
                    if let Some(code) = then_ops.code() {
                        dump.push(dump_code("then", code));
                    }
                    if let Some(code) = else_ops.code() {
                        dump.push(dump_code("else", code));
                    }
                }
            }
            if let Some(fact) = fact {
                dump.push(format!(
                    "  fact.slot={} step={} timing={:?} comparison={:?} per_ok={} step_ok={}",
                    fact.slot,
                    fact.step,
                    fact.timing,
                    fact.comparison,
                    *per_iteration == [fact.slot],
                    fact.step == 1.0
                ));
            }
        }
        assert!(
            admitted > 0,
            "pair-car-cdr walk not recognized\n{}",
            dump.join("\n")
        );
        crate::vm::execute_code_with_context(program.code(), &crate::vm::VmContext::default())
            .expect("pair-car-cdr execute");
    }

    #[test]
    fn pair_walk_admits_renamed_cell_fields() {
        let source = r#"
            function Cell(first, rest) { this.first = first; this.rest = rest; }
            const tail = new Cell(2, null);
            const head = new Cell(1, tail);
            let cell = head;
            let total = 0;
            for (let i = 0; i < 8; i++) {
              total += cell.first;
              cell = cell.rest;
              if (cell === null) cell = head;
            }
            if (total !== 12) throw new Error("renamed pair walk");
        "#;
        let program = crate::reduce::reduce_source(source).expect("reduce renamed pair");
        let mut loops = Vec::new();
        collect_loops(program.code(), &mut loops);
        let admitted = loops.iter().any(|(_, test, body, update, dst, per_iteration, _)| {
            super::CountedForFact::recognize(*test, *update).is_some_and(|fact| {
                super::recognize_pair_word_walk(fact, *body, *dst, per_iteration).is_some()
            })
        });
        if !admitted {
            let mut dump = vec![format!("loops={}", loops.len())];
            for (index, (init, test, body, update, dst, per_iteration, post_test)) in
                loops.iter().enumerate()
            {
                let fact = super::CountedForFact::recognize(*test, *update);
                dump.push(format!(
                    "loop[{index}] dst={dst} post_test={post_test} per_iteration={per_iteration:?} fact={fact:?}"
                ));
                dump.push(dump_code("init", *init));
                dump.push(dump_code("test", *test));
                dump.push(dump_code("body", *body));
                dump.push(dump_code("update", *update));
                for pc in 0..body.len() {
                    if let Some(Op::Branch {
                        condition,
                        then_ops,
                        else_ops,
                    }) = body.cold_at(pc)
                    {
                        dump.push(format!("body[{pc}] Branch condition={condition}"));
                        if let Some(code) = then_ops.code() {
                            dump.push(dump_code("then", code));
                        }
                        if let Some(code) = else_ops.code() {
                            dump.push(dump_code("else", code));
                        }
                    }
                }
            }
            panic!(
                "first/rest pair walk not recognized among {} loops\n{}",
                loops.len(),
                dump.join("\n")
            );
        }
        crate::vm::execute_code_with_context(program.code(), &crate::vm::VmContext::default())
            .expect("renamed pair walk execute");
    }
}
