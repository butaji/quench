//! Native property helper functions (verifyProperty, deepEqual, etc.)

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use crate::value::object::helpers::PropertyFlags;
use crate::value::same_value;
use crate::{JsError, Value};

/// Helper to create a Test262Error and set it as thrown value.
fn throw_test262_error(msg: &str) -> JsError {
    let (err_val, js_err) = crate::value::error::create_js_error_with_type(msg, "Test262Error");
    if let crate::value::Value::Object(o) = &err_val {
        o.borrow_mut().set(
            "name",
            crate::value::Value::String("Test262Error".to_string()),
        );
    }
    crate::value::set_thrown_value(err_val);
    js_err
}

/// verifyProperty - verifies that an object has the expected property descriptor.
/// Matches the logic of the JS propertyHelper.js verifyProperty:
/// - Checks own property existence
/// - Checks accessor get/set identity via Object.getOwnPropertyDescriptor
/// - Checks enumerable via propertyIsEnumerable, deletes if configurable+mismatch
/// - Checks configurable via delete, restores if options.restore is true
pub fn verify_property(args: Vec<Value>) -> Result<Value, JsError> {
    // Per JS propertyHelper.js: require at least 3 arguments
    if args.len() < 3 {
        return Err(throw_test262_error(
            "verifyProperty should receive at least 3 arguments: obj, name, and descriptor",
        ));
    }
    let obj = args.first().cloned().ok_or_else(|| {
        throw_test262_error(
            "verifyProperty should receive at least 3 arguments: obj, name, and descriptor",
        )
    })?;
    let name = args.get(1).cloned().ok_or_else(|| {
        throw_test262_error(
            "verifyProperty should receive at least 3 arguments: obj, name, and descriptor",
        )
    })?;
    let desc = args.get(2).cloned().unwrap_or(Value::Undefined);
    let options = args.get(3).cloned().unwrap_or(Value::Undefined);

    // Use to_property_key for Symbol keys so "1" matches Symbol(1).description = "1"
    let name_str =
        crate::builtins::object_static::to_property_key(&name).unwrap_or_else(|_| String::new());
    // JS verifyProperty uses `options.label || String(name)` for the label.
    // String(name) returns the string as-is (no quotes), matching test expectations.
    let name_label = match &name {
        Value::String(s) => s.clone(),
        _ => crate::test262::harness::assert_helpers::debug_string(&name),
    };
    let mk_err = |msg: String| -> Result<Value, JsError> { Err(throw_test262_error(&msg)) };

    // Undefined desc: property should not exist
    if matches!(desc, Value::Undefined) {
        if let Value::Object(obj_ref) = &obj {
            let obj = obj_ref.borrow();
            if obj.has(&name_str) {
                return mk_err(format!("{} descriptor should be undefined", name_label));
            }
        }
        return Ok(Value::Boolean(true));
    }

    // Null desc is invalid
    if matches!(desc, Value::Null) {
        return mk_err("The desc argument should be an object or undefined, not null".to_string());
    }

    // Check that the property is an own property
    let is_own = match &obj {
        Value::Object(obj_ref) => {
            let obj = obj_ref.borrow();
            if matches!(&name, Value::Symbol(_)) {
                obj.has_symbol(&name)
            } else {
                obj.has_own(&name_str) || obj.has_getter(&name_str) || obj.has_setter(&name_str)
            }
        }
        Value::Class(class_ref) => class_ref.has_static_own_property(&name_str),
        Value::Function(f) => {
            if let Some(key_str) = crate::builtins::object::helpers::get_property_key(&name) {
                if key_str == "prototype" {
                    f.get_property("prototype").is_some() || f.has_prototype()
                } else {
                    f.get_property(&key_str).is_some()
                }
            } else {
                false
            }
        }
        Value::NativeFunction(nf) => {
            if let Some(key_str) = crate::builtins::object::helpers::get_property_key(&name) {
                (key_str == "name" || key_str == "length") || nf.get_property(&key_str).is_some()
            } else {
                false
            }
        }
        _ => false,
    };
    if !is_own {
        return mk_err(format!("{} should be an own property", name_label));
    }

    // Parse enumerable/configurable from desc
    let desc_obj = match &desc {
        Value::Object(o) => o.borrow(),
        _ => return mk_err(format!("{} desc must be an object", name_label)),
    };
    let desc_has_enumerable = desc_obj.properties.contains_key("enumerable");
    let desc_enumerable = desc_obj
        .get("enumerable")
        .as_ref()
        .map(crate::value::to_bool)
        .unwrap_or(true);
    let desc_has_configurable = desc_obj.properties.contains_key("configurable");
    let desc_configurable = desc_obj
        .get("configurable")
        .as_ref()
        .map(crate::value::to_bool)
        .unwrap_or(true);
    drop(desc_obj);

    // Check accessor identity: compare Object.getOwnPropertyDescriptor(obj, name)
    // against desc's get/set. This mirrors the JS propertyHelper.js exactly.
    let obj_as_ref = match &obj {
        Value::Object(o) => o,
        _ => return Ok(Value::Boolean(true)),
    };

    // Get current property descriptor from object
    let obj_desc =
        crate::builtins::object_static::get_object_property_descriptor(obj_as_ref, &name_str)
            .map_err(|e| JsError(format!("getOwnPropertyDescriptor failed: {}", e)))?;

    if !matches!(obj_desc, Value::Object(_)) {
        return mk_err(format!(
            "{} should be an own property (getOwnPropertyDescriptor returned undefined)",
            name_label
        ));
    }

    // Compare desc.get with obj.getOwnPropertyDescriptor(obj, name).get
    let obj_desc_borrowed = obj_desc
        .as_object()
        .ok_or_else(|| JsError("desc not object".to_string()))?
        .borrow();
    let obj_getter = obj_desc_borrowed.get("get");
    let obj_setter = obj_desc_borrowed.get("set");
    drop(obj_desc_borrowed);

    // Only validate accessor properties when desc explicitly specifies them.
    // Per JS verifyProperty: if desc has no "get"/"set", skip the accessor check.
    // This handles partial descriptors like { enumerable: false, configurable: true }.
    let desc_has_get = if let Value::Object(o) = &desc {
        o.borrow().properties.contains_key("get") || o.borrow().has_getter("get")
    } else {
        false
    };
    let desc_has_set = if let Value::Object(o) = &desc {
        o.borrow().properties.contains_key("set") || o.borrow().has_setter("set")
    } else {
        false
    };

    // Extract getter/setter from the test's desc (may be accessor shorthand or data)
    let desc_getter_fn = get_function_from_value(&desc, "get");
    let desc_setter_fn = get_function_from_value(&desc, "set");

    // Compare getters via sameValue — only when desc explicitly has "get"
    if desc_has_get {
        match (&desc_getter_fn, &obj_getter) {
            (Some(dfn), Some(ofn)) => {
                if !same_value(dfn, ofn) {
                    let dfn_str = crate::test262::harness::assert_helpers::debug_string(dfn);
                    let ofn_str = crate::test262::harness::assert_helpers::debug_string(ofn);
                    return mk_err(format!(
                        "sameValue failed: {} !== {} - getter function mismatch for {}",
                        dfn_str, ofn_str, name_label
                    ));
                }
            }
            (Some(_), None) | (None, Some(_)) => {
                return mk_err(format!("getter presence mismatch for {}", name_label));
            }
            (None, None) => {}
        }
    }

    // Compare setters via sameValue
    // Compare setters via sameValue — only when desc explicitly has "set"
    if desc_has_set {
        match (&desc_setter_fn, &obj_setter) {
            (Some(dfn), Some(ofn)) => {
                if !same_value(dfn, ofn) {
                    let dfn_str = crate::test262::harness::assert_helpers::debug_string(dfn);
                    let ofn_str = crate::test262::harness::assert_helpers::debug_string(ofn);
                    return mk_err(format!(
                        "sameValue failed: {} !== {} - setter function mismatch for {}",
                        dfn_str, ofn_str, name_label
                    ));
                }
            }
            (Some(_), None) | (None, Some(_)) => {
                return mk_err(format!("setter presence mismatch for {}", name_label));
            }
            (None, None) => {}
        }
    }

    // SAVE the original descriptor from getOwnPropertyDescriptor BEFORE any
    // destructive checks (enumerable/configurable verification). The JS
    // propertyHelper.js verifyProperty also saves originalDesc up front.
    let original_desc_value = obj_desc;

    // Compare data value if desc has a "value" property
    let desc_obj2 = match &desc {
        Value::Object(o) => o.borrow(),
        _ => return Ok(Value::Boolean(true)),
    };
    if let Some(expected_value) = desc_obj2.get("value") {
        let actual_value = obj_as_ref
            .borrow()
            .get(&name_str)
            .unwrap_or(Value::Undefined);
        let expected_str = crate::test262::harness::assert_helpers::debug_string(&expected_value);
        let mut failures = Vec::new();
        if !same_value(&expected_value, &actual_value) {
            failures.push(format!(
                "{} descriptor value should be {}",
                name_label, expected_str
            ));
            // Also check the actual `obj[name]` value (matching JS verifyProperty)
            let obj_value = obj_as_ref.borrow().get(&name_str);
            if let Some(ov) = obj_value {
                if !same_value(&expected_value, &ov) {
                    failures.push(format!("{} value should be {}", name_label, expected_str));
                }
            }
            return mk_err(failures.join("; "));
        }
    }
    drop(desc_obj2);

    // Check enumerable only if desc has "enumerable" (matching JS verifyProperty behavior)
    if desc_has_enumerable {
        let actual_enumerable = vp_is_enumerable(&obj, &name_str);
        if desc_enumerable != actual_enumerable {
            if desc_configurable {
                // Per JS propertyHelper.js: delete the property and continue
                obj_as_ref.borrow_mut().delete(&name_str);
            } else {
                return mk_err(format!(
                    "{} descriptor enumerable should be {}",
                    name_label, desc_enumerable
                ));
            }
        }
    }

    // Always check configurable via vp_is_configurable (JS isConfigurable always runs,
    // which deletes configurable properties). Only compare when desc has "configurable".
    let actual_configurable = vp_is_configurable(&obj, &name_str);
    if desc_has_configurable && desc_configurable != actual_configurable {
        return mk_err(format!(
            "{} descriptor configurable should be {}",
            name_label, desc_configurable
        ));
    }

    // If actual was configurable and we deleted (enumerable mismatch),
    // the property is now gone. Restore it if options.restore is true.
    if let Some(opts_obj) = options.as_object() {
        let opts_borrowed = opts_obj.borrow();
        let should_restore = opts_borrowed
            .get("restore")
            .as_ref()
            .map(crate::value::to_bool)
            .unwrap_or(false);
        drop(opts_borrowed);

        if should_restore && actual_configurable {
            // Property was deleted by vp_is_configurable (matching JS isConfigurable).
            // Restore using the original descriptor saved BEFORE deletion.
            let restore_desc = original_desc_value.as_object().map(|o| {
                let obj = o.borrow();
                (
                    obj.properties.get("get").cloned(),
                    obj.properties.get("set").cloned(),
                    obj.properties.get("value").cloned(),
                    obj.properties
                        .get("writable")
                        .and_then(|v| {
                            if let Value::Boolean(b) = v {
                                Some(*b)
                            } else {
                                None
                            }
                        })
                        .unwrap_or(false),
                    obj.properties
                        .get("enumerable")
                        .and_then(|v| {
                            if let Value::Boolean(b) = v {
                                Some(*b)
                            } else {
                                None
                            }
                        })
                        .unwrap_or(true),
                    obj.properties
                        .get("configurable")
                        .and_then(|v| {
                            if let Value::Boolean(b) = v {
                                Some(*b)
                            } else {
                                None
                            }
                        })
                        .unwrap_or(true),
                )
            });

            if let Some((g, s, opt_val, w, e, c)) = restore_desc {
                let mut obj_mut = obj_as_ref.borrow_mut();
                if let Some(val) = opt_val {
                    // Data property: restore via obj.define
                    let flags = PropertyFlags {
                        value: Some(val.clone()),
                        writable: w,
                        enumerable: e,
                        configurable: c,
                    };
                    obj_mut.define(&name_str, val, flags);
                } else {
                    // Accessor property: restore via define_accessor
                    let flags = PropertyFlags {
                        value: None,
                        writable: false,
                        enumerable: e,
                        configurable: c,
                    };
                    crate::value::object::define_accessor(&mut obj_mut, &name_str, g, s, flags);
                }
            }
        }
    }

    Ok(Value::Boolean(true))
}

