const SHAPE_KERNEL_FACT_SLOTS: usize = 256;

/// The matcher consumes the semantic local-load fact. TDZ proof changes the
/// physical opcode from checked to unchecked without changing a shape plan.
#[inline]
fn is_local_load(instruction: crate::ir::Instruction) -> bool {
    matches!(
        instruction.opcode,
        crate::ir::Opcode::LoadLocal | crate::ir::Opcode::LoadLocalChecked
    )
}

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
struct NestedArrayLengthPlan {
    first_pc: usize,
    second_pc: usize,
}

#[derive(Clone, Copy)]
struct NestedArrayIndexPlan {
    first_pc: usize,
}

#[derive(Clone, Copy)]
struct ForwardZeroPlan {
    receiver_pc: usize,
    call_pc: usize,
}

#[derive(Clone, Copy)]
struct ForwardOnePlan {
    receiver_pc: usize,
    callee_pc: usize,
}

#[derive(Clone, Copy)]
struct CopyMethodPropertyPlan {
    output_call_pc: usize,
    input_call_pc: usize,
    get_pc: usize,
    set_pc: usize,
}

#[derive(Clone, Copy)]
enum ShapeKernelPlan {
    StatePredicate(StatePredicatePlan),
    StateBitwise(StateBitwisePlan),
    NestedArrayLength(NestedArrayLengthPlan),
    NestedArrayIndex(NestedArrayIndexPlan),
    PropertySelect(PropertySelectPlan),
    ForwardZero(ForwardZeroPlan),
    ForwardOne(ForwardOnePlan),
    CopyMethodProperty(CopyMethodPropertyPlan),
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

pub(crate) fn execute_shape_kernel(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    receiver: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Option<crate::value::Value> {
    // Async calls must enter the generator-backed completion path so throws
    // become Promise rejections. Shape kernels are only observationally safe
    // for synchronous functions.
    if function.is_async {
        return None;
    }
    let plan = shape_kernel_fact(function)?;
    match plan {
        ShapeKernelPlan::StatePredicate(plan) => execute_state_predicate(function, receiver, plan),
        ShapeKernelPlan::StateBitwise(plan) => execute_state_bitwise(function, receiver, plan),
        ShapeKernelPlan::NestedArrayLength(plan) => {
            execute_nested_array_length(function, receiver, plan)
        }
        ShapeKernelPlan::NestedArrayIndex(plan) => {
            execute_nested_array_index(function, receiver, arguments, plan)
        }
        ShapeKernelPlan::PropertySelect(plan) => execute_property_select(function, receiver, plan),
        ShapeKernelPlan::ForwardZero(plan) => execute_forward_zero(function, receiver, plan),
        ShapeKernelPlan::ForwardOne(plan) => {
            execute_forward_one(function, receiver, arguments, plan)
        }
        ShapeKernelPlan::CopyMethodProperty(plan) => {
            execute_copy_method_property(function, receiver, plan)
        }
    }
}

/// Execute a previously admitted zero-argument shape method from canonical
/// object and slot words. A missing fact or failed guard returns to CallN.
pub(crate) fn execute_shape_kernel_word(
    function: &crate::value::FunctionValue,
    receiver: &crate::value::ObjectData,
) -> Option<f64> {
    let ShapeKernelPlan::NestedArrayLength(plan) = cached_shape_kernel_fact(function)? else {
        return None;
    };
    let code = function.code.code()?;
    let metadata = code.metadata_at(plan.first_pc)?;
    let crate::vm::NamedCachedPayload::Word(word) =
        crate::vm::get_named_cached_payload(receiver, &metadata.named_cache)?
    else {
        return None;
    };
    // SAFETY: the receiver owns its slot and the slot owns the array for the
    // duration of this call.
    let array = unsafe { &*(&*word).array_ptr()? };
    crate::locals::array_word_is_current(array).then_some(())?;
    crate::execution_trace::event(crate::execution_trace::Event::ShapeKernelHit);
    crate::execution_trace::kernel("shape_nested_array_length", false);
    Some(array.header_length() as f64)
}

pub(crate) fn is_shape_kernel_candidate(function: &crate::value::FunctionValue) -> bool {
    function.code.code().is_some_and(|code| {
        matches!(code.len(), 6..=9)
            && code
                .instruction(1)
                .is_some_and(|op| op.opcode == crate::ir::Opcode::GetN)
    })
}

fn execute_nested_array_index(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    receiver: &crate::value::Value,
    arguments: &[crate::value::Value],
    plan: NestedArrayIndexPlan,
) -> Option<crate::value::Value> {
    let crate::value::Value::Object(receiver) = receiver else {
        return None;
    };
    let first = function.code.code()?.metadata_at(plan.first_pc)?;
    let nested = crate::locals::resolved_replacement(crate::vm::get_named_cached_object(
        receiver,
        &first.named_cache,
    )?);
    let crate::value::Value::Array(array) = nested else {
        return None;
    };
    let index = arguments.first()?.as_number()?;
    if !array.is_packed_ordinary()
        || !index.is_finite()
        || index.fract() != 0.0
        || index < 0.0
        || index > usize::MAX as f64
    {
        return None;
    }
    let index = index as usize;
    let value = array
        .dense_number_at(index)
        .map(crate::value::Value::Number)
        .or_else(|| array.dense_value_at(index))?;
    crate::execution_trace::event(crate::execution_trace::Event::ShapeKernelHit);
    crate::execution_trace::kernel("shape_nested_array_index", false);
    Some(value)
}

fn execute_nested_array_length(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    receiver: &crate::value::Value,
    plan: NestedArrayLengthPlan,
) -> Option<crate::value::Value> {
    let crate::value::Value::Object(receiver) = receiver else {
        return None;
    };
    let code = function.code.code()?;
    let first = code.metadata_at(plan.first_pc)?;
    let nested = crate::vm::get_named_cached_object(receiver, &first.named_cache)?;
    let nested = crate::locals::resolved_replacement(nested);
    let second = code.metadata_at(plan.second_pc)?;
    let value = match nested {
        crate::value::Value::Array(array) if second.name.as_deref() == Some("length") => {
            crate::arrays::property(&array, "length")
        }
        _ => return None,
    };
    crate::execution_trace::event(crate::execution_trace::Event::ShapeKernelHit);
    crate::execution_trace::kernel("shape_nested_array_length", false);
    Some(value)
}

fn execute_state_predicate(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    receiver: &crate::value::Value,
    plan: StatePredicatePlan,
) -> Option<crate::value::Value> {
    let crate::value::Value::Object(object) = receiver else {
        return None;
    };
    let code = function.code.code()?;
    let metadata = code.metadata_at(plan.state_pc)?;
    let state = cached_shape_number(object, &metadata.named_cache)?;
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
    let crate::value::Value::Object(object) = receiver else {
        return None;
    };
    let metadata = function.code.code()?.metadata_at(plan.state_pc)?;
    let payload = crate::vm::get_named_cached_payload(object, &metadata.named_cache)?;
    let state = exact_i32(match &payload {
        crate::vm::NamedCachedPayload::Word(word) => unsafe { &**word }.number()?,
        crate::vm::NamedCachedPayload::Cell(cell) => unsafe { &**cell }.load_number()?,
        crate::vm::NamedCachedPayload::Value(value) => value.as_number()?,
    })?;
    let mask = exact_i32(function.captures.get_number(plan.mask_slot)?)?;
    let state = match plan.operator {
        crate::ops::BinaryOp::BitwiseOr => state | mask,
        crate::ops::BinaryOp::BitwiseAnd => state & mask,
        _ => return None,
    };
    match payload {
        crate::vm::NamedCachedPayload::Word(word) => {
            unsafe { &*word }.store(crate::value::Value::Number(f64::from(state)))
        }
        crate::vm::NamedCachedPayload::Cell(cell) => {
            unsafe { &*cell }.store(crate::value::Value::Number(f64::from(state)))
        }
        crate::vm::NamedCachedPayload::Value(_) => return None,
    }
    crate::execution_trace::event(crate::execution_trace::Event::ShapeKernelHit);
    crate::execution_trace::kernel("shape_state_bitwise", false);
    Some(crate::value::Value::Undefined)
}

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
    (is_local_load(ops[0])
        && (ops[1].opcode, ops[1].b) == (crate::ir::Opcode::GetN, ops[0].a)
        && (ops[2].opcode, ops[2].flags, ops[2].b) == (crate::ir::Opcode::CallN, 0, ops[1].a)
        && is_local_load(ops[3])
        && (ops[4].opcode, ops[4].b) == (crate::ir::Opcode::GetN, ops[3].a)
        && (ops[5].opcode, ops[5].a) == (crate::ir::Opcode::Return, ops[4].a)
        && named(code, 1, "currentTcb")
        && named(code, 2, "markAsSuspended")
        && named(code, 4, "currentTcb"))
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
    if let Some(plan) = cached_shape_kernel_fact(function) {
        return Some(plan);
    }
    let index = (std::rc::Rc::as_ptr(function) as usize >> 4) & (SHAPE_KERNEL_FACT_SLOTS - 1);
    let plan = match_state_predicate(function)
        .map(ShapeKernelPlan::StatePredicate)
        .or_else(|| match_state_bitwise(function).map(ShapeKernelPlan::StateBitwise))
        .or_else(|| match_nested_array_length(function).map(ShapeKernelPlan::NestedArrayLength))
        .or_else(|| match_nested_array_index(function).map(ShapeKernelPlan::NestedArrayIndex))
        .or_else(|| match_property_select(function).map(ShapeKernelPlan::PropertySelect))
        .or_else(|| match_forward_zero(function).map(ShapeKernelPlan::ForwardZero))
        .or_else(|| match_forward_one(function).map(ShapeKernelPlan::ForwardOne))
        .or_else(|| match_copy_method_property(function).map(ShapeKernelPlan::CopyMethodProperty));
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

fn cached_shape_kernel_fact(function: &crate::value::FunctionValue) -> Option<ShapeKernelPlan> {
    let pointer = function as *const crate::value::FunctionValue;
    let index = (pointer as usize >> 4) & (SHAPE_KERNEL_FACT_SLOTS - 1);
    SHAPE_KERNEL_FACTS.with(|facts| {
        let facts = facts.borrow();
        let cached = facts.get(index)?.as_ref()?;
        (cached.function.as_ptr() == pointer)
            .then_some(cached.plan)
            .flatten()
    })
}

fn match_nested_array_index(
    function: &std::rc::Rc<crate::value::FunctionValue>,
) -> Option<NestedArrayIndexPlan> {
    let code = function.code.code()?;
    if code.len() != 7 || function.params != 1 {
        return None;
    }
    let [receiver, first, index, get, returned, fallback, fallback_return] =
        std::array::from_fn(|pc| code.instruction(pc).unwrap());
    use crate::ir::Opcode::*;
    (is_local_load(receiver)
        && first.opcode == GetN
        && first.b == receiver.a
        && is_local_load(index)
        && get.opcode == AGetI
        && get.b == first.a
        && get.c == index.a
        && returned.opcode == Return
        && returned.a == get.a
        && fallback.opcode == LoadConst
        && fallback_return.opcode == Return
        && fallback_return.a == fallback.a)
        .then_some(NestedArrayIndexPlan { first_pc: 1 })
}

fn match_nested_array_length(
    function: &std::rc::Rc<crate::value::FunctionValue>,
) -> Option<NestedArrayLengthPlan> {
    let code = function.code.code()?;
    if code.len() != 6 {
        return None;
    }
    let [receiver, first, second, returned, fallback, fallback_return] =
        std::array::from_fn(|pc| code.instruction(pc).unwrap());
    use crate::ir::Opcode::*;
    (is_local_load(receiver)
        && first.opcode == GetN
        && first.b == receiver.a
        && second.opcode == GetN
        && second.b == first.a
        && code.metadata_at(2).and_then(|meta| meta.name.as_deref()) == Some("length")
        && returned.opcode == Return
        && returned.a == second.a
        && fallback.opcode == LoadConst
        && fallback_return.opcode == Return
        && fallback_return.a == fallback.a)
        .then_some(NestedArrayLengthPlan {
            first_pc: 1,
            second_pc: 2,
        })
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
        || !is_local_load(load_this)
        || get_state.opcode != crate::ir::Opcode::GetN
        || !is_local_load(load_held)
        || bit_and.opcode != crate::ir::Opcode::Binary
        || bit_and.flags != crate::ir::compact_binary_id(crate::ops::BinaryOp::BitwiseAnd)
        || (bit_and.b, bit_and.c) != (get_state.a, load_held.a)
        || !matches!(
            code.constant_at(4),
            Some((_, crate::ops::Constant::Number(0.0)))
        )
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
        } if *condition == not_equal.a && fragment_returns(consequent.code(), not_equal.a) => {
            alternate.code()?
        }
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
        || !is_local_load(load_this)
        || get_again.opcode != crate::ir::Opcode::GetN
        || !is_local_load(load_suspended)
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
    matches!(
        operator,
        crate::ops::BinaryOp::BitwiseOr | crate::ops::BinaryOp::BitwiseAnd
    )
    .then_some(operator)
}

fn state_bitwise_ops_match(
    code: crate::machine::CodeView<'_>,
    ops: [crate::ir::Instruction; 10],
) -> bool {
    let [load_this, move_one, move_two, load_again, get_state, load_mask, bit_or, set_state, load_undefined, return_op] =
        ops;
    code.len() == 10
        && is_local_load(load_this)
        && (move_one.opcode, move_one.b) == (crate::ir::Opcode::Move, load_this.a)
        && (move_two.opcode, move_two.b) == (crate::ir::Opcode::Move, move_one.a)
        && is_local_load(load_again)
        && load_again.b == load_this.b
        && (get_state.opcode, get_state.b) == (crate::ir::Opcode::GetN, load_again.a)
        && is_local_load(load_mask)
        && bit_or.opcode == crate::ir::Opcode::Binary
        && compact_bitwise_operator(bit_or.flags).is_some()
        && (bit_or.b, bit_or.c) == (get_state.a, load_mask.a)
        && (set_state.opcode, set_state.a, set_state.b)
            == (crate::ir::Opcode::SetN, move_two.a, bit_or.a)
        && code.metadata_at(4).and_then(|meta| meta.name.as_deref())
            == code.metadata_at(7).and_then(|meta| meta.name.as_deref())
        && matches!(
            code.constant_at(8),
            Some((_, crate::ops::Constant::Undefined))
        )
        && load_undefined.opcode == crate::ir::Opcode::LoadConst
        && (return_op.opcode, return_op.a) == (crate::ir::Opcode::Return, load_undefined.a)
}

fn fragment_returns(code: Option<crate::machine::CodeView<'_>>, register: u16) -> bool {
    code.is_some_and(|code| {
        code.instruction(code.len().saturating_sub(1))
            .is_some_and(|op| op.opcode == crate::ir::Opcode::Return && op.a == register)
    })
}

include!("functions_shape_property_select.rs");
include!("functions_shape_forward.rs");
include!("functions_shape_copy.rs");
