//! Object kinds - distinguishes different object types for special behavior.

use std::fmt;

/// Object kind - distinguishes different object types for special behavior
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectKind {
    Ordinary,        // Plain object
    Array,           // Array object
    Function,        // Function object (has [[Call]])
    ArrowFunction,  // Arrow function (lexical this)
    Map,             // Map collection
    Set,             // Set collection
    Date,            // Date object
    Global,          // Global object (fallback lookup)
    Promise,         // Promise object
    Class,           // Class object (constructor with prototype)
}

impl fmt::Display for ObjectKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ObjectKind::Ordinary => write!(f, "ordinary"),
            ObjectKind::Array => write!(f, "array"),
            ObjectKind::Function => write!(f, "function"),
            ObjectKind::ArrowFunction => write!(f, "arrow function"),
            ObjectKind::Map => write!(f, "map"),
            ObjectKind::Set => write!(f, "set"),
            ObjectKind::Date => write!(f, "date"),
            ObjectKind::Global => write!(f, "global"),
            ObjectKind::Promise => write!(f, "promise"),
            ObjectKind::Class => write!(f, "class"),
        }
    }
}