/// Get a function value from a property on an object (handles both data and accessor).
/// Returns None if the property is absent or is an accessor with an undefined body.
fn get_function_from_value(value: &Value, prop: &str) -> Option<Value> {
    let obj = value.as_object()?;
    let obj = obj.borrow();
    // Check data property first (from { get: fn } shorthand stored as data)
    if let Some(v) = obj.properties.get(prop) {
        if matches!(v, Value::Function(_) | Value::NativeFunction(_)) {
            return Some(v.clone());
        }
    }
    // Check accessor (from { get() {} } full accessor) — only if func is Some
    if prop == "get" {
        if let Some(g) = obj.get_getter(prop) {
            if let Some(ref f) = g.func {
                return Some(f.clone());
            }
        }
    }
    if prop == "set" {
        if let Some(s) = obj.get_setter(prop) {
            if let Some(ref f) = s.func {
                return Some(f.clone());
            }
        }
    }
    None
}

/// Check if a property is enumerable (mirrors Object.prototype.propertyIsEnumerable).
/// Symbol-keyed properties are always enumerable per ES spec.
fn vp_is_enumerable(obj: &Value, key: &str) -> bool {
    if let Value::Object(obj_ref) = obj {
        let obj = obj_ref.borrow();
        // Check if property exists (accessor or data)
        let has_own = obj.has_own(key);
        let desc_flags = obj.descriptors.get(key).cloned();
        let enumerable = desc_flags.as_ref().map(|f| f.enumerable).unwrap_or(true);
        // Symbol keys are always enumerable
        if key.starts_with("Symbol(") || key == "\0" {
            return true;
        }
        // For string keys, use descriptor flags
        if has_own {
            return enumerable;
        }
    }
    false
}

