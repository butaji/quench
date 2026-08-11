fn builtin_property_writable(builtin: Builtin, key: &str) -> bool {
    if matches!(builtin, Builtin::Math) {
        return false;
    }
    if builtin == Builtin::Number {
        return !matches!(
            key,
            "EPSILON"
                | "MAX_SAFE_INTEGER"
                | "MAX_VALUE"
                | "MIN_SAFE_INTEGER"
                | "MIN_VALUE"
                | "NaN"
                | "NEGATIVE_INFINITY"
                | "POSITIVE_INFINITY"
        );
    }
    true
}
