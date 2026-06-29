//! JavaScript runtime values - HIR (High-level IR)
//!
//! This module defines the value types used by the interpreter.
//! The key design decisions:
//! - Objects use prototype chain for inheritance
//! - Functions have interior mutability (RefCell) for prototype caching
//! - Values are immutable reference-counted handles

use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;
use std::cell::RefCell;

// =============================================================================
// Value - The core runtime type
// =============================================================================

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
    /// Functions hold their closure environment and have cached prototypes
    Function(ValueFunction),
    /// Native functions (host functions) are Arc-wrapped closures
    NativeFunction(Rc<NativeFunction>),
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
            _ => false,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", to_js_string(self))
    }
}

// =============================================================================
// Object - The object type with prototype chain
// =============================================================================

/// Object kind - distinguishes different object types for special behavior
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectKind {
    Ordinary,   // Plain object
    Array,      // Array object
    Function,   // Function object (has [[Call]])
    ArrowFunction, // Arrow function (lexical this)
    Map,        // Map collection
    Set,        // Set collection
    Date,       // Date object
    Global,     // Global object (fallback lookup)
}

/// JavaScript object with prototype chain support.
/// Uses HashMap for properties and Vec for array elements.
#[derive(Debug, Clone)]
pub struct Object {
    /// Own properties of the object
    pub properties: HashMap<String, Value>,
    /// Array elements (for dense arrays)
    pub elements: Vec<Value>,
    /// Kind of object for special behavior
    pub kind: ObjectKind,
    /// Prototype object for inheritance chain (or null for end of chain)
    pub prototype: Option<Rc<RefCell<Object>>>,
}

impl Object {
    /// Create a new ordinary object with no prototype
    pub fn new(kind: ObjectKind) -> Self {
        Object {
            properties: HashMap::new(),
            elements: Vec::new(),
            kind,
            prototype: None,
        }
    }

    /// Create a new object with a specific prototype
    pub fn with_prototype(kind: ObjectKind, prototype: Rc<RefCell<Object>>) -> Self {
        Object {
            properties: HashMap::new(),
            elements: Vec::new(),
            kind,
            prototype: Some(prototype),
        }
    }

    /// Create a new array object
    pub fn new_array(len: usize) -> Self {
        let mut obj = Object::new(ObjectKind::Array);
        obj.elements = vec![Value::Undefined; len];
        obj.properties.insert("length".to_string(), Value::Number(len as f64));
        obj
    }

    /// Get a property value, including prototype chain lookup
    pub fn get(&self, key: &str) -> Option<Value> {
        // First check own properties
        if let Some(v) = self.properties.get(key) {
            return Some(v.clone());
        }
        // Check array elements (for sparse arrays)
        if let Ok(idx) = key.parse::<usize>() {
            if idx < self.elements.len() {
                return Some(self.elements[idx].clone());
            }
        }
        // Look up prototype chain
        if let Some(ref proto) = self.prototype {
            return proto.borrow().get(key);
        }
        None
    }

    /// Set a property value on this object only (no prototype chain)
    pub fn set(&mut self, key: &str, value: Value) {
        if let Ok(idx) = key.parse::<usize>() {
            while self.elements.len() <= idx {
                self.elements.push(Value::Undefined);
            }
            self.elements[idx] = value.clone();
            self.properties.insert("length".to_string(), Value::Number(self.elements.len() as f64));
        }
        self.properties.insert(key.to_string(), value);
    }

    /// Check if property exists (own or prototype chain)
    pub fn has(&self, key: &str) -> bool {
        if self.properties.contains_key(key) {
            return true;
        }
        if key.parse::<usize>().map(|i| i < self.elements.len()).unwrap_or(false) {
            return true;
        }
        // Look up prototype chain
        if let Some(ref proto) = self.prototype {
            return proto.borrow().has(key);
        }
        false
    }

    /// Delete own property
    pub fn delete(&mut self, key: &str) -> bool {
        self.properties.remove(key).is_some()
    }
}

// =============================================================================
// Function - Function values with closures and prototypes
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
    pub body: Vec<crate::ast::Statement>,
    /// Arrow function body (expression or block)
    pub arrow_body: Option<Box<crate::ast::ArrowBody>>,
    /// Closure environment - variables visible in this scope
    pub closure: Rc<RefCell<crate::env::Environment>>,
    /// Whether this is an arrow function (doesn't bind its own 'this')
    pub is_arrow: bool,
    /// Cached prototype object (shared via Rc so clones share the same prototype)
    proto_cell: Rc<RefCell<Option<Rc<RefCell<Object>>>>>
}

impl Clone for ValueFunction {
    fn clone(&self) -> Self {
        // IMPORTANT: Share the proto_cell with the clone so that both
        // the original function and its clone refer to the same prototype.
        // This ensures that Foo and Foo.constructor (a clone) both return
        // the same prototype when get_prototype() is called.
        ValueFunction {
            name: self.name.clone(),
            params: self.params.clone(),
            body: self.body.clone(),
            arrow_body: self.arrow_body.clone(),
            closure: Rc::clone(&self.closure),
            is_arrow: self.is_arrow,
            // Share the proto_cell - both functions should point to the same
            // prototype storage so modifications to the prototype are visible
            // regardless of which function reference is used.
            proto_cell: self.proto_cell.clone(),
        }
    }
}

impl ValueFunction {
    /// Create a new regular function
    pub fn new(
        name: Option<String>,
        params: Vec<String>,
        body: Vec<crate::ast::Statement>,
        closure: Rc<RefCell<super::env::Environment>>,
    ) -> Self {
        ValueFunction {
            name,
            params,
            body,
            arrow_body: None,
            closure,
            is_arrow: false,
            proto_cell: Rc::new(RefCell::new(None)),
        }
    }