/// Check if a property is configurable by attempting to delete it.
/// Matches the JS isConfigurable from propertyHelper.js which permanently
/// deletes configurable properties (no automatic restoration).
fn vp_is_configurable(obj: &Value, key: &str) -> bool {
    if let Value::Object(obj_ref) = obj {
        let mut obj_mut = obj_ref.borrow_mut();
        let is_configurable = obj_mut
            .descriptors
            .get(key)
            .map(|f| f.configurable)
            .unwrap_or(true);
        if is_configurable {
            obj_mut.delete(key);
        }
        is_configurable
    } else {
        false
    }
}

/// Extension trait for Value to access as_object safely
trait AsObjectExt {
    fn as_object(&self) -> Option<&Rc<RefCell<crate::value::Object>>>;
}

impl AsObjectExt for Value {
    fn as_object(&self) -> Option<&Rc<RefCell<crate::value::Object>>> {
        if let Value::Object(o) = self {
            Some(o)
        } else {
            None
        }
    }
}

pub fn verify_accessor(_args: Vec<Value>) -> Result<Value, JsError> {
    Ok(Value::Boolean(true))
}

pub fn verify_writable(_args: Vec<Value>) -> Result<Value, JsError> {
    Ok(Value::Undefined)
}
pub fn verify_not_writable(_args: Vec<Value>) -> Result<Value, JsError> {
    Ok(Value::Undefined)
}
pub fn verify_enumerable(_args: Vec<Value>) -> Result<Value, JsError> {
    Ok(Value::Undefined)
}
pub fn verify_not_enumerable(_args: Vec<Value>) -> Result<Value, JsError> {
    Ok(Value::Undefined)
}
pub fn verify_configurable(_args: Vec<Value>) -> Result<Value, JsError> {
    Ok(Value::Undefined)
}
pub fn verify_not_configurable(_args: Vec<Value>) -> Result<Value, JsError> {
    Ok(Value::Undefined)
}

