fn property_kind(kind: MethodDefinitionKind) -> PropertyDefinitionKind {
    match kind {
        MethodDefinitionKind::Get => PropertyDefinitionKind::Get,
        MethodDefinitionKind::Set => PropertyDefinitionKind::Set,
        _ => PropertyDefinitionKind::Data,
    }
}

fn emit_default_constructor(ops: &mut Vec<Op>, next: &mut u16) -> u16 {
    let dst = take_register(next);
    ops.push(Op::MakeFunctionWithKind {
        dst,
        body: crate::machine::FunctionCode::from_ops(vec![
            Op::Const { dst: 0, value: Constant::Undefined },
            Op::Return { src: 0 },
        ]),
        params: 0,
        captures: 0,
        kind: FunctionKind::Ordinary,
        length: 0,
        strictness: FunctionStrictness::Strict,
        is_async: false,
        mapped_arguments: false,
    });
    dst
}

fn emit_object(ops: &mut Vec<Op>, next: &mut u16, properties: Vec<(String, u16)>) -> u16 {
    let dst = take_register(next);
    ops.push(Op::MakeObject { dst, properties });
    dst
}

fn emit_string(ops: &mut Vec<Op>, next: &mut u16, value: String) -> u16 {
    let dst = take_register(next);
    ops.push(Op::Const { dst, value: Constant::String(value) });
    dst
}

fn define_static_key(
    ops: &mut Vec<Op>, next: &mut u16, object: u16, key: &str, value: u16,
    kind: PropertyDefinitionKind,
) {
    let key = emit_string(ops, next, key.to_string());
    ops.push(Op::DefineProperty { object, key, value, kind, enumerable: false });
}

fn take_register(next: &mut u16) -> u16 {
    let register = *next;
    *next = next.saturating_add(1);
    register
}
