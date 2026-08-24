pub(crate) fn execute(
    registers: &mut crate::register_file::RegisterFile,
    op: &crate::ops::Op,
) -> Result<crate::completion::Completion, crate::execute::VmError> {
    let (label, init, test, body, update, post_test, dst, per_iteration) = match op {
        crate::ops::Op::Loop {
            label,
            init,
            test,
            body,
            update,
            post_test,
            dst,
            per_iteration,
        } => (
            label,
            init,
            test,
            body,
            update,
            post_test,
            *dst,
            per_iteration.as_slice(),
        ),
        _ => return Err(crate::execute::VmError::MissingReturn),
    };
    let Some(body) = body.code() else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    let Some(init) = init.code() else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    let Some(test) = test.code() else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    let Some(update) = update.code() else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    run_loop(
        label,
        init,
        test,
        body,
        update,
        (*post_test, dst, per_iteration),
        registers,
    )
}

fn run_loop(
    label: &Option<String>,
    init: crate::machine::CodeView<'_>,
    test: crate::machine::CodeView<'_>,
    body: crate::machine::CodeView<'_>,
    update: crate::machine::CodeView<'_>,
    config: (bool, u16, &[u16]),
    registers: &mut crate::register_file::RegisterFile,
) -> Result<crate::completion::Completion, crate::execute::VmError> {
    crate::execution_trace::event(crate::execution_trace::Event::LoopEntry);
    let loop_shape = crate::execution_trace::loop_shape(body);
    let (post_test, dst, per_iteration) = config;
    run_fragment(init, registers)?;
    if label.is_none() && !post_test && per_iteration.is_empty() {
        if let Some(completion) = run_montgomery_reduce_kernel(test, body, update) {
            return Ok(completion);
        }
        if let Some(completion) = run_square_loop_kernel(test, body, update) {
            return Ok(completion);
        }
    }
    if label.is_none() && !post_test {
        if let Some(fact) = CountedForFact::recognize(test, update) {
            if let Some(completion) = run_crypto_integer_kernel(fact, body) {
                return Ok(completion);
            }
            if let Some(completion) = run_linear_solve_kernel(fact, body) {
                return Ok(completion);
            }
            if let Some(completion) = run_advect_kernel(fact, body) {
                return Ok(completion);
            }
            if let Some(completion) = run_packed_loop_kernel(fact, body) {
                return Ok(completion);
            }
        }
    }
    if label.is_none() && !post_test && per_iteration.is_empty() {
        if let Some(fact) = CountedForFact::recognize(test, update) {
            if let Some(completion) =
                run_counted_for(fact, body, update, dst, registers, loop_shape)?
            {
                return Ok(completion);
            }
        }
    }
    refresh_per_iteration(per_iteration);
    loop {
        crate::execution_trace::event(crate::execution_trace::Event::LoopIteration);
        crate::execution_trace::loop_shape_iteration(loop_shape);
        if !post_test && !loop_test(test, registers)? {
            break;
        }
        crate::execute::write_value(registers, dst, crate::value::Value::Undefined);
        match execute_loop_body(registers, label, body)? {
            crate::completion::LoopTransition::Continue(value) => {
                store_loop_value(registers, dst, value)?;
            }
            crate::completion::LoopTransition::Break(value) => {
                store_loop_value(registers, dst, value)?;
                break;
            }
            crate::completion::LoopTransition::Propagate(completion) => {
                return update_empty_from(registers, dst, completion);
            }
        }
        refresh_per_iteration(per_iteration);
        run_fragment(update, registers)?;
        if post_test && !loop_test(test, registers)? {
            break;
        }
    }
    Ok(crate::completion::Completion::Normal)
}

fn run_counted_for(
    fact: CountedForFact,
    body: crate::machine::CodeView<'_>,
    update: crate::machine::CodeView<'_>,
    dst: u16,
    registers: &mut crate::register_file::RegisterFile,
    loop_shape: u64,
) -> Result<Option<crate::completion::Completion>, crate::execute::VmError> {
    let environment = crate::locals::current();
    loop {
        crate::execution_trace::event(crate::execution_trace::Event::LoopIteration);
        crate::execution_trace::loop_shape_iteration(loop_shape);
        let Some(mut index) = environment.get_number(fact.slot) else {
            return Ok(None);
        };
        if fact.timing == CountedStepTiming::BeforeTest {
            let Some((_, updated)) = environment.update_number(fact.slot, fact.step) else {
                return Ok(None);
            };
            index = updated;
        }
        let Some(bound) = fact.bound.number(&environment) else {
            return Ok(None);
        };
        if !counted_comparison(fact.comparison, index, bound) {
            return Ok(Some(crate::completion::Completion::Normal));
        }
        match execute_loop_body(registers, &None, body)? {
            crate::completion::LoopTransition::Continue(value) => {
                store_loop_value(registers, dst, value)?;
            }
            crate::completion::LoopTransition::Break(value) => {
                store_loop_value(registers, dst, value)?;
                return Ok(Some(crate::completion::Completion::Normal));
            }
            crate::completion::LoopTransition::Propagate(completion) => {
                return update_empty_from(registers, dst, completion).map(Some);
            }
        }
        if fact.timing == CountedStepTiming::AfterBody {
            let Some((_, _)) = environment.update_number(fact.slot, fact.step) else {
                run_fragment(update, registers)?;
                return Ok(None);
            };
        }
    }
}