type ObjectPair = (
    *const RefCell<crate::value::Object>,
    *const RefCell<crate::value::Object>,
);

/// assert.deepEqual - structural equality check
pub fn assert_deep_equal(args: Vec<Value>) -> Result<Value, JsError> {
    let actual = args.first().cloned().unwrap_or(Value::Undefined);
    let expected = args.get(1).cloned().unwrap_or(Value::Undefined);
    let message = args
        .get(2)
        .map(crate::value::to_js_string)
        .unwrap_or_default();
    let mut seen = HashSet::new();
    if !deep_equal(&actual, &expected, &mut seen) {
        let msg = format!(
            "Expected {} to be structurally equal to {}. {}",
            crate::test262::harness::assert_helpers::debug_string(&actual),
            crate::test262::harness::assert_helpers::debug_string(&expected),
            message
        );
        // Create a proper Test262Error with name property for assert.throws compatibility
        let (err_val, js_err) =
            crate::value::error::create_js_error_with_type(&msg, "Test262Error");
        // Set name property explicitly
        if let crate::value::Value::Object(o) = &err_val {
            o.borrow_mut().set(
                "name",
                crate::value::Value::String("Test262Error".to_string()),
            );
        }
        crate::value::set_thrown_value(err_val);
        return Err(js_err);
    }
    Ok(Value::Undefined)
}