    /// Create a new arrow function
    pub fn new_arrow(
        params: Vec<String>,
        body: Box<crate::ast::ArrowBody>,
        closure: Rc<RefCell<super::env::Environment>>,
    ) -> Self {
        ValueFunction {
            name: None,
            params,
            body: Vec::new(),
            arrow_body: Some(body),
            closure,
            is_arrow: true,
            proto_cell: Rc::new(RefCell::new(None)),
        }
    }

    /// Get the prototype object for this function, creating it if needed.
    /// Uses a global cache keyed by function pointer to ensure prototypes are shared
    /// even across clones of the same function.
    pub fn get_prototype(&self) -> Rc<RefCell<Object>> {
        // Use the shared proto_cell which is cloned between function clones.
        // This ensures all clones of the same function share the same prototype.
        let mut cell = self.proto_cell.borrow_mut();
        
        if let Some(ref proto) = *cell {
            return Rc::clone(proto);
        }
        
        // Create new prototype
        let mut proto = Object::new(ObjectKind::Ordinary);
        // Set constructor property to point back to this function
        // IMPORTANT: Use self.clone() so that the constructor refers to THIS function
        proto.set("constructor", Value::Function(self.clone()));
        let proto_rc = Rc::new(RefCell::new(proto));
        
        // Store in the shared proto_cell
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
pub struct NativeFunction(
    pub std::rc::Rc<Box<dyn Fn(Vec<Value>) -> Result<Value, JsError>>>,
);

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

impl std::fmt::Debug for NativeFunction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "NativeFunction(...)")
    }
}

impl Clone for NativeFunction {
    fn clone(&self) -> Self {
        NativeFunction(self.0.clone())
    }
}

// =============================================================================
// Error handling
// =============================================================================

/// JavaScript error - wraps error messages
#[derive(Clone)]
pub struct JsError(pub String);

impl std::fmt::Debug for JsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "JsError({:?})", self.0)
    }
}

impl fmt::Display for JsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for JsError {}

impl From<&str> for JsError {
    fn from(s: &str) -> Self {
        JsError(s.to_string())
    }
}

impl From<String> for JsError {
    fn from(s: String) -> Self {
        JsError(s)
    }
}

// =============================================================================
// Value conversion utilities
// =============================================================================

/// Convert a Value to its JavaScript string representation
pub fn to_js_string(v: &Value) -> String {
    match v {
        Value::Undefined => "undefined".to_string(),
        Value::Null => "null".to_string(),
        Value::Boolean(b) => b.to_string(),
        Value::Number(n) => {
            if n.is_nan() {
                "NaN".to_string()
            } else if *n == f64::INFINITY {
                "Infinity".to_string()
            } else if *n == f64::NEG_INFINITY {
                "-Infinity".to_string()
            } else if n.fract() == 0.0 && n.abs() < 1e15 {
                format!("{:.0}", n)
            } else {
                n.to_string()
            }
        }
        Value::String(s) => s.clone(),
        Value::Object(o) => {
            let o = o.borrow();
            match o.kind {
                ObjectKind::Array => {
                    let parts: Vec<String> = o.elements.iter().map(to_js_string).collect();
                    format!("[{}]", parts.join(","))
                }
                ObjectKind::Function => "[Function]".to_string(),
                _ => "[object Object]".to_string(),
            }
        }
        Value::Function(_) => "[Function]".to_string(),
        Value::NativeFunction(_) => "[Function]".to_string(),
        Value::Symbol(s) => format!("Symbol({})", s),
    }
}

/// Convert a Value to boolean (JavaScript truthiness)
pub fn to_bool(v: &Value) -> bool {
    match v {
        Value::Undefined | Value::Null => false,
        Value::Boolean(b) => *b,
        Value::Number(n) => *n != 0.0 && !n.is_nan(),
        Value::String(s) => !s.is_empty(),
        Value::Object(_) | Value::Function(_) | Value::NativeFunction(_) => true,
        Value::Symbol(_) => false,
    }
}

/// Convert a Value to a number (JavaScript coercion)
pub fn to_number(v: &Value) -> f64 {
    match v {
        Value::Undefined => f64::NAN,
        Value::Null => 0.0,
        Value::Boolean(true) => 1.0,
        Value::Boolean(false) => 0.0,
        Value::Number(n) => *n,
        Value::String(s) => {
            let s = s.trim();
            if s.is_empty() {
                return 0.0;
            }
            if s == "Infinity" {
                return f64::INFINITY;
            }
            if s == "-Infinity" {
                return f64::NEG_INFINITY;
            }
            if s == "NaN" {
                return f64::NAN;
            }
            s.parse().unwrap_or(f64::NAN)
        }
        _ => f64::NAN,
    }
}

/// Strict equality comparison
pub fn strict_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Undefined, Value::Undefined) => true,
        (Value::Null, Value::Null) => true,
        (Value::Boolean(ai), Value::Boolean(bi)) => ai == bi,
        (Value::Number(ai), Value::Number(bi)) => ai == bi,
        (Value::String(ai), Value::String(bi)) => ai == bi,
        (Value::Object(ai), Value::Object(bi)) => Rc::ptr_eq(ai, bi),
        // Functions are compared by reference (same closure)
        (Value::Function(ai), Value::Function(bi)) => Rc::ptr_eq(&ai.closure, &bi.closure),
        // Native functions are compared by reference
        (Value::NativeFunction(ai), Value::NativeFunction(bi)) => {
            // Compare by checking if they point to the same underlying function
            // NativeFunction.0 is Rc<Box<...>>, so we compare the Rc pointers
            Rc::ptr_eq(&ai.0, &bi.0)
        }
        _ => false,
    }
}
