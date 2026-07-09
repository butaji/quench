//! JavaScript runtime values - the core runtime type.
//!
//! A JavaScript value - the fundamental runtime type.
//! All values are immutable handles; objects are Rc<RefCell<Object>> for mutation.

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use crate::arena::ObjectId;
use crate::value::function::{NativeFunction, NativeConstructor, ValueFunction};
use crate::value::object::Object;

pub use crate::value::convert::{to_js_string, to_bool, to_number, strict_eq, loose_eq, to_primitive};

/// A JavaScript value - the fundamental runtime type.
/// All values are immutable handles; objects are Rc<RefCell<Object>> for mutation.
#[derive(Debug, Clone)]
pub enum Value {
    Undefined,
    Null,
    Boolean(bool),
    Number(f64),
    String(String),
    /// Objects are reference-counted with interior mutability
    Object(Rc<RefCell<Object>>),
    /// Arena-resident object handles used by the shadow-tree path
    ObjectId(ObjectId),
    /// Functions hold their closure environment and have cached prototypes
    Function(ValueFunction),
    /// Native functions (host functions) are Arc-wrapped closures
    NativeFunction(Rc<NativeFunction>),
    /// Native constructors (Date, Error, etc.) - have a prototype property
    NativeConstructor(Rc<NativeConstructor>),
    /// Symbols for unique property keys
    Symbol(String),
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Undefined, Value::Undefined) => true,
            (Value::Null, Value::Null) => true,
            (Value::Boolean(a), Value::Boolean(b)) => a == b,
            (Value::Number(a), Value::Number(b)) => a == b,
            (Value::String(a), Value::String(b)) => a == b,
            (Value::ObjectId(a), Value::ObjectId(b)) => a == b,
            _ => false,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", to_js_string(self))
    }
}
