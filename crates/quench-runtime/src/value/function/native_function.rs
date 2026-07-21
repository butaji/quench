//! NativeFunction - Host functions provided by the runtime.

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use crate::value::error::JsError;
use crate::value::object::Object;
use crate::value::PropertyFlags;
use crate::value::Value;

/// Type alias for native function implementation
pub type NativeFn = std::rc::Rc<Box<dyn Fn(Vec<Value>) -> Result<Value, JsError>>>;

/// Native function - a host-provided function wrapped in Rc for shared ownership.
/// These are functions provided by the runtime (e.g., console.log, Math.sin).
pub struct NativeFunction {
    pub func: NativeFn,
    /// The function's own [[Prototype]] (internal slot, used by Object.getPrototypeOf).
    /// Separate from the `.prototype` property used for instanceof.
    /// Can hold Object or NativeFunction (since functions are objects in JS).
    pub own_prototype: Option<Value>,
    /// The `.prototype` property (used for instanceof with new).
    /// Wrapped in RefCell so set_property can lazily install it without
    /// requiring a mutable receiver.
    pub prototype: std::rc::Rc<std::cell::RefCell<Option<Rc<RefCell<Object>>>>>,
    /// Additional properties (for JS compatibility) - shared via Rc so clones share properties
    properties: std::rc::Rc<std::cell::RefCell<std::collections::HashMap<String, Value>>>,
    /// Property flags for NativeFunction properties (e.g., name is non-writable)
    property_flags:
        std::rc::Rc<std::cell::RefCell<std::collections::HashMap<String, PropertyFlags>>>,
    /// Function name (for direct eval detection: only "eval" is direct eval)
    pub name: String,
}

impl NativeFunction {
    /// Create a new native function from a closure (name defaults to "")
    pub fn new<F>(f: F) -> Self
    where
        F: Fn(Vec<Value>) -> Result<Value, JsError> + 'static,
    {
        NativeFunction {
            func: std::rc::Rc::new(Box::new(f)),
            own_prototype: None,
            prototype: std::rc::Rc::new(std::cell::RefCell::new(None)),
            properties: std::rc::Rc::new(std::cell::RefCell::new(std::collections::HashMap::new())),
            property_flags: std::rc::Rc::new(std::cell::RefCell::new(
                std::collections::HashMap::new(),
            )),
            name: String::new(),
        }
    }

    /// Create a new native function with a name (used for the built-in eval function)
    pub fn new_named<F>(name: &str, f: F) -> Self
    where
        F: Fn(Vec<Value>) -> Result<Value, JsError> + 'static,
    {
        NativeFunction {
            func: std::rc::Rc::new(Box::new(f)),
            own_prototype: None,
            prototype: std::rc::Rc::new(std::cell::RefCell::new(None)),
            properties: std::rc::Rc::new(std::cell::RefCell::new(std::collections::HashMap::new())),
            property_flags: std::rc::Rc::new(std::cell::RefCell::new(
                std::collections::HashMap::new(),
            )),
            name: name.to_string(),
        }
    }

    /// Create a new native function with an explicit name
    pub fn new_with_name<F>(name: &str, f: F) -> Self
    where
        F: Fn(Vec<Value>) -> Result<Value, JsError> + 'static,
    {
        NativeFunction {
            func: std::rc::Rc::new(Box::new(f)),
            own_prototype: None,
            prototype: std::rc::Rc::new(std::cell::RefCell::new(None)),
            properties: std::rc::Rc::new(std::cell::RefCell::new(std::collections::HashMap::new())),
            property_flags: std::rc::Rc::new(std::cell::RefCell::new(
                std::collections::HashMap::new(),
            )),
            name: name.to_string(),
        }
    }

    /// Create a new native function with a prototype
    pub fn new_with_prototype<F>(f: F, prototype: Rc<RefCell<Object>>) -> Self
    where
        F: Fn(Vec<Value>) -> Result<Value, JsError> + 'static,
    {
        NativeFunction {
            func: std::rc::Rc::new(Box::new(f)),
            own_prototype: None,
            prototype: std::rc::Rc::new(std::cell::RefCell::new(Some(prototype))),
            properties: std::rc::Rc::new(std::cell::RefCell::new(std::collections::HashMap::new())),
            property_flags: std::rc::Rc::new(std::cell::RefCell::new(
                std::collections::HashMap::new(),
            )),
            name: String::new(),
        }
    }

