//! Object kinds - distinguishes different object types for special behavior.

use std::fmt;

/// Object kind - distinguishes different object types for special behavior
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectKind {
    Ordinary,      // Plain object
    Array,         // Array object
    Function,      // Function object (has [[Call]])
    ArrowFunction, // Arrow function (lexical this)
    Map,           // Map collection
    Set,           // Set collection
    WeakMap,       // WeakMap collection
    WeakSet,       // WeakSet collection
    Date,          // Date object
    Global,        // Global object (fallback lookup)
    Promise,       // Promise object
    Class,         // Class object (constructor with prototype)
    RegExp,        // RegExp object
}

impl fmt::Display for ObjectKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", object_kind_name(self))
    }
}

fn object_kind_name(kind: &ObjectKind) -> &'static str {
    // Split into groups to reduce single-match complexity
    match kind {
        ObjectKind::Ordinary | ObjectKind::Array | ObjectKind::Function => simple_kind_name(kind),
        ObjectKind::ArrowFunction | ObjectKind::Map | ObjectKind::Set => medium_kind_name(kind),
        ObjectKind::WeakMap | ObjectKind::WeakSet => weak_kind_name(kind),
        ObjectKind::Date
        | ObjectKind::Global
        | ObjectKind::Promise
        | ObjectKind::Class
        | ObjectKind::RegExp => complex_kind_name(kind),
    }
}

fn simple_kind_name(kind: &ObjectKind) -> &'static str {
    match kind {
        ObjectKind::Ordinary => "ordinary",
        ObjectKind::Array => "array",
        ObjectKind::Function => "function",
        _ => unreachable!(),
    }
}

fn medium_kind_name(kind: &ObjectKind) -> &'static str {
    match kind {
        ObjectKind::ArrowFunction => "arrow function",
        ObjectKind::Map => "map",
        ObjectKind::Set => "set",
        _ => unreachable!(),
    }
}

fn weak_kind_name(kind: &ObjectKind) -> &'static str {
    match kind {
        ObjectKind::WeakMap => "weakmap",
        ObjectKind::WeakSet => "weakset",
        _ => unreachable!(),
    }
}

fn complex_kind_name(kind: &ObjectKind) -> &'static str {
    match kind {
        ObjectKind::Date => "date",
        ObjectKind::Global => "global",
        ObjectKind::Promise => "promise",
        ObjectKind::Class => "class",
        ObjectKind::RegExp => "regexp",
        _ => unreachable!("complex_kind_name called with {:?}", kind),
    }
}