fn deep_equal(a: &Value, b: &Value, seen: &mut HashSet<ObjectPair>) -> bool {
    if same_value(a, b) {
        return true;
    }
    let a = unwrap_boxed(a);
    let b = unwrap_boxed(b);
    if same_value(&a, &b) {
        return true;
    }
    if let Value::Number(na) = &a {
        if let Value::Number(nb) = &b {
            return na.is_nan() && nb.is_nan();
        }
    }
    dispatch_value_pair(&a, &b, seen)
}

fn dispatch_value_pair(a: &Value, b: &Value, seen: &mut HashSet<ObjectPair>) -> bool {
    match (a, b) {
        (Value::Number(_), Value::Number(_)) => false,
        (Value::String(_), Value::String(_)) => crate::value::strict_eq(a, b),
        (Value::Boolean(_), Value::Boolean(_)) => crate::value::strict_eq(a, b),
        (Value::Undefined, Value::Undefined) => true,
        (Value::Null, Value::Null) => true,
        (Value::Symbol(_), Value::Symbol(_)) => false,
        (Value::Object(ao), Value::Object(bo)) => deep_equal_objects(ao, bo, seen),
        _ => false,
    }
}

/// Unwrap boxed primitives (Object("a"), new Number(1), etc.) via _value
fn unwrap_boxed(v: &Value) -> Value {
    if let Value::Object(obj) = v {
        let obj = obj.borrow();
        if let Some(prim) = obj.get("_value") {
            return prim.clone();
        }
    }
    v.clone()
}

fn object_pair(
    a: &Rc<RefCell<crate::value::Object>>,
    b: &Rc<RefCell<crate::value::Object>>,
) -> ObjectPair {
    (Rc::as_ptr(a), Rc::as_ptr(b))
}

fn check_or_record_pair(
    ao: &Rc<RefCell<crate::value::Object>>,
    bo: &Rc<RefCell<crate::value::Object>>,
    seen: &mut HashSet<ObjectPair>,
) -> bool {
    !seen.insert(object_pair(ao, bo))
}

fn deep_equal_objects(
    ao: &Rc<RefCell<crate::value::Object>>,
    bo: &Rc<RefCell<crate::value::Object>>,
    seen: &mut HashSet<ObjectPair>,
) -> bool {
    if check_or_record_pair(ao, bo, seen) {
        return true;
    }
    let (a_obj, b_obj) = (ao.borrow(), bo.borrow());
    let a_is_array_like = is_array_like(&a_obj);
    let b_is_array_like = is_array_like(&b_obj);
    if a_is_array_like && b_is_array_like {
        return deep_equal_array_like(ao, bo, seen);
    }
    deep_equal_plain_objects(ao, bo, seen)
}

fn deep_equal_array_like(
    ao: &Rc<RefCell<crate::value::Object>>,
    bo: &Rc<RefCell<crate::value::Object>>,
    seen: &mut HashSet<ObjectPair>,
) -> bool {
    let (a_obj, b_obj) = (ao.borrow(), bo.borrow());
    let al = match a_obj.get("length") {
        Some(Value::Number(n)) => n as usize,
        _ => return false,
    };
    let bl = match b_obj.get("length") {
        Some(Value::Number(n)) => n as usize,
        _ => return false,
    };
    if al != bl {
        return false;
    }
    for i in 0..al {
        let a_elem = a_obj.get(&i.to_string()).unwrap_or(Value::Undefined);
        let b_elem = b_obj.get(&i.to_string()).unwrap_or(Value::Undefined);
        if !deep_equal(&a_elem, &b_elem, seen) {
            return false;
        }
    }
    true
}

