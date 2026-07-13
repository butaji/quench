//! Function types - ValueFunction, NativeFunction, and NativeConstructor.

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use crate::ast::{ArrowBody, Param, Statement};
use crate::env::Environment;
use crate::value::error::JsError;
use crate::value::kind::ObjectKind;
use crate::value::object::Object;
use crate::value::Value;

/// Type alias for function prototype storage
type ProtoCell = Rc<RefCell<Option<Rc<RefCell<Object>>>>>;

/// Reference to a function's cached prototype cell.
///
/// Normal clones share the cell strongly. The clone stored as the
/// prototype object's `constructor` property holds it weakly, breaking the
/// Rc cycle `function -> proto_cell -> prototype object -> constructor ->
/// proto_cell` that would otherwise leak every function prototype forever.
///
/// Known limitation: the closure cycle `function -> closure env -> function`
/// (a function whose environment binds the function itself) is still a
/// strong Rc cycle and leaks; breaking it requires a real GC.
#[derive(Clone)]
enum ProtoCellRef {
    Strong(ProtoCell),
    Weak(std::rc::Weak<RefCell<Option<Rc<RefCell<Object>>>>>),
}

impl ProtoCellRef {
    /// Get a strong reference to the cell, if it is still alive.
    fn upgrade(&self) -> Option<ProtoCell> {
        match self {
            ProtoCellRef::Strong(rc) => Some(Rc::clone(rc)),
            ProtoCellRef::Weak(w) => w.upgrade(),
        }
    }

    /// Address of the cell allocation, usable as a function identity key.
    /// A live Weak keeps the RcBox allocation reserved, so the address
    /// cannot be reused while a weak reference to it exists.
    fn as_ptr(&self) -> *const RefCell<Option<Rc<RefCell<Object>>>> {
        match self {
            ProtoCellRef::Strong(rc) => Rc::as_ptr(rc),
            ProtoCellRef::Weak(w) => w.as_ptr(),
        }
    }
}

/// Type alias for native function implementation
type NativeFn = std::rc::Rc<Box<dyn Fn(Vec<Value>) -> Result<Value, JsError>>>;

// =============================================================================
// ValueFunction - JavaScript function values
// =============================================================================

/// Function value - holds function data with closure and cached prototype.
/// Uses interior mutability (RefCell) for the prototype to allow mutation
/// even when we only have an immutable reference to the function.
pub struct ValueFunction {
    /// Function name (for toString and debugging)
    pub name: Option<String>,
    /// Parameter list with optional defaults
    pub params: Vec<Param>,
    /// Function body (for regular functions)
    pub body: std::rc::Rc<Vec<Statement>>,
    /// Arrow function body (expression or block)
    pub arrow_body: std::rc::Rc<Option<ArrowBody>>,
    /// Closure environment - variables visible in this scope
    pub closure: Rc<RefCell<Environment>>,
    /// Whether this is an arrow function (doesn't bind its own 'this')
    pub is_arrow: bool,
    /// Strictness captured where the function was DEFINED (per spec),
    /// never inherited from the call site.
    pub strict: bool,
    /// Cached prototype object
    proto_cell: ProtoCellRef,
    /// Additional properties (e.g., sameValue, notSameValue on assert)
    /// Wrapped in Rc<RefCell> so clones share mutations (see Clone impl).
    properties: std::rc::Rc<std::cell::RefCell<std::collections::HashMap<String, Value>>>,
}

impl Clone for ValueFunction {
    fn clone(&self) -> Self {
        // Share the same Rc<RefCell<HashMap>> with the original so deletes /
        // mutations are visible to subsequent accesses. Without this, every
        // env lookup returned a clone with a fresh HashMap and patterns like
        // `delete f.length; assert(!hasOwnProperty(f, "length"))` would
        // never see the deletion (the original kept its entry, the clone
        // was modified independently).
        ValueFunction {
            name: self.name.clone(),
            params: self.params.clone(),
            body: Rc::clone(&self.body),
            arrow_body: Rc::clone(&self.arrow_body),
            closure: Rc::clone(&self.closure),
            is_arrow: self.is_arrow,
            strict: self.strict,
            proto_cell: self.proto_cell.clone(),
            properties: std::rc::Rc::clone(&self.properties),
        }
    }
}

