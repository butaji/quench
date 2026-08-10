fn tail_calls_enabled(
    strictness: crate::ops::FunctionStrictness,
    kind: crate::ops::FunctionKind,
    is_async: bool,
) -> bool {
    matches!(strictness, crate::ops::FunctionStrictness::Strict)
        && matches!(kind, crate::ops::FunctionKind::Ordinary | crate::ops::FunctionKind::Arrow)
        && !is_async
}
