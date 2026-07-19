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
