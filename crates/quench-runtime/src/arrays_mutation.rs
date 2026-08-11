fn array_mutation_builtin(
    builtin: crate::ops::Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> BuiltinResult {
    use crate::ops::Builtin::*;
    match builtin {
        ArrayShift => Some(Ok(crate::builtins::array_shift(receiver))),
        ArrayReverse => Some(Ok(crate::builtins::array_reverse(receiver))),
        ArrayPop => Some(Ok(crate::builtins::array_pop(receiver))),
        ArrayUnshift => Some(Ok(crate::builtins::array_unshift(receiver, arguments))),
        ArrayFill => Some(Ok(crate::builtins::array_fill(receiver, arguments))),
        ArrayCopyWithin => Some(Ok(crate::builtins::array_copy_within(receiver, arguments))),
        _ => None,
    }
}
