#[derive(Clone, Copy)]
struct RegexpExecLoopPlan {
    regexp: u16,
    matches: u16,
}

fn run_regexp_exec_loop(
    fact: CountedForFact,
    body: crate::machine::CodeView<'_>,
    dst: u16,
    per_iteration: &[u16],
    registers: &mut crate::register_file::RegisterFile,
    loop_shape: u64,
) -> Option<crate::completion::Completion> {
    let plan = recognize_regexp_exec_loop(fact, body, dst, per_iteration)?;
    let environment = crate::locals::current();
    let index = environment.get_number(fact.slot)?;
    let bound = fact.bound.number(&environment)?;
    let iterations = unit_less_than_iterations(index, bound)?;
    let mut matches = environment.get_number(plan.matches)?;
    let regexp = match crate::locals::resolved_replacement(environment.get(plan.regexp)) {
        crate::value::Value::Object(object) => object,
        _ => return None,
    };
    let receiver = crate::value::Value::Object(std::rc::Rc::clone(&regexp));
    let method = crate::execute::get_property_result(&receiver, "exec").ok()?;
    matches!(
        method,
        crate::value::Value::Builtin(crate::ops::Builtin::RegExpExec)
    )
    .then_some(())?;
    let (_, crate::ops::Constant::String(input)) = body.constant_at(8)? else {
        return None;
    };
    crate::regexp::repeat_exact_global_exec(&regexp, input)?;
    crate::execution_trace::event(crate::execution_trace::Event::CountedForAttempt);
    trace_regexp_exec_iterations(loop_shape, iterations);
    let previous = matches + iterations.saturating_sub(1) as f64;
    matches += iterations as f64;
    environment.set(plan.matches, crate::value::Value::Number(matches));
    environment.set(
        fact.slot,
        crate::value::Value::Number(index + iterations as f64),
    );
    if iterations != 0 {
        registers.write_number(usize::from(dst), previous);
    }
    Some(crate::completion::Completion::Normal)
}

#[cfg(feature = "execution-trace")]
fn trace_regexp_exec_iterations(loop_shape: u64, iterations: usize) {
    for _ in 0..iterations {
        crate::execution_trace::event(crate::execution_trace::Event::LoopIteration);
        crate::execution_trace::loop_shape_iteration(loop_shape);
        crate::execution_trace::event(crate::execution_trace::Event::CountedForHit);
        crate::execution_trace::last_index("header");
        crate::execution_trace::kernel("regexp_exact_global_exec", false);
    }
}

#[cfg(not(feature = "execution-trace"))]
fn trace_regexp_exec_iterations(_: u64, _: usize) {}

fn recognize_regexp_exec_loop(
    fact: CountedForFact,
    body: crate::machine::CodeView<'_>,
    dst: u16,
    per_iteration: &[u16],
) -> Option<RegexpExecLoopPlan> {
    (per_iteration == [fact.slot]
        && fact.comparison == crate::ops::BinaryOp::LessThan
        && fact.timing == CountedStepTiming::AfterBody
        && fact.step == 1.0
        && body.len() == 15)
        .then_some(())?;
    let op = |pc| body.instruction(pc).unwrap();
    let regexp = op(0);
    let set_base = op(1);
    let set_object = op(2);
    let set = op(4);
    let load_again = op(6);
    let method = op(7);
    let call = op(9);
    (is_local_load(regexp)
        && set_base.opcode == crate::ir::Opcode::Move
        && set_base.b == regexp.a
        && set_object.opcode == crate::ir::Opcode::Move
        && set_object.b == set_base.a
        && matches!(
            body.constant_at(3),
            Some((_, crate::ops::Constant::Number(0.0)))
        )
        && set.opcode == crate::ir::Opcode::SetN
        && set.a == set_object.a
        && body.metadata_at(4)?.name.as_deref() == Some("lastIndex")
        && is_local_load(load_again)
        && load_again.b == regexp.b
        && method.opcode == crate::ir::Opcode::GetN
        && method.b == load_again.a
        && body.metadata_at(7)?.name.as_deref() == Some("exec")
        && matches!(
            body.constant_at(8),
            Some((_, crate::ops::Constant::String(_)))
        )
        && call.opcode == crate::ir::Opcode::CallN
        && call.flags == 1
        && body.instruction(8)?.a + 1 == call.a
        && (call.b, call.c) == (load_again.a, method.a))
        .then_some(())?;
    let matches = recognize_regexp_match_increment(body, call.a, dst)?;
    Some(RegexpExecLoopPlan {
        regexp: regexp.b,
        matches,
    })
}

fn recognize_regexp_match_increment(
    body: crate::machine::CodeView<'_>,
    result: u16,
    dst: u16,
) -> Option<u16> {
    let (_, crate::ops::Constant::Null) = body.constant_at(10)? else {
        return None;
    };
    let (condition, crate::ops::BinaryOp::StrictNotEqual, lhs, rhs) = body.binary_at(11)? else {
        return None;
    };
    let (branch_value, crate::ops::Constant::Undefined) = body.constant_at(12)? else {
        return None;
    };
    let crate::ops::Op::Branch {
        condition: branch,
        then_ops,
        else_ops,
    } = body.cold_at(13)?
    else {
        return None;
    };
    let then_ops = then_ops.code()?;
    let update = then_ops.instruction(0)?;
    let moved = then_ops.instruction(1)?;
    let final_move = body.instruction(14)?;
    ((lhs, rhs) == (result, body.instruction(10)?.a)
        && *branch == condition
        && then_ops.len() == 2
        && update.opcode == crate::ir::Opcode::UpdateLocal
        && update.flags == 0
        && moved.opcode == crate::ir::Opcode::Move
        && (moved.a, moved.b) == (branch_value, update.a)
        && else_ops.code()?.is_empty()
        && final_move.opcode == crate::ir::Opcode::Move
        && (final_move.a, final_move.b) == (dst, branch_value))
        .then_some(update.c)
}