/// Exotic kinds for boxed primitives (string, number, boolean objects)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExoticKind {
    String,
    Number,
    Boolean,
    BigInt,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── ObjectKind ──────────────────────────────────────────────────────

    #[test]
    fn object_kind_display_ordinary() {
        assert_eq!(ObjectKind::Ordinary.to_string(), "ordinary");
    }

    #[test]
    fn object_kind_display_array() {
        assert_eq!(ObjectKind::Array.to_string(), "array");
    }

    #[test]
    fn object_kind_display_function() {
        assert_eq!(ObjectKind::Function.to_string(), "function");
    }

    #[test]
    fn object_kind_display_arrow_function() {
        assert_eq!(ObjectKind::ArrowFunction.to_string(), "arrow function");
    }

    #[test]
    fn object_kind_display_map() {
        assert_eq!(ObjectKind::Map.to_string(), "map");
    }

    #[test]
    fn object_kind_display_set() {
        assert_eq!(ObjectKind::Set.to_string(), "set");
    }

    #[test]
    fn object_kind_display_weakmap() {
        assert_eq!(ObjectKind::WeakMap.to_string(), "weakmap");
    }

    #[test]
    fn object_kind_display_weakset() {
        assert_eq!(ObjectKind::WeakSet.to_string(), "weakset");
    }

    #[test]
    fn object_kind_display_date() {
        assert_eq!(ObjectKind::Date.to_string(), "date");
    }

    #[test]
    fn object_kind_display_global() {
        assert_eq!(ObjectKind::Global.to_string(), "global");
    }

    #[test]
    fn object_kind_display_promise() {
        assert_eq!(ObjectKind::Promise.to_string(), "promise");
    }

    #[test]
    fn object_kind_display_class() {
        assert_eq!(ObjectKind::Class.to_string(), "class");
    }

    #[test]
    fn object_kind_display_regexp() {
        assert_eq!(ObjectKind::RegExp.to_string(), "regexp");
    }

    #[test]
    fn object_kind_debug() {
        assert_eq!(format!("{:?}", ObjectKind::Ordinary), "Ordinary");
        assert_eq!(format!("{:?}", ObjectKind::Array), "Array");
        assert_eq!(format!("{:?}", ObjectKind::Function), "Function");
        assert_eq!(format!("{:?}", ObjectKind::ArrowFunction), "ArrowFunction");
        assert_eq!(format!("{:?}", ObjectKind::Map), "Map");
        assert_eq!(format!("{:?}", ObjectKind::Set), "Set");
        assert_eq!(format!("{:?}", ObjectKind::WeakMap), "WeakMap");
        assert_eq!(format!("{:?}", ObjectKind::WeakSet), "WeakSet");
        assert_eq!(format!("{:?}", ObjectKind::Date), "Date");
        assert_eq!(format!("{:?}", ObjectKind::Global), "Global");
        assert_eq!(format!("{:?}", ObjectKind::Promise), "Promise");
        assert_eq!(format!("{:?}", ObjectKind::Class), "Class");
        assert_eq!(format!("{:?}", ObjectKind::RegExp), "RegExp");
    }

    #[test]
    fn object_kind_clone() {
        let kinds = &[
            ObjectKind::Ordinary,
            ObjectKind::Array,
            ObjectKind::Function,
            ObjectKind::ArrowFunction,
            ObjectKind::Map,
            ObjectKind::Set,
            ObjectKind::WeakMap,
            ObjectKind::WeakSet,
            ObjectKind::Date,
            ObjectKind::Global,
            ObjectKind::Promise,
            ObjectKind::Class,
            ObjectKind::RegExp,
        ];
        for kind in kinds {
            assert_eq!(kind.clone(), *kind);
        }
    }

    #[test]
    fn object_kind_partial_eq_same() {
        assert_eq!(ObjectKind::Ordinary, ObjectKind::Ordinary);
        assert_eq!(ObjectKind::Array, ObjectKind::Array);
        assert_eq!(ObjectKind::Function, ObjectKind::Function);
        assert_eq!(ObjectKind::ArrowFunction, ObjectKind::ArrowFunction);
        assert_eq!(ObjectKind::Map, ObjectKind::Map);
        assert_eq!(ObjectKind::Set, ObjectKind::Set);
        assert_eq!(ObjectKind::WeakMap, ObjectKind::WeakMap);
        assert_eq!(ObjectKind::WeakSet, ObjectKind::WeakSet);
        assert_eq!(ObjectKind::Date, ObjectKind::Date);
        assert_eq!(ObjectKind::Global, ObjectKind::Global);
        assert_eq!(ObjectKind::Promise, ObjectKind::Promise);
        assert_eq!(ObjectKind::Class, ObjectKind::Class);
        assert_eq!(ObjectKind::RegExp, ObjectKind::RegExp);
    }

    #[test]
    fn object_kind_partial_eq_different() {
        assert_ne!(ObjectKind::Ordinary, ObjectKind::Array);
        assert_ne!(ObjectKind::Function, ObjectKind::ArrowFunction);
        assert_ne!(ObjectKind::Map, ObjectKind::Set);
        assert_ne!(ObjectKind::WeakMap, ObjectKind::WeakSet);
        assert_ne!(ObjectKind::Date, ObjectKind::Global);
        assert_ne!(ObjectKind::Promise, ObjectKind::Class);
        assert_ne!(ObjectKind::RegExp, ObjectKind::Ordinary);
        assert_ne!(ObjectKind::Array, ObjectKind::Function);
    }

    // ── ExoticKind ─────────────────────────────────────────────────────

    #[test]
    fn exotic_kind_debug() {
        assert_eq!(format!("{:?}", ExoticKind::String), "String");
        assert_eq!(format!("{:?}", ExoticKind::Number), "Number");
        assert_eq!(format!("{:?}", ExoticKind::Boolean), "Boolean");
        assert_eq!(format!("{:?}", ExoticKind::BigInt), "BigInt");
    }

    #[test]
    fn exotic_kind_clone() {
        let kinds = &[
            ExoticKind::String,
            ExoticKind::Number,
            ExoticKind::Boolean,
            ExoticKind::BigInt,
        ];
        for kind in kinds {
            assert_eq!(kind.clone(), *kind);
        }
    }

    #[test]
    fn exotic_kind_partial_eq_same() {
        assert_eq!(ExoticKind::String, ExoticKind::String);
        assert_eq!(ExoticKind::Number, ExoticKind::Number);
        assert_eq!(ExoticKind::Boolean, ExoticKind::Boolean);
        assert_eq!(ExoticKind::BigInt, ExoticKind::BigInt);
    }

    #[test]
    fn exotic_kind_partial_eq_different() {
        assert_ne!(ExoticKind::String, ExoticKind::Number);
        assert_ne!(ExoticKind::Boolean, ExoticKind::BigInt);
        assert_ne!(ExoticKind::String, ExoticKind::BigInt);
    }
}