    /// Create a new native function where the function itself is used as the prototype
    /// for another constructor (the prototype's internal [[Prototype]] is this function).
    /// This is used for TypedArray, where TypedArray (a NativeFunction) serves as
    /// Uint8Array's prototype so Object.getPrototypeOf(Uint8Array) === TypedArray.
    pub fn new_with_fn_as_prototype<F>(
        f: F,
        fn_as_proto: Rc<NativeFunction>,
        instance_proto: Rc<RefCell<Object>>,
    ) -> Self
    where
        F: Fn(Vec<Value>) -> Result<Value, JsError> + 'static,
    {
        NativeFunction {
            func: std::rc::Rc::new(Box::new(f)),
            // Function's own [[Prototype]] = the TypedArray function itself
            // This makes Object.getPrototypeOf(Uint8Array) === TypedArray
            own_prototype: Some(Value::NativeFunction(fn_as_proto)),
            // .prototype property = TypedArrayPrototype (instance prototype)
            prototype: std::rc::Rc::new(std::cell::RefCell::new(Some(instance_proto))),
            properties: std::rc::Rc::new(std::cell::RefCell::new(std::collections::HashMap::new())),
            property_flags: std::rc::Rc::new(std::cell::RefCell::new(
                std::collections::HashMap::new(),
            )),
            name: String::new(),
        }
    }

    /// Set the function's own [[Prototype]] to `proto` (Object variant).
    pub fn set_own_prototype(&mut self, proto: Rc<RefCell<Object>>) {
        self.own_prototype = Some(Value::Object(proto));
    }

    /// Set the function's own [[Prototype]] to a NativeFunction.
    pub fn set_own_prototype_fn(&mut self, proto: Rc<NativeFunction>) {
        self.own_prototype = Some(Value::NativeFunction(proto));
    }

    /// Get a property from this native function
    pub fn get_property(&self, key: &str) -> Option<Value> {
        self.properties.borrow().get(key).cloned()
    }

    /// Get property flags for a property on this native function
    pub fn get_property_flags(&self, key: &str) -> Option<PropertyFlags> {
        self.property_flags.borrow().get(key).cloned()
    }

    /// Call the native function with arguments and a this binding
    pub fn call(&self, this_val: Value, args: Vec<Value>) -> Result<Value, JsError> {
        crate::interpreter::set_native_this(this_val);
        let result = (self.func)(args);
        crate::interpreter::take_native_this();
        result
    }

    /// Set a property on this native function (e.g., prototype).
    /// Returns Ok(()) on success, or Err(JsError) if the property is non-writable.
    pub fn set_property(&self, key: &str, value: Value) -> Result<(), JsError> {
        // Check if property is non-writable
        if let Some(flags) = self.property_flags.borrow().get(key) {
            if !flags.writable {
                let (_, err) = crate::value::error::create_js_error_with_type(
                    &format!("Cannot assign to read only property '{}'", key),
                    "TypeError",
                );
                return Err(err);
            }
        }

        if key == "prototype" {
            if let Value::Object(o) = &value {
                // Set constructor on the prototype
                o.borrow_mut().set(
                    "constructor",
                    Value::NativeFunction(std::rc::Rc::new(self.clone())),
                );
                // Lazily install the prototype reference
                if self.prototype.borrow().is_none() {
                    *self.prototype.borrow_mut() = Some(Rc::clone(o));
                }
            }
            self.properties.borrow_mut().insert(key.to_string(), value);
        } else {
            self.properties.borrow_mut().insert(key.to_string(), value);
        }
        Ok(())
    }

    /// Define a property with explicit flags on this native function.
    pub fn define_property(&self, key: &str, value: Value, flags: PropertyFlags) {
        self.properties.borrow_mut().insert(key.to_string(), value);
        self.property_flags
            .borrow_mut()
            .insert(key.to_string(), flags);
    }

    /// Remove a property from this native function.
    /// Returns true if the property was present.
    pub fn remove_property(&self, key: &str) -> bool {
        let removed = self.properties.borrow_mut().remove(key).is_some();
        self.property_flags.borrow_mut().remove(key);
        removed
    }
}

impl fmt::Debug for NativeFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NativeFunction(...)")
    }
}

impl PartialEq for NativeFunction {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && std::rc::Rc::ptr_eq(&self.func, &other.func)
    }
}

impl Clone for NativeFunction {
    fn clone(&self) -> Self {
        NativeFunction {
            func: self.func.clone(),
            own_prototype: self.own_prototype.clone(),
            prototype: std::rc::Rc::clone(&self.prototype),
            properties: std::rc::Rc::clone(&self.properties),
            property_flags: std::rc::Rc::clone(&self.property_flags),
            name: self.name.clone(),
        }
    }
}
