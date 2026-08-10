fn name_function_declaration(ops: &mut Vec<Op>, function: u16, name: &str) {
    ops.push(Op::SetFunctionName {
        function,
        name: name.to_string(),
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
        body,
        params,
        captures,
        kind: metadata.kind,
        length: metadata.length,
        strictness: metadata.strictness,
        is_async: metadata.is_async,
        mapped_arguments: metadata.mapped_arguments,
    }
}
