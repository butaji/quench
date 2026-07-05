// linter-skip
//! Promise built-in implementation
//!
//! This module provides a basic Promise implementation for quench-runtime.
//! Since the runtime is synchronous, promises are resolved synchronously
//! when possible (fulfillment/rejection happens immediately).

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{NativeConstructor, NativeFunction, Object, ObjectKind, Value};
use crate::Context;

// ============================================================================
// Promise States
// ============================================================================

/// Promise state enumeration
#[derive(Debug, Clone, PartialEq)]
pub enum PromiseState {
    Pending,
    Fulfilled,
    Rejected,
}

/// Promise internal slots
#[derive(Debug, Clone)]
pub struct PromiseData {
    /// Promise state
    pub state: PromiseState,
    /// Promise result (value or error)
    pub result: Value,
}

impl PromiseData {
    fn new() -> Self {
        PromiseData {
            state: PromiseState::Pending,
            result: Value::Undefined,
        }
    }
}

// ============================================================================
// Promise Prototype Methods
// ============================================================================

/// Create Promise.prototype with then and catch methods
fn create_promise_proto() -> Rc<RefCell<Object>> {
    let proto = Object::new(ObjectKind::Ordinary);
    let proto_rc = Rc::new(RefCell::new(proto));

    // Promise.prototype.then(onFulfilled, onRejected)
    let _proto_clone = Rc::clone(&proto_rc);
    proto_rc.borrow_mut().set("then", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        let _on_fulfilled = args.get(0).cloned().unwrap_or(Value::Undefined);
        let _on_rejected = args.get(1).cloned().unwrap_or(Value::Undefined);
        
        // Create a new promise to return
        let new_promise = Object::new(ObjectKind::Promise);
        let new_promise_rc = Rc::new(RefCell::new(new_promise));
        let new_promise_clone = Rc::clone(&new_promise_rc);
        
        // Get the current promise's state from the this binding
        let this_val = crate::interpreter::get_native_this()
            .unwrap_or(Value::Undefined);
        
        if let Value::Object(ref obj) = this_val {
            let mut obj_mut = obj.borrow_mut();
            
            // Check if this is a Promise object with internal slots
            if let Some(promise_data) = obj_mut.promise_data.take() {
                match promise_data.state {
                    PromiseState::Fulfilled => {
                        let mut new_data = PromiseData::new();
                        new_data.state = PromiseState::Fulfilled;
                        new_data.result = promise_data.result.clone();
                        new_promise_clone.borrow_mut().promise_data = Some(new_data);
                    }
                    PromiseState::Rejected => {
                        let mut new_data = PromiseData::new();
                        new_data.state = PromiseState::Rejected;
                        new_data.result = promise_data.result.clone();
                        new_promise_clone.borrow_mut().promise_data = Some(new_data);
                    }
                    PromiseState::Pending => {
                        let new_data = PromiseData::new();
                        new_promise_clone.borrow_mut().promise_data = Some(new_data);
                    }
                }
            } else {
                let new_data = PromiseData::new();
                new_promise_clone.borrow_mut().promise_data = Some(new_data);
            }
            drop(obj_mut);
        } else {
            let new_data = PromiseData::new();
            new_promise_clone.borrow_mut().promise_data = Some(new_data);
        }
        
        Ok(Value::Object(new_promise_rc))
    }))));

    // Promise.prototype.catch(onRejected)
    proto_rc.borrow_mut().set("catch", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        let _on_rejected = args.first().cloned().unwrap_or(Value::Undefined);
        
        // Create a new promise
        let new_promise = Object::new(ObjectKind::Promise);
        let new_promise_rc = Rc::new(RefCell::new(new_promise));
        let new_promise_clone = Rc::clone(&new_promise_rc);
        
        // Get current promise state
        let this_val = crate::interpreter::get_native_this()
            .unwrap_or(Value::Undefined);
        
        if let Value::Object(ref obj) = this_val {
            let mut obj_mut = obj.borrow_mut();
            if let Some(promise_data) = obj_mut.promise_data.take() {
                let mut new_data = PromiseData::new();
                new_data.state = promise_data.state.clone();
                new_data.result = promise_data.result.clone();
                new_promise_clone.borrow_mut().promise_data = Some(new_data);
            } else {
                let new_data = PromiseData::new();
                new_promise_clone.borrow_mut().promise_data = Some(new_data);
            }
            drop(obj_mut);
        } else {
            let new_data = PromiseData::new();
            new_promise_clone.borrow_mut().promise_data = Some(new_data);
        }
        
        Ok(Value::Object(new_promise_rc))
    }))));

    // Promise.prototype.finally(onFinally) - basic implementation
    proto_rc.borrow_mut().set("finally", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        let _on_finally = args.first().cloned().unwrap_or(Value::Undefined);
        let this_val = crate::interpreter::get_native_this()
            .unwrap_or(Value::Undefined);
        
        // Return a new promise that resolves with the same value
        let new_promise = Object::new(ObjectKind::Promise);
        let new_promise_rc = Rc::new(RefCell::new(new_promise));
        let new_promise_clone = Rc::clone(&new_promise_rc);
        
        if let Value::Object(ref obj) = this_val {
            let mut obj_mut = obj.borrow_mut();
            if let Some(promise_data) = obj_mut.promise_data.take() {
                let mut new_data = PromiseData::new();
                new_data.state = promise_data.state.clone();
                new_data.result = promise_data.result.clone();
                new_promise_clone.borrow_mut().promise_data = Some(new_data);
            } else {
                let new_data = PromiseData::new();
                new_promise_clone.borrow_mut().promise_data = Some(new_data);
            }
            drop(obj_mut);
        } else {
            let new_data = PromiseData::new();
            new_promise_clone.borrow_mut().promise_data = Some(new_data);
        }
        
        Ok(Value::Object(new_promise_rc))
    }))));

    proto_rc
}

