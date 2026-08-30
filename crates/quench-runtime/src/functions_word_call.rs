const WORD_CALL_FACT_SLOTS: usize = 256;

#[derive(Clone, Copy)]
enum WordCallPlan {
    AddConstant(f64),
}

#[derive(Clone)]
struct WordCallFact {
    function: std::rc::Weak<crate::value::FunctionValue>,
    plan: Option<WordCallPlan>,
}

thread_local! {
    static WORD_CALL_FACTS: std::cell::RefCell<Vec<Option<WordCallFact>>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

pub(crate) fn word_add_constant(function: &crate::value::FunctionValue) -> Option<f64> {
    let WordCallPlan::AddConstant(value) = word_call_fact(function)?;
    Some(value)
}

pub(crate) fn execute_cached_word_call(
    caller: &mut crate::register_file::RegisterFile,
    destination: u16,
    argument: u16,
    function: &crate::value::FunctionValue,
) -> Option<()> {
    let constant = word_add_constant(function)?;
    let value = caller.read_number(usize::from(argument))?;
    crate::execution_trace::event(crate::execution_trace::Event::LeafAttempt);
    crate::execution_trace::event(crate::execution_trace::Event::LeafHit);
    crate::execution_trace::kernel("word_call_add_constant", false);
    crate::execution_trace::function_call_shape(
        function.params,
        function.code.capture_slots().len(),
        function.code.code(),
    );
    caller.write_number(usize::from(destination), value + constant);
    Some(())
}

fn word_call_fact(function: &crate::value::FunctionValue) -> Option<WordCallPlan> {
    let pointer = function as *const crate::value::FunctionValue;
    let index = (pointer as usize >> 4) & (WORD_CALL_FACT_SLOTS - 1);
    if let Some(plan) = cached_word_call(index, pointer) {
        return plan;
    }
    crate::execution_trace::function_call_shape(
        function.params,
        function.code.capture_slots().len(),
        function.code.code(),
    );
    let plan = recognize_word_call(function);
    let fact = WordCallFact {
        function: weak_from_borrowed(function),
        plan,
    };
    WORD_CALL_FACTS.with(|facts| {
        let mut facts = facts.borrow_mut();
        if facts.is_empty() {
            facts.resize_with(WORD_CALL_FACT_SLOTS, || None);
        }
        facts[index] = Some(fact);
    });
    plan
}

fn cached_word_call(
    index: usize,
    pointer: *const crate::value::FunctionValue,
) -> Option<Option<WordCallPlan>> {
    WORD_CALL_FACTS.with(|facts| {
        let facts = facts.borrow();
        let fact = facts.get(index)?.as_ref()?;
        (fact.function.as_ptr() == pointer).then_some(fact.plan)
    })
}

fn recognize_word_call(function: &crate::value::FunctionValue) -> Option<WordCallPlan> {
    (function.params == 1
        && !function.is_async
        && matches!(function.kind, crate::ops::FunctionKind::Ordinary))
    .then_some(())?;
    let code = function.code.code()?;
    matches!(code.len(), 4 | 6).then_some(())?;
    let [parameter, constant, add, returned] =
        std::array::from_fn(|pc| code.instruction(pc).unwrap());
    let parameter_slot = u16::try_from(function.captures.len()).ok()?;
    let (_, crate::ops::Constant::Number(value)) = code.constant_at(1)? else {
        return None;
    };
    (matches!(
        parameter.opcode,
        crate::ir::Opcode::LoadLocal | crate::ir::Opcode::LoadLocalChecked
    )
        && parameter.b == parameter_slot
        && constant.opcode == crate::ir::Opcode::LoadConst
        && word_add_operator(add)
        && (add.b, add.c) == (parameter.a, constant.a)
        && returned.opcode == crate::ir::Opcode::Return
        && returned.a == add.a
        && trailing_undefined_return(code))
    .then_some(WordCallPlan::AddConstant(*value))
}

fn trailing_undefined_return(code: crate::machine::CodeView<'_>) -> bool {
    if code.len() == 4 {
        return true;
    }
    matches!(
        code.constant_at(4),
        Some((_, crate::ops::Constant::Undefined))
    ) && code
        .instruction(5)
        .is_some_and(|op| op.opcode == crate::ir::Opcode::Return)
}

fn word_add_operator(instruction: crate::ir::Instruction) -> bool {
    instruction.opcode == crate::ir::Opcode::Add
        || (instruction.opcode == crate::ir::Opcode::Binary
            && crate::ir::compact_binary_operator(instruction.flags)
                == Some(crate::ops::BinaryOp::Add))
}

fn weak_from_borrowed(
    function: &crate::value::FunctionValue,
) -> std::rc::Weak<crate::value::FunctionValue> {
    let pointer = function as *const crate::value::FunctionValue;
    // SAFETY: the caller's function word owns a strong reference throughout
    // this call. The temporary increment is balanced when `owner` drops.
    unsafe {
        std::rc::Rc::increment_strong_count(pointer);
        let owner = std::rc::Rc::from_raw(pointer);
        std::rc::Rc::downgrade(&owner)
    }
}
