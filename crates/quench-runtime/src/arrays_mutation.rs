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
        ArrayPush => Some(crate::builtins::array_push(receiver, arguments)),
        ArrayShift => Some(crate::builtins::array_shift(receiver)),
        ArrayReverse => Some(crate::builtins::array_reverse(receiver)),
        TypedArrayReverse => Some(typed_array_reverse(receiver)),
        ArrayPop => Some(crate::builtins::array_pop(receiver)),
        ArrayUnshift => Some(crate::builtins::array_unshift(receiver, arguments)),
        ArrayFill => Some(crate::builtins::array_fill(receiver, arguments)),
        ArrayCopyWithin => Some(crate::builtins::array_copy_within(receiver, arguments)),
        ArrayFindLast => Some(crate::builtins::array_find_last(receiver, arguments)),
        ArrayFindLastIndex => Some(crate::builtins::array_find_last_index(receiver, arguments)),
        TypedArrayFindLast => Some(typed_array_find_last(receiver, arguments)),
        TypedArrayFindLastIndex => Some(typed_array_find_last_index(receiver, arguments)),
        ArrayToSorted => Some(crate::builtins::array_to_sorted(receiver, arguments)),
        TypedArrayToSorted => Some(typed_array_to_sorted(receiver, arguments)),
        TypedArraySort => Some(typed_array_sort(receiver, arguments)),
        TypedArrayWith => Some(typed_array_with(receiver, arguments)),
        ArrayToSpliced => Some(crate::builtins::array_to_spliced(receiver, arguments)),
        ArrayWith => Some(crate::builtins::array_with(receiver, arguments)),
        _ => None,
    }
}
