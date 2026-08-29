fn state_bitwise_word_transition(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    object: &crate::value::ObjectData,
    operator: crate::ops::BinaryOp,
) -> Option<(*const crate::register_file::SlotWord, f64)> {
    let ShapeKernelPlan::StateBitwise(plan) = shape_kernel_fact(function)? else {
        return None;
    };
    if plan.operator != operator {
        return None;
    }
    let name = function
        .code
        .code()?
        .metadata_at(plan.state_pc)?
        .name
        .as_deref()?;
    let state = writable_own_word(object, name)?;
    let current = exact_i32(state.number()?)?;
    let mask = exact_i32(function.captures.get_number(plan.mask_slot)?)?;
    let next = match operator {
        crate::ops::BinaryOp::BitwiseOr => current | mask,
        crate::ops::BinaryOp::BitwiseAnd => current & mask,
        _ => return None,
    };
    Some((std::ptr::from_ref(state), f64::from(next)))
}

struct SchedulerSuspendWordTransition {
    state: *const crate::register_file::SlotWord,
    next_state: f64,
    current: crate::value::Value,
}

impl SchedulerSuspendWordTransition {
    fn execute(self) -> crate::value::Value {
        // SAFETY: admission retains `current`, proves its ordinary own state
        // word, and performs no shape mutation before this store.
        unsafe { &*self.state }.store(crate::value::Value::Number(self.next_state));
        self.current
    }
}

fn scheduler_suspend_word_transition(
    callee: &crate::value::Value,
    scheduler: &crate::value::Value,
) -> Option<SchedulerSuspendWordTransition> {
    let crate::value::Value::Function(suspend) = callee else {
        return None;
    };
    let code = match_scheduler_suspend(suspend)?;
    let crate::value::Value::Object(scheduler) = scheduler else {
        return None;
    };
    let current = crate::vm::get_named_cached_object(scheduler, &code.metadata_at(1)?.named_cache)?;
    let crate::value::Value::Object(current_object) = &current else {
        return None;
    };
    if current_object.has_replacement() {
        return None;
    }
    let mark = cached_shape_method(&current, code, 2)?;
    let crate::value::Value::Function(mark) = mark else {
        return None;
    };
    let (state, next_state) =
        state_bitwise_word_transition(&mark, current_object, crate::ops::BinaryOp::BitwiseOr)?;
    Some(SchedulerSuspendWordTransition {
        state,
        next_state,
        current,
    })
}

fn match_scheduler_suspend(
    function: &crate::value::FunctionValue,
) -> Option<crate::machine::CodeView<'_>> {
    let code = function.code.code()?;
    if function.params != 0 || function.code.capture_slots().len() != 1 || code.len() != 8 {
        return None;
    }
    let ops: [_; 8] = std::array::from_fn(|pc| code.instruction(pc).unwrap());
    let current = code.metadata_at(1)?.name.as_deref()?;
    let mark = code.metadata_at(2)?.name.as_deref()?;
    let current_again = code.metadata_at(4)?.name.as_deref()?;
    (is_local_load(ops[0])
        && (ops[1].opcode, ops[1].b) == (crate::ir::Opcode::GetN, ops[0].a)
        && (ops[2].opcode, ops[2].flags, ops[2].b) == (crate::ir::Opcode::CallN, 0, ops[1].a)
        && is_local_load(ops[3])
        && (ops[4].opcode, ops[4].b) == (crate::ir::Opcode::GetN, ops[3].a)
        && (ops[5].opcode, ops[5].a) == (crate::ir::Opcode::Return, ops[4].a)
        && current == current_again
        && !mark.is_empty())
    .then_some(code)
}

#[inline(always)]
fn cached_shape_number(
    object: &crate::value::ObjectData,
    cache: &std::cell::Cell<u64>,
) -> Option<f64> {
    match crate::vm::get_named_cached_payload(object, cache)? {
        crate::vm::NamedCachedPayload::Word(word) => unsafe { &*word }.number(),
        crate::vm::NamedCachedPayload::Cell(cell) => unsafe { &*cell }.load_number(),
        crate::vm::NamedCachedPayload::Value(value) => value.as_number(),
    }
}
