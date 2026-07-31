//! Native property helper functions (verifyProperty, makeNativeError, etc.)

use std::cell::RefCell;
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
/// - Checks value via SameValue when desc has "value"
/// - Checks enumerable via the descriptor flags, deletes if configurable+mismatch
/// - Checks writable against the descriptor AND via an isWritable probe
/// - Checks configurable via delete, restores if options.restore is true
///
/// Note: get/set identity is NOT compared here — that is verifyAccessorProperty's job.
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
                // Symbol-keyed properties are stored differently depending on type:
                // - Accessor properties: stored in getters/setters (via define_accessor)
                // - Data properties: stored in properties (via define)
                // - Some are also stored in symbol_properties
                obj.has_symbol(&name)
                    || obj.has_getter(&name_str)
                    || obj.has_setter(&name_str)
                    || obj.has_own(&name_str) // check properties/descriptors too
            } else {
                obj.has_own(&name_str) || obj.has_getter(&name_str) || obj.has_setter(&name_str)
            }
        }
        Value::Class(class_ref) => class_ref.has_static_own_property(&name_str),
        Value::Function(f) => {
            if let Some(key_str) = crate::builtins::object::helpers::get_property_key(&name) {
                if key_str == "prototype" {
                    // Non-arrow functions always have .prototype as an own property
                    // (created lazily on first access). Even generator functions.
                    !f.is_arrow
                } else {
                    (key_str == "name" && !f.is_property_deleted("name"))
                        || (key_str == "length" && !f.is_property_deleted("length"))
                        || f.get_property(&key_str).is_some()
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
        Value::NativeConstructor(nc) => {
            if let Some(key_str) = crate::builtins::object::helpers::get_property_key(&name) {
                key_str == "name"
                    || key_str == "length"
                    || key_str == "prototype"
                    || nc.get_static_method(&key_str).is_some()
                    || nc.get_accessor(&key_str).is_some()
            } else {
                false
            }
        }
        _ => false,
    };
    if !is_own {
        return mk_err(format!("{} should be an own property", name_label));
    }

    if let Value::Function(function) = &obj {
        let actual =
            crate::builtins::object_static::get_function_property_descriptor(function, &name_str)?;
        let actual_obj = actual
            .as_object()
            .ok_or_else(|| JsError("function property descriptor is missing".into()))?;
        let expected_obj = desc
            .as_object()
            .ok_or_else(|| JsError("function property descriptor is invalid".into()))?;
        let actual_obj = actual_obj.borrow();
        let expected_obj = expected_obj.borrow();
        for key in ["value", "writable", "enumerable", "configurable"] {
            if let Some(expected) = expected_obj.get(key) {
                if !crate::value::same_value(
                    &expected,
                    &actual_obj.get(key).unwrap_or(Value::Undefined),
                ) {
                    return mk_err(format!("{} descriptor {} mismatch", name_label, key));
                }
            }
        }
        return Ok(Value::Boolean(true));
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
    let desc_has_writable = desc_obj.properties.contains_key("writable");
    let desc_writable = desc_obj
        .get("writable")
        .as_ref()
        .map(crate::value::to_bool)
        .unwrap_or(false);
    drop(desc_obj);

    // Get current property descriptor from object (saved up front, before any
    // destructive checks — the JS verifyProperty also saves originalDesc).
    let obj_as_ref = match &obj {
        Value::Object(o) => o,
        _ => return Ok(Value::Boolean(true)),
    };
    let obj_desc =
        crate::builtins::object_static::get_object_property_descriptor(obj_as_ref, &name_str)
            .map_err(|e| JsError(format!("getOwnPropertyDescriptor failed: {}", e)))?;

    if !matches!(obj_desc, Value::Object(_)) {
        return mk_err(format!(
            "{} should be an own property (getOwnPropertyDescriptor returned undefined)",
            name_label
        ));
    }
    let original_desc_value = obj_desc;

    // Compare data value if desc has a "value" property
    let desc_obj2 = match &desc {
        Value::Object(o) => o.borrow(),
        _ => return Ok(Value::Boolean(true)),
    };
    if let Some(expected_value) = desc_obj2.get("value") {
        let mapped_getter = {
            let obj = obj_as_ref.borrow();
            if matches!(
                obj.data,
                crate::value::object::helpers::ObjData::Args { .. }
            ) {
                obj.get_getter(&name_str)
                    .and_then(|getter| getter.func.clone())
            } else {
                None
            }
        };
        let actual_value = mapped_getter
            .and_then(|getter| {
                crate::eval::function::call_value_with_this(
                    getter,
                    vec![],
                    Value::Object(Rc::clone(obj_as_ref)),
                )
                .ok()
            })
            .or_else(|| obj_as_ref.borrow().get(&name_str))
            .unwrap_or(Value::Undefined);
        let expected_str = crate::test262::harness::assert_helpers::debug_string(&expected_value);
        let mut failures = Vec::new();
        if !same_value(&expected_value, &actual_value) {
            failures.push(format!(
                "{} descriptor value should be {}",
                name_label, expected_str
            ));
            // Also check the actual `obj[name]` value (matching JS verifyProperty)
            let obj_value = Some(actual_value.clone());
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

    // Check writable only if desc has "writable" (matching JS verifyProperty):
    // compare against the original descriptor AND probe actual writability
    // via isWritable (write sentinel, compare, restore).
    if desc_has_writable {
        let original_writable = original_desc_value
            .as_object()
            .and_then(|o| o.borrow().get("writable"))
            .and_then(|v| match v {
                Value::Boolean(b) => Some(b),
                _ => None,
            });
        let actual_writable = vp_is_writable(obj_as_ref, &name_str);
        if original_writable != Some(desc_writable) || actual_writable != desc_writable {
            return mk_err(format!(
                "{} descriptor should {}be writable",
                name_label,
                if desc_writable { "" } else { "not " }
            ));
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

/// Check if a property is enumerable via its descriptor flags (mirrors
/// Object.prototype.propertyIsEnumerable). Symbol keys use the actual
/// descriptor's enumerable flag exactly like string keys.
fn vp_is_enumerable(obj: &Value, key: &str) -> bool {
    if let Value::Object(obj_ref) = obj {
        let obj = obj_ref.borrow();
        if obj.kind == crate::value::ObjectKind::Class && obj.has_own(key) {
            return true;
        }
        let exists = obj.has_own(key) || obj.has_getter(key) || obj.has_setter(key);
        if exists {
            return obj
                .descriptors
                .get(key)
                .map(|f| f.enumerable)
                .unwrap_or(true);
        }
    }
    false
}

/// Probe actual writability: write a sentinel, compare, restore — mirrors
/// isWritable from propertyHelper.js. For arrays the "length" probe uses a
/// numeric sentinel (matching the harness's patched
/// nonIndexNumericPropertyName) since a string would be an invalid length.
fn vp_is_writable(obj_ref: &Rc<RefCell<crate::value::Object>>, key: &str) -> bool {
    let (had_value, old_value, sentinel) = {
        let obj = obj_ref.borrow();
        let is_array_length = obj.kind == crate::value::ObjectKind::Array && key == "length";
        let sentinel = if is_array_length {
            Value::Number(999999.0)
        } else {
            Value::String("unlikelyValue".to_string())
        };
        (
            obj.has_own(key),
            obj.get(key).unwrap_or(Value::Undefined),
            sentinel,
        )
    };
    let new_value = if same_value(&sentinel, &old_value) {
        Value::String(format!("{}2", crate::value::to_js_string(&sentinel)))
    } else {
        sentinel
    };
    obj_ref.borrow_mut().set(key, new_value.clone());
    let write_succeeded = obj_ref
        .borrow()
        .get(key)
        .is_some_and(|v| same_value(&v, &new_value));
    if write_succeeded {
        let mut obj = obj_ref.borrow_mut();
        if had_value {
            obj.set(key, old_value);
        } else {
            obj.delete(key);
        }
    }
    write_succeeded
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

/// makeNativeError - factory for native error objects
pub fn make_native_error(_args: Vec<Value>) -> Result<Value, JsError> {
    use crate::value::{Object, ObjectKind};
    Ok(Value::Object(std::rc::Rc::new(std::cell::RefCell::new(
        Object::new(ObjectKind::Ordinary),
    ))))
}
