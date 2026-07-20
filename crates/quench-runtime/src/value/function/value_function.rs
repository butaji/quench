//! ValueFunction - JavaScript function values.

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use crate::ast::{ArrowBody, Param, Statement};
use crate::env::Environment;
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

// =============================================================================
// ValueFunction
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
    /// Whether this is an async function (wraps return value in Promise.resolve())
    pub is_async: bool,
    /// Whether this is a generator function (has yield capability)
    pub is_generator: bool,
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
        // mutations are visible to subsequent accesses.
        ValueFunction {
            name: self.name.clone(),
            params: self.params.clone(),
            body: std::rc::Rc::clone(&self.body),
            arrow_body: std::rc::Rc::clone(&self.arrow_body),
            closure: std::rc::Rc::clone(&self.closure),
            is_arrow: self.is_arrow,
            is_async: self.is_async,
            is_generator: self.is_generator,
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
/// including) the first one with a default value, then stop.
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
        is_async: bool,
        is_generator: bool,
    ) -> Self {
        let length = expected_argument_count(&params);
        let mut props = std::collections::HashMap::new();
        props.insert("length".to_string(), Value::Number(length));
        if let Some(ref n) = name {
            props.insert("name".to_string(), Value::String(n.clone()));
        }
        ValueFunction {
            name,
            params,
            body: std::rc::Rc::new(body),
            arrow_body: std::rc::Rc::new(None),
            closure,
            is_arrow: false,
            is_async,
            is_generator,
            strict: false,
            proto_cell: ProtoCellRef::Strong(Rc::new(RefCell::new(None))),
            properties: std::rc::Rc::new(std::cell::RefCell::new(props)),
        }
    }

    /// Create a new arrow function
    #[allow(clippy::boxed_local)]
    pub fn new_arrow(
        params: Vec<Param>,
        body: Box<ArrowBody>,
        closure: Rc<RefCell<Environment>>,
    ) -> Self {
        let length = expected_argument_count(&params);
        let mut props = std::collections::HashMap::new();
        props.insert("length".to_string(), Value::Number(length));
        ValueFunction {
            name: None,
            params,
            body: std::rc::Rc::new(Vec::new()),
            arrow_body: std::rc::Rc::new(Some(*body)),
            closure,
            is_arrow: true,
            is_async: false,
            is_generator: false,
            strict: false,
            proto_cell: ProtoCellRef::Strong(Rc::new(RefCell::new(None))),
            properties: std::rc::Rc::new(std::cell::RefCell::new(props)),
        }
    }

    /// Get the prototype object for this function, creating it if needed.
    pub fn get_prototype(&self) -> Rc<RefCell<Object>> {
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

    /// Identity key for strict equality.
    pub(crate) fn identity_ptr(&self) -> *const RefCell<Option<Rc<RefCell<Object>>>> {
        self.proto_cell.as_ptr()
    }

    /// Compute the function's length per ECMA-262 14.1 / 9.2.4
    pub fn length(&self) -> usize {
        expected_argument_count(&self.params) as usize
    }

    /// Get a property from this function (e.g., sameValue, notSameValue)
    pub fn get_property(&self, key: &str) -> Option<Value> {
        self.properties.borrow().get(key).cloned()
    }

    /// Set a property on this function (e.g., prototype).
    pub fn set_property(&self, key: &str, value: Value) {
        self.with_mut(|props| {
            props.insert(key.to_string(), value);
        });
    }

    /// Remove a property. Returns true if it was present.
    pub fn remove_property(&self, key: &str) -> bool {
        self.properties.borrow_mut().remove(key).is_some()
    }

    /// Access properties with mutable borrow.
    fn with_mut<F>(&self, f: F)
    where
        F: FnOnce(&mut std::collections::HashMap<String, Value>),
    {
        let mut map = self.properties.borrow_mut();
        f(&mut map);
    }
}
