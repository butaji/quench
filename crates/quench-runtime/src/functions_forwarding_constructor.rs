struct ForwardingConstructorFact;

impl ForwardingConstructorFact {
    fn recognize(function: &crate::value::FunctionValue) -> Option<Self> {
        use crate::ir::Opcode::*;
        const SHAPE: [crate::ir::Opcode; 8] = [
            LoadLocalChecked,
            GetN,
            GetN,
            LoadLocalChecked,
            LoadLocal,
            CallN,
            LoadConst,
            Return,
        ];
        (function.params == 0 && matches!(function.kind, crate::ops::FunctionKind::Ordinary))
            .then_some(())?;
        let code = function.code.code()?;
        (code.len() == SHAPE.len()).then_some(())?;
        SHAPE
            .into_iter()
            .enumerate()
            .all(|(pc, opcode)| code.instruction(pc).is_some_and(|op| op.opcode == opcode))
            .then_some(())?;
        validate_forwarding_operands(function, code)?;
        Some(Self)
    }
}

fn validate_forwarding_operands(
    function: &crate::value::FunctionValue,
    code: crate::machine::CodeView<'_>,
) -> Option<()> {
    let arguments = function.captures.len() as u16;
    let this = arguments.checked_add(1)?;
    let [load_this, initialize, apply, call_this, call_arguments] =
        [0, 1, 2, 3, 4].map(|pc| code.instruction(pc).unwrap());
    (load_this.b == this && initialize.b == load_this.a).then_some(())?;
    (apply.b == initialize.a && call_this.b == this && call_arguments.b == arguments)
        .then_some(())?;
    (code.metadata_at(1)?.name.as_deref() == Some("initialize")
        && code.metadata_at(2)?.name.as_deref() == Some("apply"))
    .then_some(())?;
    validate_forwarding_call(code, apply.a, call_this.a, call_arguments.a)
}

fn validate_forwarding_call(
    code: crate::machine::CodeView<'_>,
    callee: u16,
    this: u16,
    arguments: u16,
) -> Option<()> {
    let call = code.instruction(5)?;
    let first_argument = call.a.checked_sub(u16::from(call.flags))?;
    (call.opcode == crate::ir::Opcode::CallN
        && call.flags == 2
        && call.b == code.instruction(1)?.a
        && call.c == callee
        && first_argument == this
        && first_argument.checked_add(1)? == arguments)
        .then_some(())
}

fn execute_forwarding_constructor(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    this_value: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Option<Result<(crate::value::Value, crate::value::Value), crate::execute::VmError>> {
    ForwardingConstructorFact::recognize(function)?;
    Some(forward_constructor(function, this_value, arguments))
}

fn forward_constructor(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    this_value: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Result<(crate::value::Value, crate::value::Value), crate::execute::VmError> {
    let initializer = crate::execute::get_property_result(this_value, "initialize")?;
    let apply = crate::execute::get_property_result(&initializer, "apply")?;
    if matches!(
        apply,
        crate::value::Value::Builtin(crate::ops::Builtin::FunctionApply)
    ) {
        crate::functions::execute_target(&initializer, this_value, arguments)?;
    } else {
        forward_custom_apply(function, &initializer, &apply, this_value, arguments)?;
    }
    crate::execution_trace::kernel("forwarding_constructor", false);
    let final_this = crate::locals::resolved_replacement(this_value.clone());
    Ok((crate::value::Value::Undefined, final_this))
}

fn forward_custom_apply(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    initializer: &crate::value::Value,
    apply: &crate::value::Value,
    this_value: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Result<(), crate::execute::VmError> {
    let environment = crate::environment::Environment::new();
    let list = arguments_object(function, arguments.to_vec(), &environment);
    crate::functions::execute_target(apply, initializer, &[this_value.clone(), list])?;
    Ok(())
}
