//! NativeConstructor - Host constructors (Date, Error, etc.).

use std::collections::HashSet;
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
    /// The constructor function's own [[Prototype]], when it differs from Function.prototype.
    pub own_prototype: Option<Value>,
    /// Static methods on the constructor
    /// Wrapped in RefCell so we can mutate even when shared via Rc
    static_methods: std::cell::RefCell<std::collections::HashMap<String, Value>>,
    deleted_static_methods: std::cell::RefCell<HashSet<String>>,
    non_deletable_static_methods: std::cell::RefCell<HashSet<String>>,
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
            own_prototype: None,
            static_methods: std::cell::RefCell::new(std::collections::HashMap::new()),
            deleted_static_methods: std::cell::RefCell::new(HashSet::new()),
            non_deletable_static_methods: std::cell::RefCell::new(HashSet::new()),
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

    pub fn set_own_prototype(&mut self, proto: Value) {
        self.own_prototype = Some(proto);
    }

    pub fn inherits_function_constructor(&self) -> bool {
        matches!(
            self.name().as_str(),
            "AsyncFunction" | "GeneratorFunction" | "AsyncGeneratorFunction"
        )
    }

    /// Set a static method on the constructor
    pub fn set_static_method(&self, name: &str, value: Value) {
        self.deleted_static_methods.borrow_mut().remove(name);
        if name == "assign" {
            if let Value::NativeFunction(function) = &value {
                function.define_property(
                    "name",
                    Value::String("assign".to_string()),
                    crate::value::PropertyFlags {
                        value: Some(Value::String("assign".to_string())),
                        writable: false,
                        enumerable: false,
                        configurable: true,
                    },
                );
                function.define_property(
                    "length",
                    Value::Number(2.0),
                    crate::value::PropertyFlags {
                        value: Some(Value::Number(2.0)),
                        writable: false,
                        enumerable: false,
                        configurable: true,
                    },
                );
            }
        }
        self.static_methods
            .borrow_mut()
            .insert(name.to_string(), value);
    }

    pub fn set_static_constant(&self, name: &str, value: Value) {
        self.set_static_method(name, value);
        self.non_deletable_static_methods
            .borrow_mut()
            .insert(name.to_string());
    }

    /// Get a static method from the constructor
    pub fn get_static_method(&self, name: &str) -> Option<Value> {
        if self.deleted_static_methods.borrow().contains(name) {
            return None;
        }
        self.static_methods.borrow().get(name).cloned()
    }

    pub fn normalize_static_method(&self, name: &str) {
        let Some(Value::Function(function)) = self.static_methods.borrow().get(name).cloned()
        else {
            return;
        };
        let _ = function.set_property("name", Value::String(name.to_string()));
        let _ = function.set_property("prototype", Value::Undefined);
        let _ = function.set_property("\0nonconstructable", Value::Boolean(true));
    }

    pub fn static_method_names(&self) -> Vec<String> {
        self.static_methods.borrow().keys().cloned().collect()
    }

    pub fn is_property_deleted(&self, name: &str) -> bool {
        self.deleted_static_methods.borrow().contains(name)
    }

    pub fn is_non_deletable_static_method(&self, name: &str) -> bool {
        self.non_deletable_static_methods.borrow().contains(name)
    }

    pub fn delete_static_method(&self, name: &str) -> bool {
        if self.non_deletable_static_methods.borrow().contains(name) {
            return false;
        }
        if self.static_methods.borrow().contains_key(name) || matches!(name, "length" | "name") {
            self.deleted_static_methods
                .borrow_mut()
                .insert(name.to_string());
            return true;
        }
        false
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
            own_prototype: self.own_prototype.clone(),
            static_methods: std::cell::RefCell::new(self.static_methods.borrow().clone()),
            deleted_static_methods: std::cell::RefCell::new(
                self.deleted_static_methods.borrow().clone(),
            ),
            non_deletable_static_methods: std::cell::RefCell::new(
                self.non_deletable_static_methods.borrow().clone(),
            ),
            accessors: std::rc::Rc::clone(&self.accessors),
            name: std::cell::RefCell::new(self.name.borrow().clone()),
        }
    }
}
