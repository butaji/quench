fn name_function_declaration(
    ops: &mut Vec<Op>,
    function: u16,
    next_register: &mut u16,
    name: &str,
) {
    ops.push(Op::SetFunctionName {
        function,
        name: name.to_string(),
    });
    let marker = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::Const {
        dst: marker,
        value: crate::ops::Constant::Boolean(true),
    });
    ops.push(Op::SetProperty {
        object: function,
        key: crate::functions::FUNCTION_SELF.to_string(),
        src: marker,
        strict: true,
    });
}

fn function_declaration_op(
    dst: u16,
    body: Vec<Op>,
    params: u16,
    captures: u16,
    metadata: functions::FunctionMetadata,
) -> Op {
    Op::MakeFunctionWithKind {
        dst,
        body: crate::machine::FunctionCode::pending(body).with_facts(crate::facts::FunctionFacts {
            direct_constructor: metadata.direct_constructor.clone(),
            linked_record_insert: metadata.linked_record_insert.clone(),
            forward_construct_call: metadata.forward_construct_call.clone(),
            forward_then_call: metadata.forward_then_call.clone(),
            counted_method_loop: metadata.counted_method_loop.clone(),
        }),
        params,
        captures,
        kind: metadata.kind,
        length: metadata.length,
        strictness: metadata.strictness,
        is_async: metadata.is_async,
        mapped_arguments: metadata.mapped_arguments,
    }
}
