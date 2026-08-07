//! NativeConstructor - Host constructors (Date, Error, etc.).

use std::fmt;

use crate::value::error::JsError;
use crate::value::object::Object;
use crate::value::Value;

/// Stored getter/setter for accessor properties on NativeConstructor
#[derive(Clone)]
pub struct ConstructorAccessor {
    pub getter: Option<Value>,
    pub setter: Option<Value>,
}

/// Native constructor - a host-provided constructor function.
/// Similar to NativeFunction but has a prototype property for instanceof checks.
pub struct NativeConstructor {
    /// The constructor function wrapped in Rc for shared ownership
    func: super::NativeFn,
    /// The prototype object for instanceof checks
    pub prototype: std::rc::Rc<std::cell::RefCell<Object>>,
    /// Static methods on the constructor
    /// Wrapped in RefCell so we can mutate even when shared via Rc
    static_methods: std::cell::RefCell<std::collections::HashMap<String, Value>>,
    /// Accessor properties (getters/setters) defined via Object.defineProperty
    /// Wrapped in RefCell so we can mutate even when shared via Rc
    accessors:
        std::rc::Rc<std::cell::RefCell<std::collections::HashMap<String, ConstructorAccessor>>>,
    /// The name of the constructor (for Error.name matching)
    name: std::cell::RefCell<String>,
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
            static_methods: std::cell::RefCell::new(std::collections::HashMap::new()),
            accessors: std::rc::Rc::new(std::cell::RefCell::new(std::collections::HashMap::new())),
            name: std::cell::RefCell::new(String::new()),
        }
    }

    /// Get the name of this constructor
    pub fn name(&self) -> String {
        self.name.borrow().clone()
    }

    /// Set the name of this constructor
    pub fn set_name(&self, name: &str) {
        *self.name.borrow_mut() = name.to_string();
    }

    /// Set a static method on the constructor
    pub fn set_static_method(&self, name: &str, value: Value) {
        self.static_methods
            .borrow_mut()
            .insert(name.to_string(), value);
    }

    /// Get a static method from the constructor
    pub fn get_static_method(&self, name: &str) -> Option<Value> {
        self.static_methods.borrow().get(name).cloned()
    }

    /// Define an accessor property on this constructor (for Object.defineProperty)
    pub fn define_accessor(&self, name: &str, getter: Option<Value>, setter: Option<Value>) {
        self.accessors
            .borrow_mut()
            .insert(name.to_string(), ConstructorAccessor { getter, setter });
    }

    /// Get an accessor property from this constructor
    pub fn get_accessor(&self, name: &str) -> Option<ConstructorAccessor> {
        self.accessors.borrow().get(name).cloned()
    }

    /// Call the constructor with arguments and a this binding
    pub fn call(&self, this_val: Value, args: Vec<Value>) -> Result<Value, JsError> {
        crate::interpreter::set_native_this(this_val);
        let result = (self.func)(args);
        crate::interpreter::take_native_this();
        result
    }

    /// Get the internal function Rc for comparison
    pub(crate) fn func_rc(&self) -> &super::NativeFn {
        &self.func
    }

    /// Call the inner function directly, setting native_this to this constructor.
    pub(crate) fn call_func(&self, args: Vec<Value>) -> Result<Value, JsError> {
        (self.func)(args)
    }

    /// Set a property on this native constructor (e.g., static methods).
    /// Delegates to static_methods for consistency with NativeFunction.
    pub fn set_property(&self, key: &str, value: Value) {
        self.set_static_method(key, value);
    }
}

impl fmt::Debug for NativeConstructor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NativeConstructor(...)")
    }
}

impl PartialEq for NativeConstructor {
    fn eq(&self, other: &Self) -> bool {
        std::rc::Rc::ptr_eq(&self.func, &other.func) && *self.name.borrow() == *other.name.borrow()
    }
}

impl Clone for NativeConstructor {
    fn clone(&self) -> Self {
        NativeConstructor {
            func: self.func.clone(),
            prototype: std::rc::Rc::clone(&self.prototype),
            static_methods: std::cell::RefCell::new(self.static_methods.borrow().clone()),
            accessors: std::rc::Rc::clone(&self.accessors),
            name: std::cell::RefCell::new(self.name.borrow().clone()),
        }
    }
}