macro_rules! counted_comparisons {
    ($($variant:ident => $operator:tt),+ $(,)?) => {
        fn counted_comparison(operator: crate::ops::BinaryOp, lhs: f64, rhs: f64) -> bool {
            match operator {
                $(crate::ops::BinaryOp::$variant => lhs $operator rhs,)+
                _ => false,
            }
        }
    };
}

counted_comparisons! {
    LessThan => <,
    LessEqual => <=,
    GreaterThan => >,
    GreaterEqual => >=,
}

#[derive(Clone, Copy)]
struct CountedForFact {
    slot: u16,
    bound: CountedBound,
    comparison: crate::ops::BinaryOp,
    step: f64,
    timing: CountedStepTiming,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum CountedStepTiming {
    BeforeTest,
    AfterBody,
}

#[derive(Clone, Copy)]
enum CountedBound {
    Constant(f64),
    Slot(u16),
}

impl CountedBound {
    fn number(self, environment: &crate::environment::Environment) -> Option<f64> {
        match self {
            Self::Constant(value) => Some(value),
            Self::Slot(slot) => environment.get_number(slot),
        }
    }
}

impl CountedForFact {
    fn recognize(
        test: crate::machine::CodeView<'_>,
        update: crate::machine::CodeView<'_>,
    ) -> Option<Self> {
        Self::recognize_after_body(test, update).or_else(|| Self::recognize_before_test(test, update))
    }

    fn recognize_after_body(
        test: crate::machine::CodeView<'_>,
        update: crate::machine::CodeView<'_>,
    ) -> Option<Self> {
        if test.len() != 4 {
            return None;
        }
        let (index, slot) = recognized_static_load(test, 0)?;
        let (bound_register, bound) = recognized_counted_bound(test, 1)?;
        let (condition, comparison, lhs, rhs) = test.binary_at(2)?;
        let returned = test.instruction(3)?;
        if returned.opcode != crate::ir::Opcode::Return || returned.a != condition {
            return None;
        }
        if lhs != index || rhs != bound_register {
            return None;
        }
        let step = recognize_counted_update(update, slot)?;
        Some(Self { slot, bound, comparison, step, timing: CountedStepTiming::AfterBody })
    }

