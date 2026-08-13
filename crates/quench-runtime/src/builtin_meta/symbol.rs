//! Symbol method metadata.

use crate::ops::Builtin;

/// Returns the builtin for a Symbol prototype property key.
pub fn symbol_prop(key: &str) -> Option<Builtin> {
    use Builtin::*;
    match key {
        "iterator" => Some(SymbolIterator),
        "asyncIterator" => Some(SymbolAsyncIterator),
        "dispose" => Some(SymbolDispose),
        "asyncDispose" => Some(SymbolAsyncDispose),
        "unscopables" => Some(SymbolUnscopables),
        "toStringTag" => Some(SymbolToStringTag),
        "toPrimitive" => Some(SymbolToPrimitive),
        "hasInstance" => Some(SymbolHasInstance),
        "isConcatSpreadable" => Some(SymbolIsConcatSpreadable),
        "species" => Some(SymbolSpecies),
        "match" => Some(SymbolMatch),
        "replace" => Some(SymbolReplace),
        "search" => Some(SymbolSearch),
        "split" => Some(SymbolSplit),
        "matchAll" => Some(SymbolMatchAll),
        "for" => Some(SymbolFor),
        "keyFor" => Some(SymbolKeyFor),
        _ => None,
    }
}

pub const fn fn_name(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::SymbolToString => Some("Symbol.prototype.toString"),
        Builtin::SymbolValueOf => Some("Symbol.prototype.valueOf"),
        Builtin::SymbolPrototypeToPrimitive => Some("[Symbol.toPrimitive]"),
        Builtin::SymbolDescriptionGetter => Some("get description"),
        Builtin::SymbolFor => Some("Symbol.for"),
        Builtin::SymbolKeyFor => Some("Symbol.keyFor"),
        _ => None,
    }
}

pub const fn fn_len(b: Builtin) -> Option<f64> {
    match b {
        Builtin::SymbolToString | Builtin::SymbolValueOf | Builtin::SymbolDescriptionGetter => {
            Some(0.0)
        }
        Builtin::SymbolPrototypeToPrimitive => Some(1.0),
        Builtin::SymbolFor | Builtin::SymbolKeyFor => Some(1.0),
        _ => None,
    }
}

pub const fn short_name(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::SymbolToString => Some("toString"),
        Builtin::SymbolValueOf => Some("valueOf"),
        Builtin::SymbolPrototypeToPrimitive => Some("[Symbol.toPrimitive]"),
        Builtin::SymbolDescriptionGetter => Some("get description"),
        Builtin::SymbolFor => Some("for"),
        Builtin::SymbolKeyFor => Some("keyFor"),
        _ => None,
    }
}
