fn array_mutation_builtin(
    builtin: crate::ops::Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> BuiltinResult {
    if let Some(result) = typed_array_static(builtin, receiver, arguments) {
        return Some(result);
    }
    use crate::ops::Builtin::*;
    match builtin {
        ArrayShift => Some(Ok(crate::builtins::array_shift(receiver))),
        ArrayReverse => Some(Ok(crate::builtins::array_reverse(receiver))),
        ArrayPop => Some(Ok(crate::builtins::array_pop(receiver))),
        ArrayUnshift => Some(Ok(crate::builtins::array_unshift(receiver, arguments))),
        ArrayFill => Some(Ok(crate::builtins::array_fill(receiver, arguments))),
        ArrayCopyWithin => Some(Ok(crate::builtins::array_copy_within(receiver, arguments))),
        ArrayFindLast => Some(crate::builtins::array_find_last(receiver, arguments)),
        ArrayFindLastIndex => Some(crate::builtins::array_find_last_index(receiver, arguments)),
        ArrayToSorted => Some(Ok(crate::builtins::array_to_sorted(receiver, arguments))),
        _ => None,
    }
}