impl fmt::Debug for ValueFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ValueFunction({:?})", self.name)
    }
}

/// Per ES §14.1 ExpectedArgumentCount: count parameters until (and
/// including) the first one with a default value, then stop. Returns
/// the count as f64 for use in property descriptors.
pub(crate) fn expected_argument_count(params: &[Param]) -> f64 {
    let mut count = 0;
    for p in params {
        if p.default.is_some() {
            break;
        }
        count += 1;
    }
    count as f64
}

impl ValueFunction {
    /// Create a new regular function
    pub fn new(
        name: Option<String>,
        params: Vec<Param>,
        body: Vec<Statement>,
        closure: Rc<RefCell<Environment>>,
    ) -> Self {
        // Per ES §9.2.4 FunctionInitialize: install `length` and (when named)
        // `name` as own properties so Object.getOwnPropertyDescriptor sees them.
        // `length` counts parameters **before the first default**, not all
        // params without defaults (see ExpectedArgumentCount in §14.1).
        let length = expected_argument_count(&params);
        let mut props = std::collections::HashMap::new();
        props.insert("length".to_string(), Value::Number(length));
        if let Some(ref n) = name {
            props.insert("name".to_string(), Value::String(n.clone()));
        }
        ValueFunction {
            name,
            params,
            body: Rc::new(body),
            arrow_body: Rc::new(None),
            closure,
            is_arrow: false,
            strict: false,
            proto_cell: ProtoCellRef::Strong(Rc::new(RefCell::new(None))),
            properties: std::rc::Rc::new(std::cell::RefCell::new(props)),
        }
    }

    /// Create a new arrow function
    #[allow(clippy::boxed_local)] // Box needed to avoid copying large Expression type
    pub fn new_arrow(
        params: Vec<Param>,
        body: Box<ArrowBody>,
        closure: Rc<RefCell<Environment>>,
    ) -> Self {
        // Per ES §9.2.4 FunctionInitialize: arrow functions are also functions
        // and must have a configurable `length` and (when named) `name`.
        let length = expected_argument_count(&params);
        let mut props = std::collections::HashMap::new();
        props.insert("length".to_string(), Value::Number(length));
        ValueFunction {
            name: None,
            params,
            body: Rc::new(Vec::new()),
            arrow_body: Rc::new(Some(*body)),
            closure,
            is_arrow: true,
            strict: false,
            proto_cell: ProtoCellRef::Strong(Rc::new(RefCell::new(None))),
            properties: std::rc::Rc::new(std::cell::RefCell::new(props)),
        }
    }

    /// Get the prototype object for this function, creating it if needed.
    pub fn get_prototype(&self) -> Rc<RefCell<Object>> {
        // First check if prototype was explicitly set via FooObj.prototype = ...
        if let Some(Value::Object(proto)) = self.properties.borrow().get("prototype") {
            return Rc::clone(proto);
        }
        if let Some(cell) = self.proto_cell.upgrade() {
            let mut cell_ref = cell.borrow_mut();
            if let Some(ref proto) = *cell_ref {
                return Rc::clone(proto);
            }
            let proto_rc = Rc::new(RefCell::new(self.new_prototype_object()));
            *cell_ref = Some(Rc::clone(&proto_rc));
            return proto_rc;
        }
        // Weak back-edge expired: the original function was dropped while its
        // prototype object outlived it. Build an uncached prototype.
        Rc::new(RefCell::new(self.new_prototype_object()))
    }

