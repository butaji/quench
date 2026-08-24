const SHAPE_KERNEL_FACT_SLOTS: usize = 256;

#[derive(Clone, Copy)]
struct StatePredicatePlan {
    state_pc: usize,
    held_slot: u16,
    suspended_slot: u16,
}

#[derive(Clone, Copy)]
struct StateBitwisePlan {
    state_pc: usize,
    mask_slot: u16,
    operator: crate::ops::BinaryOp,
}

#[derive(Clone, Copy)]
enum ShapeKernelPlan {
    StatePredicate(StatePredicatePlan),
    StateBitwise(StateBitwisePlan),
}

#[derive(Clone)]
struct ShapeKernelFact {
    function: std::rc::Weak<crate::value::FunctionValue>,
    plan: Option<ShapeKernelPlan>,
}

thread_local! {
    static SHAPE_KERNEL_FACTS: std::cell::RefCell<Vec<Option<ShapeKernelFact>>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

fn execute_shape_kernel(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    receiver: &crate::value::Value,
) -> Option<crate::value::Value> {
    let plan = shape_kernel_fact(function)?;
    match plan {
        ShapeKernelPlan::StatePredicate(plan) => execute_state_predicate(function, receiver, plan),
        ShapeKernelPlan::StateBitwise(plan) => execute_state_bitwise(function, receiver, plan),
    }
}

fn execute_state_predicate(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    receiver: &crate::value::Value,
    plan: StatePredicatePlan,
) -> Option<crate::value::Value> {
    let crate::value::Value::Object(object) = receiver else { return None };
    let code = function.code.code()?;
    let metadata = code.metadata_at(plan.state_pc)?;
    let cell = crate::vm::get_named_cached_cell(object, &metadata.named_cache)?;
    // SAFETY: `receiver` owns the object and therefore its property cell for
    // the duration of this non-mutating kernel.
    let state = unsafe { &*cell }.load_number()?;
    let state_i32 = exact_i32(state)?;
    let held = exact_i32(function.captures.get_number(plan.held_slot)?)?;
    let suspended = function.captures.get_number(plan.suspended_slot)?;
    crate::execution_trace::event(crate::execution_trace::Event::ShapeKernelHit);
    crate::execution_trace::kernel("shape_state_predicate", false);
    Some(crate::value::Value::Boolean(
        (state_i32 & held) != 0 || state == suspended,
    ))
}

fn execute_state_bitwise(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    receiver: &crate::value::Value,
    plan: StateBitwisePlan,
) -> Option<crate::value::Value> {
    let crate::value::Value::Object(object) = receiver else { return None };
    let metadata = function.code.code()?.metadata_at(plan.state_pc)?;
    let cell = crate::vm::get_named_cached_cell(object, &metadata.named_cache)?;
    // SAFETY: the receiver retains the cached ordinary data-property cell.
    let cell = unsafe { &*cell };
    let state = exact_i32(cell.load_number()?)?;
    let mask = exact_i32(function.captures.get_number(plan.mask_slot)?)?;
    let state = match plan.operator {
        crate::ops::BinaryOp::BitwiseOr => state | mask,
        crate::ops::BinaryOp::BitwiseAnd => state & mask,
        _ => return None,
    };
    cell.store(crate::value::Value::Number(f64::from(state)));
    crate::execution_trace::event(crate::execution_trace::Event::ShapeKernelHit);
    crate::execution_trace::kernel("shape_state_bitwise", false);
    Some(crate::value::Value::Undefined)
}

fn exact_i32(value: f64) -> Option<i32> {
    (value.is_finite()
        && value.fract() == 0.0
        && value >= f64::from(i32::MIN)
        && value <= f64::from(i32::MAX))
    .then_some(value as i32)
}

fn shape_kernel_fact(
    function: &std::rc::Rc<crate::value::FunctionValue>,
) -> Option<ShapeKernelPlan> {
    let index = (std::rc::Rc::as_ptr(function) as usize >> 4) & (SHAPE_KERNEL_FACT_SLOTS - 1);
    let cached = SHAPE_KERNEL_FACTS.with(|facts| facts.borrow().get(index).and_then(Clone::clone));
    if let Some(cached) = cached.filter(|cached| {
        cached.function.upgrade().is_some_and(|value| std::rc::Rc::ptr_eq(&value, function))
    }) {
        return cached.plan;
    }
    let plan = match_state_predicate(function)
        .map(ShapeKernelPlan::StatePredicate)
        .or_else(|| match_state_bitwise(function).map(ShapeKernelPlan::StateBitwise));
    SHAPE_KERNEL_FACTS.with(|facts| {
        let mut facts = facts.borrow_mut();
        if facts.is_empty() {
            facts.resize_with(SHAPE_KERNEL_FACT_SLOTS, || None);
        }
        facts[index] = Some(ShapeKernelFact {
            function: std::rc::Rc::downgrade(function),
            plan,
        });
    });
    plan
}

fn match_state_predicate(
    function: &std::rc::Rc<crate::value::FunctionValue>,
) -> Option<StatePredicatePlan> {
    let code = function.code.code()?;
    let load_this = code.instruction(0)?;
    let get_state = code.instruction(1)?;
    let load_held = code.instruction(2)?;
    let bit_and = code.instruction(3)?;
    let load_zero = code.instruction(4)?;
    let not_equal = code.instruction(5)?;
    let conditional = code.instruction(6)?;
    if code.len() != 10
        || load_this.opcode != crate::ir::Opcode::LoadLocalChecked
        || get_state.opcode != crate::ir::Opcode::GetN
        || load_held.opcode != crate::ir::Opcode::LoadLocalChecked
        || bit_and.opcode != crate::ir::Opcode::Binary
        || bit_and.flags != crate::ir::compact_binary_id(crate::ops::BinaryOp::BitwiseAnd)
        || (bit_and.b, bit_and.c) != (get_state.a, load_held.a)
        || !matches!(code.constant_at(4), Some((_, crate::ops::Constant::Number(0.0))))
        || load_zero.opcode != crate::ir::Opcode::LoadConst
        || not_equal.opcode != crate::ir::Opcode::Binary
        || not_equal.flags != crate::ir::compact_binary_id(crate::ops::BinaryOp::NotEqual)
        || (not_equal.b, not_equal.c) != (bit_and.a, load_zero.a)
    {
        return None;
    }
    let alternate = match code.cold(conditional)? {
        crate::ops::Op::Conditional {
            condition,
            consequent,
            alternate,
            ..
        } if *condition == not_equal.a
            && fragment_returns(consequent.code(), not_equal.a) => alternate.code()?,
        _ => return None,
    };
    match_state_predicate_alternate(code, load_this, load_held, alternate)
}

fn match_state_predicate_alternate(
    code: crate::machine::CodeView<'_>,
    main_this: crate::ir::Instruction,
    load_held: crate::ir::Instruction,
    alternate: crate::machine::CodeView<'_>,
) -> Option<StatePredicatePlan> {
    let load_this = alternate.instruction(0)?;
    let get_again = alternate.instruction(1)?;
    let load_suspended = alternate.instruction(2)?;
    let equal = alternate.instruction(3)?;
    if alternate.len() != 5
        || load_this.opcode != crate::ir::Opcode::LoadLocalChecked
        || get_again.opcode != crate::ir::Opcode::GetN
        || load_suspended.opcode != crate::ir::Opcode::LoadLocalChecked
        || equal.opcode != crate::ir::Opcode::Binary
        || equal.flags != crate::ir::compact_binary_id(crate::ops::BinaryOp::Equal)
        || load_this.b != main_this.b
        || (equal.b, equal.c) != (get_again.a, load_suspended.a)
        || code.metadata_at(1)?.name != alternate.metadata_at(1)?.name
        || !fragment_returns(Some(alternate), equal.a)
    {
        return None;
    }
    Some(StatePredicatePlan {
        state_pc: 1,
        held_slot: load_held.b,
        suspended_slot: load_suspended.b,
    })
}

fn match_state_bitwise(
    function: &std::rc::Rc<crate::value::FunctionValue>,
) -> Option<StateBitwisePlan> {
    let code = function.code.code()?;
    let load_this = code.instruction(0)?;
    let move_one = code.instruction(1)?;
    let move_two = code.instruction(2)?;
    let load_again = code.instruction(3)?;
    let get_state = code.instruction(4)?;
    let load_mask = code.instruction(5)?;
    let bit_or = code.instruction(6)?;
    let set_state = code.instruction(7)?;
    let load_undefined = code.instruction(8)?;
    let return_op = code.instruction(9)?;
    let operator = compact_bitwise_operator(bit_or.flags)?;
    if !state_bitwise_ops_match(
        code,
        [
            load_this,
            move_one,
            move_two,
            load_again,
            get_state,
            load_mask,
            bit_or,
            set_state,
            load_undefined,
            return_op,
        ],
    ) {
        return None;
    }
    Some(StateBitwisePlan {
        state_pc: 4,
        mask_slot: load_mask.b,
        operator,
    })
}

fn compact_bitwise_operator(flags: u8) -> Option<crate::ops::BinaryOp> {
    let operator = crate::ir::compact_binary_operator(flags)?;
    matches!(operator, crate::ops::BinaryOp::BitwiseOr | crate::ops::BinaryOp::BitwiseAnd)
        .then_some(operator)
}

fn state_bitwise_ops_match(
    code: crate::machine::CodeView<'_>,
    ops: [crate::ir::Instruction; 10],
) -> bool {
    let [load_this, move_one, move_two, load_again, get_state, load_mask, bit_or, set_state, load_undefined, return_op] = ops;
    code.len() == 10
        && load_this.opcode == crate::ir::Opcode::LoadLocalChecked
        && (move_one.opcode, move_one.b) == (crate::ir::Opcode::Move, load_this.a)
        && (move_two.opcode, move_two.b) == (crate::ir::Opcode::Move, move_one.a)
        && (load_again.opcode, load_again.b) == (crate::ir::Opcode::LoadLocalChecked, load_this.b)
        && (get_state.opcode, get_state.b) == (crate::ir::Opcode::GetN, load_again.a)
        && load_mask.opcode == crate::ir::Opcode::LoadLocalChecked
        && bit_or.opcode == crate::ir::Opcode::Binary
        && compact_bitwise_operator(bit_or.flags).is_some()
        && (bit_or.b, bit_or.c) == (get_state.a, load_mask.a)
        && (set_state.opcode, set_state.a, set_state.b)
            == (crate::ir::Opcode::SetN, move_two.a, bit_or.a)
        && code.metadata_at(4).and_then(|meta| meta.name.as_deref())
            == code.metadata_at(7).and_then(|meta| meta.name.as_deref())
        && matches!(code.constant_at(8), Some((_, crate::ops::Constant::Undefined)))
        && load_undefined.opcode == crate::ir::Opcode::LoadConst
        && (return_op.opcode, return_op.a) == (crate::ir::Opcode::Return, load_undefined.a)
}

fn fragment_returns(code: Option<crate::machine::CodeView<'_>>, register: u16) -> bool {
    code.is_some_and(|code| {
        code.instruction(code.len().saturating_sub(1)).is_some_and(|op| {
            op.opcode == crate::ir::Opcode::Return && op.a == register
        })
    })
}
