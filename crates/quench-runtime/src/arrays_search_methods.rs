fn array_search_method(key: &str) -> Option<crate::ops::Builtin> {
    match key {
        "findLast" => Some(crate::ops::Builtin::ArrayFindLast),
        "findLastIndex" => Some(crate::ops::Builtin::ArrayFindLastIndex),
        _ => None,
    }
}