    /// Build the prototype object for this function.
    fn new_prototype_object(&self) -> Object {
        let mut proto = Object::new(ObjectKind::Ordinary);
        proto.set("constructor", self.constructor_value());
        if let Some(func_proto) = crate::builtins::get_function_prototype() {
            proto.prototype = Some(func_proto);
        }
        proto
    }

    /// `constructor` property value for the prototype object.
    /// Holds the proto cell weakly so the prototype does not keep the
    /// function (and its own proto cell) alive forever.
    fn constructor_value(&self) -> Value {
        let mut ctor = self.clone();
        if let Some(cell) = self.proto_cell.upgrade() {
            ctor.proto_cell = ProtoCellRef::Weak(Rc::downgrade(&cell));
        }
        Value::Function(ctor)
    }

    /// Check if function has a prototype (cached)
    pub fn has_prototype(&self) -> bool {
        self.proto_cell
            .upgrade()
            .is_some_and(|cell| cell.borrow().is_some())
    }

    /// Identity key for strict equality: distinct function declarations get
    /// distinct proto cells at construction, and clones share the same cell.
    pub(crate) fn identity_ptr(&self) -> *const RefCell<Option<Rc<RefCell<Object>>>> {
        self.proto_cell.as_ptr()
    }

    /// Compute the function's length per ECMA-262 14.1 / 9.2.4
    /// (ExpectedArgumentCount). Length is the number of parameters
    /// **before the first parameter with a default value** (or rest).
    pub fn length(&self) -> usize {
        expected_argument_count(&self.params) as usize
    }

    /// Get a property from this function (e.g., sameValue, notSameValue)
    pub fn get_property(&self, key: &str) -> Option<Value> {
        self.properties.borrow().get(key).cloned()
    }

    /// Set a property on this function (e.g., prototype).
    /// Uses with_mut to work around nested borrow issues.
    pub fn set_property(&self, key: &str, value: Value) {
        self.with_mut(|props| {
            props.insert(key.to_string(), value);
        });
    }

    /// Remove a property. Returns true if it was present (and was removable).
    /// Per ES spec, ordinary functions allow configurable length/name removal.
    pub fn remove_property(&self, key: &str) -> bool {
        let mut map = self.properties.borrow_mut();
        map.remove(key).is_some()
    }

    /// Access properties with mutable borrow, works around outer borrow issues.
    fn with_mut<F>(&self, f: F)
    where
        F: FnOnce(&mut std::collections::HashMap<String, Value>),
    {
        let mut map = self.properties.borrow_mut();
        f(&mut map);
    }
}

// =============================================================================
// NativeFunction - Host functions provided by the runtime
// =============================================================================

/// Native function - a host-provided function wrapped in Arc for shared ownership.
/// These are functions provided by the runtime (e.g., console.log, Math.sin).
pub struct NativeFunction {
    pub func: NativeFn,
    /// Optional prototype object (for built-in constructors like Number).
    /// Wrapped in RefCell so set_property can lazily install it without
    /// requiring a mutable receiver.
    pub prototype: std::rc::Rc<std::cell::RefCell<Option<Rc<RefCell<Object>>>>>,
    /// Additional properties (for JS compatibility) - shared via Rc so clones share properties
    properties: std::rc::Rc<std::cell::RefCell<std::collections::HashMap<String, Value>>>,
}

impl NativeFunction {
    /// Create a new native function from a closure
    pub fn new<F>(f: F) -> Self
    where
        F: Fn(Vec<Value>) -> Result<Value, JsError> + 'static,
    {
        NativeFunction {
            func: std::rc::Rc::new(Box::new(f)),
            prototype: std::rc::Rc::new(std::cell::RefCell::new(None)),
            properties: std::rc::Rc::new(std::cell::RefCell::new(std::collections::HashMap::new())),
        }
    }

