fn reduced_key_register(key: &ReducedKey, ops: &mut Vec<Op>, next: &mut u16) -> u16 {
    match key {
        ReducedKey::Static(key) => emit_const(ops, next, Constant::String(key.clone())),
        ReducedKey::Dynamic(key) => *key,
    }
}

fn static_key(key: &PropertyKey<'_>) -> Option<String> {
    if let PropertyKey::BigIntLiteral(value) = key {
        return crate::literal::bigint_value(value);
    }
    key.static_name().map(|name| name.into_owned())
}

fn emit_const(ops: &mut Vec<Op>, next: &mut u16, value: Constant) -> u16 {
    let dst = take_register(next);
    ops.push(Op::Const { dst, value });
    dst
}

fn take_register(next: &mut u16) -> u16 {
    let register = *next;
    *next = next.saturating_add(1);
    register
}
