//! Function types - ValueFunction, NativeFunction, and NativeConstructor.

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use crate::ast::{ArrowBody, Statement};
use crate::env::Environment;
use crate::value::error::JsError;
use crate::value::object::Object;
use crate::value::kind::ObjectKind;
use crate::value::Value;

/// Type alias for function prototype storage
type ProtoCell = Rc<RefCell<Option<Rc<RefCell<Object>>>>>;

/// Type alias for native function implementation
type NativeFn = std::rc::Rc<Box<dyn Fn(Vec<Value>) -> Result<Value, JsError>>>;

// =============================================================================
// ValueFunction - JavaScript function values
// =============================================================================

/// Function value - holds function data with closure and cached prototype.
/// Uses interior mutability (RefCell) for the prototype to allow mutation
/// even when we only have an immutable reference to the function.
#[derive(Debug)]
pub struct ValueFunction {
    /// Function name (for toString and debugging)
    pub name: Option<String>,
    /// Parameter names
    pub params: Vec<String>,
    /// Function body (for regular functions)
    pub body: std::rc::Rc<Vec<Statement>>,
    /// Arrow function body (expression or block)
    pub arrow_body: std::rc::Rc<Option<ArrowBody>>,
    /// Closure environment - variables visible in this scope
    pub closure: Rc<RefCell<Environment>>,
    /// Whether this is an arrow function (doesn't bind its own 'this')
    pub is_arrow: bool,
    /// Cached prototype object
    proto_cell: ProtoCell
}

impl Clone for ValueFunction {
    fn clone(&self) -> Self {
        ValueFunction {
            name: self.name.clone(),
            params: self.params.clone(),
            body: Rc::clone(&self.body),
            arrow_body: Rc::clone(&self.arrow_body),
            closure: Rc::clone(&self.closure),
            is_arrow: self.is_arrow,
            proto_cell: self.proto_cell.clone(),
        }
    }
}

impl ValueFunction {
    /// Create a new regular function
    pub fn new(
        name: Option<String>,
        params: Vec<String>,
        body: Vec<Statement>,
        closure: Rc<RefCell<Environment>>,
    ) -> Self {
        ValueFunction {
            name,
            params,
            body: Rc::new(body),
            arrow_body: Rc::new(None),
            closure,
            is_arrow: false,
            proto_cell: Rc::new(RefCell::new(None)),
        }
    }

    /// Create a new arrow function
    #[allow(clippy::boxed_local)] // Box needed to avoid copying large Expression type
    pub fn new_arrow(
        params: Vec<String>,
        body: Box<ArrowBody>,
        closure: Rc<RefCell<Environment>>,
    ) -> Self {
        ValueFunction {
            name: None,
            params,
            body: Rc::new(Vec::new()),
            arrow_body: Rc::new(Some(*body)),
            closure,
            is_arrow: true,
            proto_cell: Rc::new(RefCell::new(None)),
        }
    }

    /// Get the prototype object for this function, creating it if needed.
    pub fn get_prototype(&self) -> Rc<RefCell<Object>> {
        let mut cell = self.proto_cell.borrow_mut();
        
        if let Some(ref proto) = *cell {
            return Rc::clone(proto);
        }
        
        let mut proto = Object::new(ObjectKind::Ordinary);
        proto.set("constructor", Value::Function(self.clone()));
        if let Some(func_proto) = crate::builtins::get_function_prototype() {
            proto.prototype = Some(func_proto);
        }
        let proto_rc = Rc::new(RefCell::new(proto));
        
        *cell = Some(Rc::clone(&proto_rc));
        proto_rc
    }

    /// Check if function has a prototype (cached)
    pub fn has_prototype(&self) -> bool {
        self.proto_cell.borrow().is_some()
    }
}

// =============================================================================
// NativeFunction - Host functions provided by the runtime
// =============================================================================

/// Native function - a host-provided function wrapped in Arc for shared ownership.
/// These are functions provided by the runtime (e.g., console.log, Math.sin).
pub struct NativeFunction(pub NativeFn);

impl NativeFunction {
    /// Create a new native function from a closure
    pub fn new<F>(f: F) -> Self
    where
        F: Fn(Vec<Value>) -> Result<Value, JsError> + 'static,
    {
        NativeFunction(std::rc::Rc::new(Box::new(f)))
    }

    /// Call the native function with arguments
    pub fn call(&self, args: Vec<Value>) -> Result<Value, JsError> {
        (self.0)(args)
    }
}

impl fmt::Debug for NativeFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NativeFunction(...)")
    }
}

impl Clone for NativeFunction {
    fn clone(&self) -> Self {
        NativeFunction(self.0.clone())
    }
}

// =============================================================================
// NativeConstructor - Host constructors (Date, Error, etc.)
// =============================================================================

/// Native constructor - a host-provided constructor function.
/// Similar to NativeFunction but has a prototype property for instanceof checks.
pub struct NativeConstructor {
    /// The constructor function wrapped in Rc for shared ownership
    func: NativeFn,
    /// The prototype object for instanceof checks
    pub prototype: std::rc::Rc<std::cell::RefCell<Object>>,
}

impl NativeConstructor {
    /// Create a new native constructor with a custom prototype
    pub fn new<F>(f: F, prototype: std::rc::Rc<std::cell::RefCell<Object>>) -> Self
    where
        F: Fn(Vec<Value>) -> Result<Value, JsError> + 'static,
    {
        NativeConstructor {
            func: std::rc::Rc::new(Box::new(f)),
            prototype,
        }
    }

    /// Call the constructor with arguments
    pub fn call(&self, args: Vec<Value>) -> Result<Value, JsError> {
        (self.func)(args)
    }

    /// Get the internal function Rc for comparison
    pub(crate) fn func_rc(&self) -> &NativeFn {
        &self.func
    }
}

impl fmt::Debug for NativeConstructor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NativeConstructor(...)")
    }
}

impl Clone for NativeConstructor {
    fn clone(&self) -> Self {
        NativeConstructor {
            func: std::rc::Rc::clone(&self.func),
            prototype: std::rc::Rc::clone(&self.prototype),
        }
    }
}