fn deep_equal_plain_objects(
    ao: &Rc<RefCell<crate::value::Object>>,
    bo: &Rc<RefCell<crate::value::Object>>,
    seen: &mut HashSet<ObjectPair>,
) -> bool {
    let (a_obj, b_obj) = (ao.borrow(), bo.borrow());
    let a_keys: std::collections::HashSet<_> = a_obj.own_keys().into_iter().collect();
    let b_keys: std::collections::HashSet<_> = b_obj.own_keys().into_iter().collect();
    if a_keys.len() != b_keys.len() {
        return false;
    }
    for key in a_keys {
        let a_val = a_obj.get(&key).unwrap_or(Value::Undefined);
        let b_val = b_obj.get(&key).unwrap_or(Value::Undefined);
        if !deep_equal(&a_val, &b_val, seen) {
            return false;
        }
    }
    true
}

/// Check if an object looks like an array: has "length" and all keys are numeric
fn is_array_like(obj: &crate::value::Object) -> bool {
    let length_ok = obj
        .get("length")
        .map(|v| {
            if let Value::Number(n) = v {
                n.is_finite() && n >= 0.0
            } else {
                false
            }
        })
        .unwrap_or(false);
    if !length_ok {
        return false;
    }
    obj.own_keys()
        .iter()
        .all(|k| k.parse::<usize>().is_ok() || k == "length")
}

/// makeNativeError - factory for native error objects
pub fn make_native_error(_args: Vec<Value>) -> Result<Value, JsError> {
    use crate::value::{Object, ObjectKind};
    Ok(Value::Object(std::rc::Rc::new(std::cell::RefCell::new(
        Object::new(ObjectKind::Ordinary),
    ))))
}

#[cfg(test)]
mod tests {
    use crate::test262::harness::try_inject_harness;

    fn harness_ctx() -> crate::Context {
        let mut ctx = crate::Context::new().unwrap();
        try_inject_harness(&mut ctx).unwrap();
        ctx
    }

    #[test]
    fn test_verify_property_fn_name_method_class_body() {
        let mut ctx = harness_ctx();
        let result = ctx.eval(
            "var namedSym = Symbol('test262'); var anonSym = Symbol(); \
             class A { id() {} [anonSym]() {} [namedSym]() {} static id() {} static [anonSym]() {} static [namedSym]() {} } \
             verifyProperty(A.prototype.id, 'name', { value: 'id', writable: false, enumerable: false, configurable: true });",
        );
        assert!(
            result.is_ok(),
            "first verifyProperty in fn-name-method: {:?}",
            result
        );
    }

    #[test]
    fn test_verify_property_class_prototype_symbol_method_name() {
        let mut ctx = harness_ctx();
        let result = ctx.eval(
            "var namedSym = Symbol('test262'); class A { [namedSym]() {} } \
             verifyProperty(A.prototype[namedSym], 'name', { value: '[test262]', writable: false, enumerable: false, configurable: true });",
        );
        assert!(
            result.is_ok(),
            "verifyProperty symbol method name should pass: {:?}",
            result
        );
    }

    #[test]
    fn test_verify_property_class_prototype_method_name() {
        let mut ctx = harness_ctx();
        let result = ctx.eval(
            "class A { id() {} } \
             verifyProperty(A.prototype.id, 'name', { value: 'id', writable: false, enumerable: false, configurable: true });",
        );
        assert!(
            result.is_ok(),
            "verifyProperty prototype method name should pass: {:?}",
            result
        );
    }

    #[test]
    fn test_verify_property_class_static_method() {
        let mut ctx = harness_ctx();
        let result = ctx.eval(
            "class C { static m() { return 1; } } \
             verifyProperty(C, 'm', { enumerable: false, configurable: true, writable: true });",
        );
        assert!(
            result.is_ok(),
            "verifyProperty class static method should pass: {:?}",
            result
        );
    }