    /// Create a new native function with a prototype
    pub fn new_with_prototype<F>(f: F, prototype: Rc<RefCell<Object>>) -> Self
    where
        F: Fn(Vec<Value>) -> Result<Value, JsError> + 'static,
    {
        NativeFunction {
            func: std::rc::Rc::new(Box::new(f)),
            prototype: std::rc::Rc::new(std::cell::RefCell::new(Some(prototype))),
            properties: std::rc::Rc::new(std::cell::RefCell::new(std::collections::HashMap::new())),
        }
    }

    /// Get a property from this native function
    pub fn get_property(&self, key: &str) -> Option<Value> {
        self.properties.borrow().get(key).cloned()
    }

    /// Call the native function with arguments and a this binding
    pub fn call(&self, this_val: Value, args: Vec<Value>) -> Result<Value, JsError> {
        crate::interpreter::set_native_this(this_val);
        let result = (self.func)(args);
        crate::interpreter::take_native_this();
        result
    }

    /// Set a property on this native function (e.g., prototype)
    pub fn set_property(&self, key: &str, value: Value) {
        if key == "prototype" {
            if let Value::Object(o) = &value {
                // Set constructor on the prototype
                o.borrow_mut().set(
                    "constructor",
                    Value::NativeFunction(std::rc::Rc::new(self.clone())),
                );
                // Lazily install the prototype reference so later reads via
                // fn.prototype return the same instance instead of a fresh
                // synthesized object.
                if self.prototype.borrow().is_none() {
                    *self.prototype.borrow_mut() = Some(Rc::clone(o));
                }
            }
            // Keep the prototype readable via get_property; otherwise a
            // NativeFunction created without a prototype would synthesize a
            // fresh (wrong) object on every `fn.prototype` read.
            self.properties.borrow_mut().insert(key.to_string(), value);
        } else {
            // Store other properties
            self.properties.borrow_mut().insert(key.to_string(), value);
        }
    }
}

impl fmt::Debug for NativeFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NativeFunction(...)")
    }
}

impl Clone for NativeFunction {
    fn clone(&self) -> Self {
        NativeFunction {
            func: self.func.clone(),
            prototype: std::rc::Rc::clone(&self.prototype),
            // Share the properties HashMap via Rc so clones see the same properties
            properties: std::rc::Rc::clone(&self.properties),
        }
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
    /// Static methods on the constructor
    static_methods: std::collections::HashMap<String, Value>,
    /// The name of the constructor (for Error.name matching)
    name: String,
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
            static_methods: std::collections::HashMap::new(),
            name: String::new(),
        }
    }

    /// Get the name of this constructor
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Set the name of this constructor
    pub fn set_name(&mut self, name: &str) {
        self.name = name.to_string();
    }

    /// Set a static method on the constructor
    pub fn set_static_method(&mut self, name: &str, value: Value) {
        self.static_methods.insert(name.to_string(), value);
    }

    /// Get a static method from the constructor
    pub fn get_static_method(&self, name: &str) -> Option<Value> {
        self.static_methods.get(name).cloned()
    }

    /// Call the constructor with arguments and a this binding
    pub fn call(&self, this_val: Value, args: Vec<Value>) -> Result<Value, JsError> {
        crate::interpreter::set_native_this(this_val);
        let result = (self.func)(args);
        crate::interpreter::take_native_this();
        result
    }

    /// Get the internal function Rc for comparison
    pub(crate) fn func_rc(&self) -> &NativeFn {
        &self.func
    }

    /// Call the inner function directly, setting native_this to this constructor.
    pub(crate) fn call_func(&self, args: Vec<Value>) -> Result<Value, JsError> {
        (self.func)(args)
    }

    /// Set a property on this native constructor (e.g., prototype)
    pub fn set_property(&self, _key: &str, _value: Value) {
        // No-op for now - NativeConstructor stores prototype differently
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
            static_methods: self.static_methods.clone(),
            name: self.name.clone(),
        }
    }
}
