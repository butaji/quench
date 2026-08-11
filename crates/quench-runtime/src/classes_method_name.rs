fn set_method_name(
    ops: &mut Vec<Op>,
    method: &MethodDefinition<'_>,
    function: u16,
    key: u16,
) -> Option<()> {
    let prefix = accessor_prefix(method.kind);
    if method.computed {
        ops.push(Op::SetFunctionNameDynamic {
            function,
            key,
            prefix: prefix.map(str::to_string),
        });
        return Some(());
    }
    let mut name = method_key(&method.key)?;
    if let Some(prefix) = prefix {
        name = format!("{prefix} {name}");
    }
    ops.push(Op::SetFunctionName { function, name });
    Some(())
}

fn accessor_prefix(kind: MethodDefinitionKind) -> Option<&'static str> {
    match kind {
        MethodDefinitionKind::Get => Some("get"),
        MethodDefinitionKind::Set => Some("set"),
        _ => None,
    }
}

fn method_key(key: &PropertyKey<'_>) -> Option<String> {
    match key {
        PropertyKey::StaticIdentifier(key) => Some(key.name.to_string()),
        PropertyKey::PrivateIdentifier(key) => Some(format!("#{}", key.name)),
        PropertyKey::StringLiteral(key) => Some(key.value.to_string()),
        PropertyKey::NumericLiteral(key) => Some(crate::conversion::number_to_string(key.value)),
        PropertyKey::BigIntLiteral(key) => crate::literal::bigint_value(key),
        _ => None,
    }
}