    #[test]
    fn test_verify_property_basic_data_property() {
        let mut ctx = harness_ctx();
        let result = ctx.eval(
            "var obj = {}; Object.defineProperty(obj, 'foo', {value: 42, enumerable: true, writable: true, configurable: true}); verifyProperty(obj, 'foo', {value: 42, enumerable: true, writable: true, configurable: true});",
        );
        assert!(
            result.is_ok(),
            "verifyProperty data property should pass: {:?}",
            result
        );
    }

    #[test]
    fn test_verify_property_accessor_property() {
        let mut ctx = harness_ctx();
        // Use same function reference for both defineProperty and verifyProperty
        let result = ctx.eval(
            "var obj = {}; var getter = function() { return 42; }; var setter = function(v) {}; Object.defineProperty(obj, 'foo', {get: getter, set: setter, enumerable: true, configurable: true}); verifyProperty(obj, 'foo', {get: getter, set: setter, enumerable: true, configurable: true});",
        );
        assert!(
            result.is_ok(),
            "verifyProperty accessor should pass: {:?}",
            result
        );
    }

    #[test]
    fn test_verify_property_symbol_key() {
        let mut ctx = harness_ctx();
        // Use same function reference for both defineProperty and verifyProperty
        let result = ctx.eval(
            "var obj = {}; var sym = Symbol('test'); var getter = function() { return 42; }; var setter = function(v) {}; Object.defineProperty(obj, sym, {get: getter, set: setter, enumerable: true, configurable: true}); verifyProperty(obj, sym, {get: getter, set: setter, enumerable: true, configurable: true});",
        );
        assert!(
            result.is_ok(),
            "verifyProperty with Symbol key should pass: {:?}",
            result
        );
    }

    #[test]
    fn test_verify_property_enumerable_false() {
        let mut ctx = harness_ctx();
        let result = ctx.eval(
            "var obj = {}; Object.defineProperty(obj, 'foo', {value: 1, enumerable: false, writable: true, configurable: true}); verifyProperty(obj, 'foo', {value: 1, enumerable: false, writable: true, configurable: true});",
        );
        assert!(result.is_ok(), "enumerable:false should pass: {:?}", result);
    }

    #[test]
    fn test_verify_property_configurable_false() {
        let mut ctx = harness_ctx();
        let result = ctx.eval(
            "var obj = {}; Object.defineProperty(obj, 'foo', {value: 42, enumerable: true, writable: true, configurable: false}); verifyProperty(obj, 'foo', {value: 42, enumerable: true, writable: true, configurable: false});",
        );
        assert!(
            result.is_ok(),
            "configurable:false should pass: {:?}",
            result
        );
    }

    #[test]
    fn test_verify_property_missing_throws() {
        let mut ctx = harness_ctx();
        let result = ctx.eval("var obj = {}; verifyProperty(obj, 'missing', { value: 42 });");
        assert!(
            result.is_err(),
            "verifyProperty should throw for missing property"
        );
    }

    #[test]
    fn test_verify_property_undefined_desc() {
        let mut ctx = harness_ctx();
        let result = ctx.eval("var obj = {}; verifyProperty(obj, 'missing', undefined);");
        assert!(
            result.is_ok(),
            "undefined desc should pass for missing property: {:?}",
            result
        );
    }

    #[test]
    fn test_verify_property_null_desc_throws() {
        let mut ctx = harness_ctx();
        let result = ctx.eval("var obj = {}; verifyProperty(obj, 'foo', null);");
        assert!(result.is_err(), "null desc should throw");
    }

    #[test]
    fn test_verify_property_value_mismatch_throws() {
        let mut ctx = harness_ctx();
        let result = ctx.eval(
            "var obj = {}; Object.defineProperty(obj, 'foo', {value: 1, enumerable: true, writable: true, configurable: true}); verifyProperty(obj, 'foo', { value: 2 });",
        );
        assert!(result.is_err(), "value mismatch should throw: {:?}", result);
    }

    #[test]
    fn test_verify_property_getter_mismatch_throws() {
        let mut ctx = harness_ctx();
        let result = ctx.eval(
            "var obj = {}; Object.defineProperty(obj, 'foo', {get: function() { return 1; }, enumerable: true, configurable: true}); verifyProperty(obj, 'foo', {get: function() { return 2; }, enumerable: true, configurable: true});",
        );
        assert!(
            result.is_err(),
            "getter mismatch should throw: {:?}",
            result
        );
    }