// ============================================================================
// Promise Constructor
// ============================================================================

/// Create Promise constructor with static methods
fn create_promise_constructor(proto: Rc<RefCell<Object>>) -> NativeConstructor {
    // Add static methods to a separate object
    let mut static_methods = std::collections::HashMap::new();
    
    // Promise.resolve(value)
    let resolve_proto = Rc::clone(&proto);
    static_methods.insert("resolve".to_string(), Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        let value = args.first().cloned().unwrap_or(Value::Undefined);
        
        // If value is already a promise, return it
        if let Value::Object(ref obj) = value {
            let obj_ref = obj.borrow();
            if obj_ref.kind == ObjectKind::Promise {
                return Ok(value.clone());
            }
        }
        
        // Create a new resolved promise
        let promise_obj = Object::new(ObjectKind::Promise);
        let promise_rc = Rc::new(RefCell::new(promise_obj));
        
        {
            let mut obj = promise_rc.borrow_mut();
            obj.prototype = Some(Rc::clone(&resolve_proto));
            let mut data = PromiseData::new();
            data.state = PromiseState::Fulfilled;
            data.result = value;
            obj.promise_data = Some(data);
        }
        
        Ok(Value::Object(promise_rc))
    }))));
    
    // Promise.reject(reason)
    let reject_proto = Rc::clone(&proto);
    static_methods.insert("reject".to_string(), Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        let reason = args.first().cloned().unwrap_or(Value::Undefined);
        
        // Create a new rejected promise
        let promise_obj = Object::new(ObjectKind::Promise);
        let promise_rc = Rc::new(RefCell::new(promise_obj));
        
        {
            let mut obj = promise_rc.borrow_mut();
            obj.prototype = Some(Rc::clone(&reject_proto));
            let mut data = PromiseData::new();
            data.state = PromiseState::Rejected;
            data.result = reason;
            obj.promise_data = Some(data);
        }
        
        Ok(Value::Object(promise_rc))
    }))));
    
    let proto_clone = Rc::clone(&proto);
    
    let mut constructor = NativeConstructor::new(
        move |_args| {
            // Note: The executor is ignored in this simplified implementation
            // Full Promise implementation would need to call the executor synchronously
            
            // Create the promise object
            let promise_obj = Object::new(ObjectKind::Promise);
            let promise_rc = Rc::new(RefCell::new(promise_obj));
            
            // Initialize promise data
            {
                let mut obj = promise_rc.borrow_mut();
                obj.prototype = Some(Rc::clone(&proto_clone));
                obj.promise_data = Some(PromiseData::new());
            }
            
            Ok(Value::Object(promise_rc))
        },
        proto,
    );
    
    // Add static methods
    for (name, value) in static_methods {
        constructor.set_static_method(&name, value);
    }
    
    constructor
}

// ============================================================================
// Register Promise
// ============================================================================

/// Register Promise into the context
pub fn register_promise(ctx: &mut Context) {
    // Create Promise.prototype
    let proto = create_promise_proto();
    
    // Create constructor with static methods
    let proto_clone = Rc::clone(&proto);
    let constructor = create_promise_constructor(proto_clone);
    
    // Link Promise.prototype to Object.prototype
    let object_proto = crate::value::get_object_prototype();
    if let Some(op) = object_proto {
        proto.borrow_mut().set("__proto__", Value::Object(op));
    }
    
    // Set up constructor property on prototype
    proto.borrow_mut().set("constructor", Value::NativeConstructor(Rc::new(constructor.clone())));
    
    // Set the constructor
    ctx.set_global("Promise".to_string(), Value::NativeConstructor(Rc::new(constructor)));
}

/// Get Promise prototype for instanceof checks
#[allow(dead_code)]
pub fn get_promise_prototype() -> Option<Rc<RefCell<Object>>> {
    // This would need to return the Promise prototype if we store it globally
    None
}
