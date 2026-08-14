pub(crate) fn builtin_property_writable(builtin: Builtin, key: &str) -> bool {
    if matches!(
        (builtin, key),
        (
            Builtin::GeneratorFunctionPrototype,
            "prototype" | "constructor"
        )
    ) {
        return false;
    }
    if builtin == Builtin::DatePrototype && key == "Symbol.toPrimitive" {
        return false;
    }
    if builtin == Builtin::SymbolPrototype && key == "Symbol.toPrimitive" {
        return false;
    }
    if is_well_known_symbol_property(builtin, key) {
        return false;
    }
    if builtin == Builtin::Math && crate::math::constant(key).is_some() {
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

fn builtin_property_configurable(builtin: Builtin, key: &str) -> bool {
    if builtin == Builtin::GeneratorFunctionPrototype && key == "prototype" {
        return true;
    }
    builtin != Builtin::Math || crate::math::constant(key).is_none()
}
pub(crate) fn is_well_known_symbol_property(builtin: Builtin, key: &str) -> bool {
    builtin == Builtin::Symbol
        && matches!(
            key,
            "asyncDispose"
                | "asyncIterator"
                | "dispose"
                | "hasInstance"
                | "isConcatSpreadable"
                | "iterator"
                | "match"
                | "matchAll"
                | "replace"
                | "search"
                | "species"
                | "split"
                | "toPrimitive"
                | "toStringTag"
                | "unscopables"
        )
}