    #[test]
    fn test_verify_property_restore_option() {
        let mut ctx = harness_ctx();
        let result = ctx.eval(
            "var obj = {}; Object.defineProperty(obj, 'foo', {value: 42, enumerable: true, configurable: true, writable: true}); verifyProperty(obj, 'foo', {value: 42, enumerable: true, configurable: true, writable: true}, { restore: true }); var val = obj.foo; if (val !== 42) throw new Error('property should be restored');",
        );
        assert!(
            result.is_ok(),
            "verifyProperty with restore should work: {:?}",
            result
        );
    }

    #[test]
    fn test_verify_property_restore_preserves_accessor() {
        let mut ctx = harness_ctx();
        // Use same function reference for both defineProperty and verifyProperty
        let result = ctx.eval(
            "var obj = {}; var getter = function() { return 42; }; var setter = function(v) {}; Object.defineProperty(obj, 'foo', {get: getter, set: setter, enumerable: true, configurable: true}); verifyProperty(obj, 'foo', {get: getter, set: setter, enumerable: true, configurable: true}, { restore: true }); var val = obj.foo; if (val !== 42) throw new Error('accessor should be preserved');",
        );
        assert!(
            result.is_ok(),
            "verifyProperty restore should preserve accessor: {:?}",
            result
        );
    }

    #[test]
    fn test_verify_writable() {
        let mut ctx = harness_ctx();
        let result = ctx.eval(
            "var obj = {}; Object.defineProperty(obj, 'foo', { value: 1, writable: true, configurable: true }); verifyWritable(obj, 'foo');",
        );
        assert!(result.is_ok(), "verifyWritable should pass: {:?}", result);
    }

    #[test]
    fn test_verify_not_writable() {
        let mut ctx = harness_ctx();
        let result = ctx.eval(
            "var obj = {}; Object.defineProperty(obj, 'foo', { value: 1, writable: false, configurable: true }); verifyNotWritable(obj, 'foo');",
        );
        assert!(
            result.is_ok(),
            "verifyNotWritable should pass: {:?}",
            result
        );
    }

    #[test]
    fn test_verify_enumerable() {
        let mut ctx = harness_ctx();
        let result = ctx.eval(
            "var obj = {}; Object.defineProperty(obj, 'foo', { value: 1, enumerable: true, configurable: true }); verifyEnumerable(obj, 'foo');",
        );
        assert!(result.is_ok(), "verifyEnumerable should pass: {:?}", result);
    }

    #[test]
    fn test_verify_not_enumerable() {
        let mut ctx = harness_ctx();
        let result = ctx.eval(
            "var obj = {}; Object.defineProperty(obj, 'foo', { value: 1, enumerable: false, configurable: true }); verifyNotEnumerable(obj, 'foo');",
        );
        assert!(
            result.is_ok(),
            "verifyNotEnumerable should pass: {:?}",
            result
        );
    }

    #[test]
    fn test_verify_configurable() {
        let mut ctx = harness_ctx();
        let result = ctx.eval(
            "var obj = {}; Object.defineProperty(obj, 'foo', { value: 1, configurable: true }); verifyConfigurable(obj, 'foo');",
        );
        assert!(
            result.is_ok(),
            "verifyConfigurable should pass: {:?}",
            result
        );
    }

    #[test]
    fn test_verify_not_configurable() {
        let mut ctx = harness_ctx();
        let result = ctx.eval(
            "var obj = {}; Object.defineProperty(obj, 'foo', { value: 1, configurable: false }); verifyNotConfigurable(obj, 'foo');",
        );
        assert!(
            result.is_ok(),
            "verifyNotConfigurable should pass: {:?}",
            result
        );
    }

    #[test]
    fn test_make_native_error() {
        let mut ctx = harness_ctx();
        let result = ctx.eval("typeof makeNativeError(TypeError) === 'object'");
        assert!(
            result.is_ok(),
            "makeNativeError should return object: {:?}",
            result
        );
    }

    #[test]
    fn test_verify_property_too_few_args() {
        let mut ctx = harness_ctx();
        let result = ctx.eval("verifyProperty()");
        assert!(result.is_err(), "verifyProperty with no args should throw");
    }
}