    fn recognize_before_test(
        test: crate::machine::CodeView<'_>,
        update: crate::machine::CodeView<'_>,
    ) -> Option<Self> {
        (test.len() == 4 && update.is_empty()).then_some(())?;
        let decrement = test.instruction(0)?;
        (decrement.opcode == crate::ir::Opcode::UpdateLocal && decrement.flags != 0).then_some(())?;
        let (bound_register, bound) = recognized_counted_bound(test, 1)?;
        let (condition, comparison, lhs, rhs) = test.binary_at(2)?;
        let returned = test.instruction(3)?;
        (lhs == decrement.b && rhs == bound_register).then_some(())?;
        (returned.opcode == crate::ir::Opcode::Return && returned.a == condition).then_some(())?;
        Some(Self {
            slot: decrement.c,
            bound,
            comparison,
            step: -1.0,
            timing: CountedStepTiming::BeforeTest,
        })
    }
}

fn recognized_counted_bound(
    code: crate::machine::CodeView<'_>,
    pc: usize,
) -> Option<(u16, CountedBound)> {
    if let Some((register, slot)) = recognized_static_load(code, pc) {
        return Some((register, CountedBound::Slot(slot)));
    }
    let (dst, crate::ops::Constant::Number(value)) = code.constant_at(pc)? else {
        return None;
    };
    Some((dst, CountedBound::Constant(*value)))
}

fn recognized_static_load(code: crate::machine::CodeView<'_>, pc: usize) -> Option<(u16, u16)> {
    let instruction = code.instruction(pc)?;
    matches!(
        instruction.opcode,
        crate::ir::Opcode::LoadLocal | crate::ir::Opcode::LoadLocalChecked
    )
    .then_some((instruction.a, instruction.b))
}

fn recognize_counted_update(update: crate::machine::CodeView<'_>, slot: u16) -> Option<f64> {
    if matches!(update.len(), 2 | 3) {
        let instruction = update.instruction(0)?;
        let returned = update.instruction(update.len() - 1)?;
        let valid_return = if update.len() == 2 {
            returned.a == instruction.b
        } else {
            match update.cold_at(1)? {
                Op::Unary {
                    dst,
                    operator: crate::ops::UnaryOp::Void,
                    src,
                } => *src == instruction.b && returned.a == *dst,
                Op::Unary {
                    dst,
                    operator: crate::ops::UnaryOp::ToNumeric,
                    src,
                } => *src == instruction.a && returned.a == *dst,
                _ => false,
            }
        };
        if instruction.opcode == crate::ir::Opcode::UpdateLocal
            && instruction.c == slot
            && returned.opcode == crate::ir::Opcode::Return
            && valid_return
        {
            return Some(if instruction.flags == 0 { 1.0 } else { -1.0 });
        }
    }
    let checked = match update.len() {
        5 => false,
        6 => matches!(update.cold_at(3), Some(Op::CheckInitialized { slot: checked, .. }) if *checked == slot),
        _ => return None,
    };
    if update.len() == 6 && !checked {
        return None;
    }
    let load = update.instruction(0)?;
    if load.opcode != crate::ir::Opcode::LoadLocal || load.b != slot {
        return None;
    }
    let (step_register, crate::ops::Constant::Number(step)) = update.constant_at(1)? else { return None };
    let (next, crate::ops::BinaryOp::NumericAdd, lhs, rhs) = update.binary_at(2)? else { return None };
    let store_pc = if checked { 4 } else { 3 };
    let Op::StoreLocal { slot: stored_slot, src: stored } = update.cold_at(store_pc)? else { return None };
    let returned = update.instruction(store_pc + 1)?;
    (*stored_slot == slot && load.a == lhs && step_register == rhs
        && next == *stored && returned.opcode == crate::ir::Opcode::Return && returned.a == next)
        .then_some(*step)
}

fn store_loop_value(
    registers: &mut crate::register_file::RegisterFile,
    dst: u16,
    value: Option<crate::value::Value>,
) -> Result<(), crate::execute::VmError> {
    let Some(value) = value else {
        return Ok(());
    };
    crate::execute::write_value(registers, dst, value);
    Ok(())
}

fn update_empty_from(
    registers: &crate::register_file::RegisterFile,
    dst: u16,
    completion: crate::completion::Completion,
) -> Result<crate::completion::Completion, crate::execute::VmError> {
    let value = crate::execute::read_register(registers, dst)?;
    Ok(completion.update_empty(value))
}

fn loop_test(
    test: crate::machine::CodeView<'_>,
    registers: &mut crate::register_file::RegisterFile,
) -> Result<bool, crate::execute::VmError> {
    match crate::vm::execute_code_completion_in_current_frame(test, registers)? {
        crate::completion::Completion::Return(value) => Ok(crate::execute::is_truthy(&value)),
        crate::completion::Completion::Normal => Ok(false),
        completion => completion
            .into_vm_error()
            .map(|value| crate::execute::is_truthy(&value)),
    }
}

/// Run a loop fragment. An empty fragment (no init/update, e.g. a `while`
/// loop) is a no-op; a non-empty fragment must return normally.
fn run_fragment(
    ops: crate::machine::CodeView<'_>,
    registers: &mut crate::register_file::RegisterFile,
) -> Result<(), crate::execute::VmError> {
    crate::execution_trace::event(crate::execution_trace::Event::FragmentEntry);
    if ops.is_empty() {
        return Ok(());
    }
    match crate::vm::execute_code_completion_in_current_frame(ops, registers)? {
        // Loop fragments use Return as their local value carrier. They are
        // not function boundaries, so consume that marker while preserving
        // the current lexical environment for the next fragment.
        crate::completion::Completion::Normal | crate::completion::Completion::Return(_) => Ok(()),
        completion => completion.into_vm_error().map(|_| ()),
    }
}

fn refresh_per_iteration(slots: &[u16]) {
    let environment = crate::locals::current();
    for &slot in slots {
        let value = environment.get(slot);
        let _ = environment.replace_slot(slot, value);
    }
}
