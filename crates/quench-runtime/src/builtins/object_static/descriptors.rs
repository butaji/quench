//! Property descriptor helpers for Object.getOwnPropertyDescriptor,
//! Object.defineProperty, and related operations.

use crate::ast::PropertyKey;
use crate::env::Environment;
use crate::eval::class::helpers::{
    accessor_function_name, method_function_name, prop_key_to_string,
};
use crate::value::object::helpers::as_array_index;
use crate::value::{
    to_bool, to_js_string, to_primitive, JsError, PropertyFlags, Value, ValueFunction,
};
use crate::{Object, ObjectKind};

use std::cell::RefCell;
use std::rc::Rc;

/// Convert a value to a property key. Symbols use `desc\0id` so equal
/// descriptions never collide (AGENTS.md / R5).
pub fn to_property_key(v: &Value) -> Result<String, JsError> {
    let prim = to_primitive(v, Some("string"))?;
    match prim {
        Value::Symbol(s) => Ok(s.property_key()),
        _ => Ok(to_js_string(&prim)),
    }
}

/// Object.defineProperty(obj, prop, descriptor) - defines a property
pub fn object_define_property(args: Vec<Value>) -> Result<Value, JsError> {
    let obj = args.first().cloned().unwrap_or(Value::Undefined);
    let prop = args
        .get(1)
        .map(to_property_key)
        .unwrap_or(Ok("".to_string()))?;
    if let Value::Object(object) = &obj {
        crate::eval::member::trigger_deferred_namespace(object, &prop)?;
    }
    let desc = args
        .get(2)
        .ok_or_else(|| JsError::from("Object.defineProperty: descriptor required"))?;
    if !matches!(
        desc,
        Value::Object(_)
            | Value::Function(_)
            | Value::NativeFunction(_)
            | Value::NativeConstructor(_)
            | Value::Class(_)
    ) {
        return Err(JsError::from(
            "TypeError: property descriptor is not an object",
        ));
    }
    // Per spec, absent descriptor flags default to false for new properties.
    let mut flags = PropertyFlags {
        value: None,
        writable: false,
        enumerable: false,
        configurable: false,
    };
    let mut has_writable = false;
    let mut has_enumerable = false;
    let mut has_configurable = false;
    let mut getter: Option<Value> = None;
    let mut setter: Option<Value> = None;
    let descriptor_has_get = matches!(
        desc,
        Value::Object(descriptor) if descriptor.borrow().has_own("get")
    );
    let descriptor_has_set = matches!(
        desc,
        Value::Object(descriptor) if descriptor.borrow().has_own("set")
    );
    let setter_accessor = matches!(
        desc,
        Value::Object(descriptor) if descriptor.borrow().has_setter("set")
    );
    let mut descriptor_has_value = matches!(
        desc,
        Value::Object(descriptor) if descriptor.borrow().has_own("value")
    );
    let descriptor_has_writable = matches!(
        desc,
        Value::Object(descriptor) if descriptor.borrow().has_own("writable")
    );

    if let Value::Function(function) = desc {
        if let Some(value) = function.get_property("value") {
            descriptor_has_value = true;
            flags.value = Some(value);
        }
    }

    if let Value::Object(desc_obj) = desc {
        let descriptor_value = crate::eval::member::eval_object_member_value(
            desc_obj,
            &Value::String("value".to_string()),
            None,
        )?;
        let inherited_configurable = crate::eval::member::eval_object_member_value(
            desc_obj,
            &Value::String("configurable".to_string()),
            None,
        )?;
        let inherited_writable = crate::eval::member::eval_object_member_value(
            desc_obj,
            &Value::String("writable".to_string()),
            None,
        )?;
        let descriptor_get = crate::eval::member::eval_object_member_value(
            desc_obj,
            &Value::String("get".to_string()),
            None,
        )?;
        let mut descriptor_set = crate::eval::member::eval_object_member_value(
            desc_obj,
            &Value::String("set".to_string()),
            None,
        )?;
        if desc_obj.borrow().has_setter("set") && !desc_obj.borrow().has_getter("set") {
            descriptor_set = Value::Undefined;
        }
        let source_set_only =
            desc_obj.borrow().has_setter("set") && !desc_obj.borrow().has_getter("set");
        let desc_borrowed = desc_obj.borrow();
        // Per ES §10.1.6.1 ToPropertyDescriptor: use Get(desc, key) which
        // walks the prototype chain — the descriptor may inherit get/set/etc.
        // from prototype (e.g. using Math as a descriptor after modifying
        // Object.prototype.get).
        if !matches!(descriptor_value, Value::Undefined) {
            flags.value = Some(descriptor_value);
        } else if let Some(val) = desc_borrowed.get("value") {
            flags.value = Some(val);
        }
        // Per spec §10.1.6.1 ToPropertyDescriptor: use Get(desc, key) which
        // invokes getters. For accessor properties, also check getters map.
        // Helper: read a descriptor boolean flag, invoking accessor getter if needed.
        let read_flag = |key: &str, has: &mut bool, flag: &mut bool| {
            if let Ok(val) = crate::eval::member::eval_object_member_value(
                desc_obj,
                &Value::String(key.to_string()),
                None,
            ) {
                if !matches!(val, Value::Undefined) {
                    *has = true;
                    *flag = to_bool(&val);
                }
            }
        };
        read_flag("writable", &mut has_writable, &mut flags.writable);
        read_flag("enumerable", &mut has_enumerable, &mut flags.enumerable);
        read_flag(
            "configurable",
            &mut has_configurable,
            &mut flags.configurable,
        );
        if !matches!(inherited_writable, Value::Undefined) {
            has_writable = true;
            flags.writable = to_bool(&inherited_writable);
        }
        if getter.is_none() && !matches!(descriptor_get, Value::Undefined) {
            getter = Some(descriptor_get.clone());
        }
        if setter.is_none() && !matches!(descriptor_set, Value::Undefined) {
            setter = Some(descriptor_set.clone());
        }
        if source_set_only {
            setter = None;
        }
        if let Some(value) = desc_borrowed.get_own("configurable") {
            has_configurable = true;
            flags.configurable = to_bool(&value);
        } else if !matches!(inherited_configurable, Value::Undefined) {
            has_configurable = true;
            flags.configurable = to_bool(&inherited_configurable);
        }
        // Per ES §10.1.6.1 ToPropertyDescriptor: "get" in desc → accessor descriptor.
        if getter.is_none() && !matches!(descriptor_get, Value::Undefined) {
            if let Some(g) = desc_borrowed.get("get") {
                match &g {
                    Value::Function(f) => getter = Some(Value::Function(f.clone())),
                    Value::NativeFunction(_) | Value::NativeConstructor(_) => {
                        getter = Some(g.clone())
                    }
                    _ => {}
                }
            }
        }
        if setter.is_none() && !matches!(descriptor_set, Value::Undefined) {
            if let Some(s) = desc_borrowed.get("set") {
                match &s {
                    Value::Function(f) => setter = Some(Value::Function(f.clone())),
                    Value::NativeFunction(_) | Value::NativeConstructor(_) => {
                        setter = Some(s.clone())
                    }
                    _ => {}
                }
            }
        }
        // Fallback: check getters/setters maps for accessor properties
        // defined via object literal shorthand syntax ({ get foo() {} })
        if getter.is_none() {
            if let Some(g) = desc_borrowed.get_getter("get") {
                if let Some(f) = &g.func {
                    getter = Some(f.clone());
                } else if !g.body.is_empty() {
                    let closure = Rc::new(RefCell::new((*g.closure).borrow().clone()));
                    let func = Value::Function(ValueFunction::new(
                        None,
                        vec![],
                        (*g.body).clone(),
                        closure,
                        false,
                        false,
                    ));
                    getter = Some(func);
                }
            }
        }
        if setter.is_none() && !source_set_only {
            if let Some(s) = desc_borrowed.get_setter("set") {
                if let Some(f) = &s.func {
                    setter = Some(f.clone());
                } else if !s.body.is_empty() {
                    let closure = Rc::new(RefCell::new((*s.closure).borrow().clone()));
                    let func = Value::Function(ValueFunction::new(
                        None,
                        vec![s.param.clone()],
                        (*s.body).clone(),
                        closure,
                        false,
                        false,
                    ));
                    setter = Some(func);
                }
            }
        }
    }

    if let Value::Object(object) = &obj {
        if object.borrow().kind == ObjectKind::ModuleNamespace {
            return define_module_namespace_property(
                obj,
                &prop,
                flags,
                descriptor_has_value,
                has_writable,
                has_enumerable,
                has_configurable,
            );
        }
    }

    if let Value::Class(c) = &obj {
        let value = flags.value.clone().unwrap_or(Value::Undefined);
        c.set_static_property(&prop, value, &Rc::new(RefCell::new(Environment::new())))?;
        c.deleted_properties.borrow_mut().remove(&prop);
        return Ok(obj);
    }

    if getter
        .as_ref()
        .is_some_and(|value| !matches!(value, Value::Undefined) && !value.is_callable())
        || setter
            .as_ref()
            .is_some_and(|value| !matches!(value, Value::Undefined) && !value.is_callable())
    {
        return Err(JsError::from(
            "TypeError: getter and setter must be callable",
        ));
    }
    if (getter.is_some() || setter.is_some()) && (flags.value.is_some() || has_writable) {
        return Err(JsError::from(
            "TypeError: descriptor cannot mix data and accessor fields",
        ));
    }

    if let Value::Function(function) = &obj {
        if function.get_property(&prop).is_some() {
            let value_changed = descriptor_has_value
                && flags.value.as_ref().is_some_and(|value| {
                    !crate::value::same_value(
                        &function.get_property(&prop).unwrap_or(Value::Undefined),
                        value,
                    )
                });
            if has_configurable && flags.configurable
                || value_changed
                || (has_writable && flags.writable)
            {
                let (error, js_error) = crate::value::error::create_js_error_with_type(
                    "Cannot redefine non-configurable property",
                    "TypeError",
                );
                crate::value::error::set_thrown_value(error);
                return Err(js_error);
            }
        }
        if let Some(getter) = getter {
            function.define_accessor(&prop, Some(getter), setter);
        } else if setter.is_some() {
            function.define_accessor(&prop, None, setter);
        } else if let Some(value) = flags.value {
            // Per ES §15.4.5: SetFunctionName — if value is an anonymous
            // non-arrow function, name it after the property key.
            let value = if let Value::Function(mut f) = value {
                if f.name.is_none() && !f.is_arrow {
                    f.name = Some(prop.clone());
                    let _ = f.set_property("name", Value::String(prop.clone()));
                }
                Value::Function(f)
            } else {
                value
            };
            function.set_property(&prop, value)?;
        }
        if !flags.writable {
            function.mark_nonwritable(&prop);
        }
        return Ok(obj);
    }

    if let Value::NativeConstructor(nc) = &obj {
        if let Some(value) = flags.value {
            nc.set_static_method(&prop, value);
        }
        return Ok(obj);
    }

    if prop == "length" {
        if let Value::Object(o) = &obj {
            if o.borrow().kind == ObjectKind::Array {
                if let Some(value) = flags.value.take() {
                    let primitive = crate::value::to_primitive(&value, None)?;
                    if matches!(primitive, Value::Object(_)) {
                        let (error, js_error) = crate::value::error::create_js_error_with_type(
                            "Cannot convert object to primitive value",
                            "TypeError",
                        );
                        crate::value::error::set_thrown_value(error);
                        return Err(js_error);
                    }
                    let number = crate::value::to_number(&primitive);
                    if !number.is_finite()
                        || !(0.0..=4_294_967_295.0).contains(&number)
                        || number.fract() != 0.0
                    {
                        let (error, js_error) = crate::value::error::create_js_error_with_type(
                            "Invalid array length",
                            "RangeError",
                        );
                        crate::value::error::set_thrown_value(error);
                        return Err(js_error);
                    }
                    flags.value = Some(Value::Number(if number == 0.0 { 0.0 } else { number }));
                }
            }
        }
    }

    if descriptor_has_value && !descriptor_has_get && !descriptor_has_set {
        getter = None;
        setter = None;
    }
    if let Value::Object(o) = &obj {
        let mapped_non_configurable = as_array_index(&prop).is_some()
            && matches!(
                o.borrow().data,
                crate::value::object::helpers::ObjData::Args { .. }
            )
            && o.borrow()
                .get_descriptor(&prop)
                .is_some_and(|existing| !existing.configurable);
        let descriptor_requests_configurable = match desc {
            Value::Object(descriptor) => descriptor
                .borrow()
                .get("configurable")
                .is_some_and(|value| to_bool(&value)),
            _ => false,
        };
        if mapped_non_configurable && descriptor_requests_configurable {
            let (error, js_error) = crate::value::error::create_js_error_with_type(
                "Cannot redefine non-configurable property",
                "TypeError",
            );
            crate::value::error::set_thrown_value(error);
            return Err(js_error);
        }
        let accessor_redefinition = o.borrow().get_descriptor(&prop).is_some_and(|existing| {
            let mapped = matches!(&o.borrow().data, crate::value::object::helpers::ObjData::Args { mapped } if as_array_index(&prop).is_some_and(|idx| mapped.contains_key(&(idx as u32))));
            !mapped && !existing.configurable && (o.borrow().has_getter(&prop) || o.borrow().has_setter(&prop))
        });
        let mapped_property = matches!(
            &o.borrow().data,
            crate::value::object::helpers::ObjData::Args { mapped }
                if as_array_index(&prop)
                    .is_some_and(|idx| mapped.contains_key(&(idx as u32)))
        );
        let data_to_accessor = o.borrow().get_descriptor(&prop).is_some_and(|existing| {
            !existing.configurable
                && (getter.is_some() || setter.is_some())
                && !o.borrow().has_getter(&prop)
                && !o.borrow().has_setter(&prop)
        });
        let setter_changed = o.borrow().get_descriptor(&prop).is_some_and(|existing| {
            !existing.configurable
                && descriptor_has_set
                && !crate::value::same_value(
                    &o.borrow()
                        .get_setter(&prop)
                        .and_then(|setter| setter.func.clone())
                        .unwrap_or(Value::Undefined),
                    &setter.clone().unwrap_or(Value::Undefined),
                )
        });
        let getter_changed = o.borrow().get_descriptor(&prop).is_some_and(|existing| {
            !existing.configurable
                && descriptor_has_get
                && !crate::value::same_value(
                    &o.borrow()
                        .get_getter(&prop)
                        .and_then(|getter| getter.func.clone())
                        .unwrap_or(Value::Undefined),
                    &getter.clone().unwrap_or(Value::Undefined),
                )
        });
        if !mapped_property
            && ((accessor_redefinition && (flags.value.is_some() || has_writable))
                || data_to_accessor
                || setter_changed
                || getter_changed)
        {
            let (error, js_error) = crate::value::error::create_js_error_with_type(
                "Cannot redefine non-configurable accessor property",
                "TypeError",
            );
            crate::value::error::set_thrown_value(error);
            return Err(js_error);
        }
        let existing_non_configurable = o
            .borrow()
            .descriptors
            .get(&prop)
            .is_some_and(|existing| !existing.configurable);
        let requests_configurable = match desc {
            Value::Object(descriptor) => descriptor
                .borrow()
                .get("configurable")
                .is_some_and(|value| to_bool(&value)),
            _ => false,
        };
        if existing_non_configurable && requests_configurable {
            let (error, js_error) = crate::value::error::create_js_error_with_type(
                "Cannot redefine non-configurable property",
                "TypeError",
            );
            crate::value::error::set_thrown_value(error);
            return Err(js_error);
        }
    }

    if getter.is_none() && setter.is_none() && !has_writable && flags.value.is_none() {
        if let Value::Object(o) = &obj {
            let mut object = o.borrow_mut();
            let existing = object.get_descriptor(&prop);
            let mapped_index = matches!(
                &object.data,
                crate::value::object::helpers::ObjData::Args { mapped }
                    if as_array_index(&prop)
                        .is_some_and(|idx| mapped.contains_key(&(idx as u32)))
            );
            let existing_getter = if descriptor_has_get {
                None
            } else if mapped_index {
                None
            } else {
                object
                    .get_getter(&prop)
                    .and_then(|getter| getter.func.clone())
            };
            let existing_setter = if descriptor_has_set {
                None
            } else if mapped_index {
                None
            } else {
                object
                    .get_setter(&prop)
                    .and_then(|setter| setter.func.clone())
            };
            if let Some(existing) = existing {
                if existing_setter.is_some() || existing_getter.is_some() {
                    if has_enumerable
                        && flags.enumerable != existing.enumerable
                        && !existing.configurable
                    {
                        let (error, js_error) = crate::value::error::create_js_error_with_type(
                            "Cannot redefine non-configurable property",
                            "TypeError",
                        );
                        crate::value::error::set_thrown_value(error);
                        return Err(js_error);
                    }
                    if !has_enumerable {
                        flags.enumerable = existing.enumerable;
                    }
                    if !has_configurable {
                        flags.configurable = existing.configurable;
                    }
                    object.define_accessor(&prop, existing_getter, existing_setter, flags);
                    drop(object);
                    return Ok(obj);
                }
            }
        }
    }

    if !descriptor_has_value {
        if let Value::Object(o) = &obj {
            let object = o.borrow();
            let mapped_index = matches!(
                &object.data,
                crate::value::object::helpers::ObjData::Args { mapped }
                    if as_array_index(&prop)
                        .is_some_and(|idx| mapped.contains_key(&(idx as u32)))
            );
            if (object.has_getter(&prop) || object.has_setter(&prop)) && !mapped_index {
                if !descriptor_has_get {
                    getter = object
                        .get_getter(&prop)
                        .and_then(|getter| getter.func.clone());
                }
                if !descriptor_has_set {
                    setter = object
                        .get_setter(&prop)
                        .and_then(|setter| setter.func.clone());
                }
            }
        }
    }

    if getter.is_some() || setter.is_some() {
        if prop == "length" {
            if let Value::Object(o) = &obj {
                if o.borrow().kind == ObjectKind::Array {
                    let (error, js_error) = crate::value::error::create_js_error_with_type(
                        "Array length must be a data property",
                        "TypeError",
                    );
                    crate::value::error::set_thrown_value(error);
                    return Err(js_error);
                }
            }
        }
        // Accessor descriptor: store the get/set functions themselves so
        // invocation and getOwnPropertyDescriptor see the same values.
        if let Value::Object(o) = &obj {
            let mut obj = o.borrow_mut();
            // Per ES §9.4.2.1 (Array Exotic Objects): if defining an array index
            // >= length, update length. For ordinary objects this does not apply.
            if obj.kind == ObjectKind::Array {
                if let Some(idx) = as_array_index(&prop) {
                    let current_len = obj
                        .get("length")
                        .and_then(|v| {
                            if let Value::Number(n) = v {
                                Some(n as usize)
                            } else {
                                None
                            }
                        })
                        .unwrap_or(0);
                    if idx + 1 > current_len {
                        obj.set("length", Value::Number((idx + 1) as f64));
                    }
                }
            }
            if obj.has_own(&prop) {
                if let Some(existing) = obj.get_descriptor(&prop) {
                    if !has_writable {
                        flags.writable = existing.writable;
                    }
                    if !has_enumerable {
                        flags.enumerable = existing.enumerable;
                    }
                    if !has_configurable {
                        flags.configurable = existing.configurable;
                    }
                }
            }
            if let Some(idx) = as_array_index(&prop).or_else(|| {
                prop.parse::<usize>()
                    .ok()
                    .filter(|index| *index < 4_294_967_295)
            }) {
                if let crate::value::object::helpers::ObjData::Args { mapped } = &mut obj.data {
                    mapped.remove(&(idx as u32));
                    obj.holes.insert(idx);
                    obj.properties.shift_remove(&prop);
                    obj.getters.shift_remove(&prop);
                    obj.setters.shift_remove(&prop);
                }
            }
            if descriptor_has_get
                && !descriptor_has_set
                && matches!(
                    obj.data,
                    crate::value::object::helpers::ObjData::Args { .. }
                )
            {
                obj.setters.shift_remove(&prop);
                setter = None;
            }
            if (setter_accessor || descriptor_has_set)
                && !descriptor_has_get
                && matches!(
                    obj.data,
                    crate::value::object::helpers::ObjData::Args { .. }
                )
            {
                obj.getters.shift_remove(&prop);
                getter = None;
            }
            obj.define_accessor(&prop, getter, setter, flags);
        } else if let Value::NativeConstructor(nc) = &obj {
            // Object.defineProperty on a native constructor (e.g., Promise)
            nc.define_accessor(&prop, getter, setter);
        } else if let Value::NativeFunction(nf) = &obj {
            // Object.defineProperty on a native function (e.g., bound function)
            nf.define_accessor(&prop, getter, setter);
        }
        return Ok(obj);
    }

    if let Value::Object(o) = &obj {
        if o.borrow().get_descriptor(&prop).is_none() && !o.borrow().extensible {
            let (error, js_error) = crate::value::error::create_js_error_with_type(
                "Cannot add property to non-extensible object",
                "TypeError",
            );
            crate::value::error::set_thrown_value(error);
            return Err(js_error);
        }
        if o.borrow().kind == ObjectKind::Array {
            let index = as_array_index(&prop).or_else(|| {
                prop.parse::<usize>()
                    .ok()
                    .filter(|index| *index < 4_294_967_295)
            });
            if let Some(index) = index {
                let current_length = o
                    .borrow()
                    .get("length")
                    .and_then(|value| match value {
                        Value::Number(length) => Some(length as usize),
                        _ => None,
                    })
                    .unwrap_or(0);
                if index >= current_length
                    && o.borrow()
                        .get_descriptor("length")
                        .is_some_and(|flags| !flags.writable)
                {
                    let (error, js_error) = crate::value::error::create_js_error_with_type(
                        "Cannot extend array with non-writable length",
                        "TypeError",
                    );
                    crate::value::error::set_thrown_value(error);
                    return Err(js_error);
                }
            }
        }
        if prop == "length"
            && o.borrow().kind == ObjectKind::Array
            && descriptor_has_value
            && matches!(flags.value, Some(Value::Undefined))
        {
            let (error, js_error) = crate::value::error::create_js_error_with_type(
                "Invalid array length",
                "RangeError",
            );
            crate::value::error::set_thrown_value(error);
            return Err(js_error);
        }
        if let Some(existing) = o.borrow().get_descriptor(&prop) {
            if !existing.configurable
                && ((has_configurable
                    && flags.configurable
                    && !(prop == "length" && o.borrow().kind == ObjectKind::Array))
                    || (has_enumerable && flags.enumerable != existing.enumerable))
            {
                let (error, js_error) = crate::value::error::create_js_error_with_type(
                    "Cannot redefine non-configurable property",
                    "TypeError",
                );
                crate::value::error::set_thrown_value(error);
                return Err(js_error);
            }
        }
        if prop == "length" && o.borrow().kind == ObjectKind::Array {
            if let Some(Value::Number(next_length)) = flags.value.as_ref() {
                let current_length = o
                    .borrow()
                    .get("length")
                    .and_then(|value| match value {
                        Value::Number(length) => Some(length as usize),
                        _ => None,
                    })
                    .unwrap_or(0);
                let next_length = *next_length as usize;
                if next_length < current_length {
                    let blocked = (next_length..current_length).find(|index| {
                        o.borrow()
                            .get_descriptor(&index.to_string())
                            .is_some_and(|descriptor| !descriptor.configurable)
                    });
                    if let Some(blocked) = blocked {
                        let partial_length = blocked + 1;
                        let mut object = o.borrow_mut();
                        object.elements.truncate(partial_length);
                        object.holes.retain(|index| *index < partial_length);
                        object.properties.retain(|key, _| {
                            key.parse::<usize>()
                                .map(|index| index < partial_length || key == "length")
                                .unwrap_or(true)
                        });
                        object
                            .properties
                            .insert("length".to_string(), Value::Number(partial_length as f64));
                        if let Some(length_flags) = object.descriptors.get_mut("length") {
                            length_flags.value = Some(Value::Number(partial_length as f64));
                            if has_writable && !flags.writable {
                                length_flags.writable = false;
                            }
                        }
                        drop(object);
                        let (error, js_error) = crate::value::error::create_js_error_with_type(
                            "Cannot delete non-configurable array element",
                            "TypeError",
                        );
                        crate::value::error::set_thrown_value(error);
                        return Err(js_error);
                    }
                }
            }
        }
        if let Some(value) = flags.value.clone() {
            if matches!(
                o.borrow().data,
                crate::value::object::helpers::ObjData::Args { .. }
            ) {
                let writable = o
                    .borrow()
                    .get_descriptor(&prop)
                    .map(|flags| flags.writable)
                    .unwrap_or(true);
                if writable {
                    if let Some(setter) = o.borrow().get_setter(&prop).cloned() {
                        let env = Rc::new(RefCell::new(crate::env::Environment::new()));
                        crate::eval::object::call_setter(o, &setter, value, &env)?;
                    }
                }
            }
        }
        let mapped_value = if flags.value.is_none() {
            let frozen_value = as_array_index(&prop).and_then(|idx| {
                let borrowed = o.borrow();
                borrowed
                    .get_descriptor(&prop)
                    .filter(|flags| !flags.writable)
                    .and_then(|_| {
                        borrowed
                            .properties
                            .get(&prop)
                            .cloned()
                            .or_else(|| borrowed.elements.get(idx).cloned())
                    })
            });
            if frozen_value.is_some() {
                frozen_value
            } else {
                o.borrow()
                    .get_getter(&prop)
                    .and_then(|getter| getter.func.clone())
                    .and_then(|getter| {
                        crate::eval::function::call_value_with_this(
                            getter,
                            vec![],
                            Value::Object(Rc::clone(o)),
                        )
                        .ok()
                    })
            }
        } else {
            None
        };
        let mut obj = o.borrow_mut();
        let value = flags
            .value
            .clone()
            .or(mapped_value)
            .or_else(|| obj.get_own_value(&prop))
            .unwrap_or(Value::Undefined);
        // Per ES §15.4.5: SetFunctionName — if value is a non-arrow
        // function, name it after the property key. We always rename (not
        // just when f.name.is_none()) so patterns like:
        //   Object.defineProperty(o, "y", { value: function(){} })
        // produce the spec-compliant name (here: "y", not "value" which
        // is the descriptor's "value" key).
        let value = if let Value::Function(mut f) = value {
            if !f.is_arrow {
                f.name = Some(prop.clone());
                let _ = f.set_property("name", Value::String(prop.clone()));
            }
            Value::Function(f)
        } else {
            value
        };
        // Per ES §9.4.2.1 (Array Exotic Objects): if defining an array index
        // >= length, update length. For ordinary objects this does not apply.
        if obj.kind == ObjectKind::Array {
            if let Some(idx) = as_array_index(&prop).or_else(|| {
                prop.parse::<usize>()
                    .ok()
                    .filter(|index| *index < 4_294_967_295)
            }) {
                let current_len = obj
                    .get("length")
                    .and_then(|v| {
                        if let Value::Number(n) = v {
                            Some(n as usize)
                        } else {
                            None
                        }
                    })
                    .unwrap_or(0);
                if idx + 1 > current_len {
                    let length = Value::Number((idx + 1) as f64);
                    if idx >= crate::value::object::helpers::MAX_ARRAY_ELEMENTS {
                        obj.properties.insert("length".to_string(), length);
                    } else {
                        obj.set("length", length);
                    }
                }
            }
        }
        if obj.get_descriptor(&prop).is_some() {
            if let Some(existing) = obj.get_descriptor(&prop) {
                if !descriptor_has_writable {
                    flags.writable = existing.writable;
                }
                if !has_writable {
                    flags.writable = existing.writable;
                }
                if !has_enumerable {
                    flags.enumerable = existing.enumerable;
                }
                if !has_configurable {
                    flags.configurable = existing.configurable;
                }
            }
        }
        let removes_mapping =
            !flags.configurable && !flags.writable && (has_writable || has_configurable);
        let mapped_index = matches!(
            &obj.data,
            crate::value::object::helpers::ObjData::Args { mapped }
                if as_array_index(&prop)
                    .is_some_and(|idx| mapped.contains_key(&(idx as u32)))
        );
        if let Some(existing) = obj.get_descriptor(&prop) {
            let value_changed = descriptor_has_value
                && flags.value.as_ref().is_some_and(|next| {
                    !crate::value::same_value(
                        existing.value.as_ref().unwrap_or(&Value::Undefined),
                        next,
                    )
                });
            let length_value_change =
                prop == "length" && obj.kind == ObjectKind::Array && existing.writable;
            if !existing.configurable
                && ((value_changed && !length_value_change && !mapped_index)
                    || (has_writable && flags.writable && !existing.writable))
            {
                let (error, err) = crate::value::error::create_js_error_with_type(
                    "Cannot redefine non-configurable property",
                    "TypeError",
                );
                crate::value::error::set_thrown_value(error);
                return Err(err);
            }
        }
        if prop == "length" && obj.kind == ObjectKind::Array {
            if let Some(Value::Number(next_length)) = flags.value.as_ref() {
                let next_length = *next_length as usize;
                obj.elements.truncate(next_length);
                obj.holes.retain(|index| *index < next_length);
                obj.properties.retain(|key, _| {
                    key.parse::<usize>()
                        .map(|index| index < next_length || key == "length")
                        .unwrap_or(true)
                });
                obj.descriptors.retain(|key, _| {
                    key.parse::<usize>()
                        .map(|index| index < next_length || key == "length")
                        .unwrap_or(true)
                });
                obj.getters.retain(|key, _| {
                    key.parse::<usize>()
                        .map(|index| index < next_length)
                        .unwrap_or(true)
                });
                obj.setters.retain(|key, _| {
                    key.parse::<usize>()
                        .map(|index| index < next_length)
                        .unwrap_or(true)
                });
            }
        }
        if prop == "length"
            && matches!(
                obj.data,
                crate::value::object::helpers::ObjData::Args { .. }
            )
        {
            if let Some(Value::Number(next_length)) = flags.value.as_ref() {
                let next_length = *next_length as usize;
                if next_length > obj.elements.len() {
                    obj.elements.resize(next_length, Value::Undefined);
                }
            }
        }
        let drop_mapping = mapped_index && descriptor_has_value && !flags.writable;
        if drop_mapping {
            if let Some(idx) = as_array_index(&prop) {
                if let crate::value::object::helpers::ObjData::Args { mapped } = &mut obj.data {
                    mapped.remove(&(idx as u32));
                    obj.getters.shift_remove(&prop);
                    obj.setters.shift_remove(&prop);
                }
            }
        }
        obj.define(&prop, value, flags);
        if removes_mapping
            || (mapped_index
                && descriptor_has_value
                && !obj
                    .get_descriptor(&prop)
                    .is_some_and(|property| property.writable))
        {
            if let Some(idx) = as_array_index(&prop) {
                if let crate::value::object::helpers::ObjData::Args { mapped } = &mut obj.data {
                    mapped.remove(&(idx as u32));
                    obj.getters.shift_remove(&prop);
                    obj.setters.shift_remove(&prop);
                }
            }
        }
    }
    Ok(obj)
}

fn define_module_namespace_property(
    object: Value,
    prop: &str,
    flags: PropertyFlags,
    has_value: bool,
    has_writable: bool,
    has_enumerable: bool,
    has_configurable: bool,
) -> Result<Value, JsError> {
    let Value::Object(namespace) = &object else {
        unreachable!();
    };
    let namespace = namespace.borrow();
    if !namespace.has_own(prop) && !namespace.symbol_properties.contains_key(prop) {
        return reject_namespace_property();
    }
    let symbol = prop.contains('\0')
        || prop == "Symbol.toStringTag"
        || namespace.symbol_properties.contains_key(prop);
    let current_value = namespace
        .get_own_value(prop)
        .or_else(|| namespace.symbol_properties.get(prop).cloned());
    let same_value = !has_value
        || flags.value.as_ref().is_some_and(|value| {
            current_value
                .as_ref()
                .is_some_and(|current| crate::value::same_value(current, value))
        });
    let writable = !has_writable || flags.writable == !symbol;
    let enumerable = !has_enumerable || flags.enumerable == !symbol;
    let configurable = !has_configurable || !flags.configurable;
    let allowed = same_value && writable && enumerable && configurable;
    drop(namespace);
    if allowed {
        Ok(object)
    } else {
        reject_namespace_property()
    }
}

fn reject_namespace_property() -> Result<Value, JsError> {
    let (error, js_error) = crate::value::error::create_js_error_with_type(
        "Cannot redefine module namespace property",
        "TypeError",
    );
    crate::value::error::set_thrown_value(error);
    Err(js_error)
}

/// Object.getOwnPropertyDescriptor(obj, prop) - gets property descriptor
pub fn object_get_own_property_descriptor(args: Vec<Value>) -> Result<Value, JsError> {
    let obj = args
        .first()
        .ok_or_else(|| JsError::from("Object.getOwnPropertyDescriptor requires argument"))?;
    let prop = args
        .get(1)
        .map(to_property_key)
        .unwrap_or(Ok("".to_string()))?;

    if let Value::Object(o) = obj {
        crate::eval::member::trigger_deferred_namespace(o, &prop)?;
        return get_object_property_descriptor(o, &prop);
    } else if let Value::Function(ref f) = obj {
        return get_function_property_descriptor(f, &prop);
    } else if let Value::NativeFunction(ref nf) = obj {
        return get_native_function_property_descriptor(nf.as_ref(), &prop);
    } else if let Value::NativeConstructor(nc) = obj {
        return get_native_constructor_property_descriptor(nc, &prop);
    } else if let Value::Class(c) = obj {
        return get_class_property_descriptor(c, &prop);
    }
    Ok(Value::Undefined)
}

/// Get property descriptor from an Object value.
pub fn get_object_property_descriptor(
    o: &Rc<RefCell<Object>>,
    prop: &str,
) -> Result<Value, JsError> {
    if o.borrow().kind == ObjectKind::ModuleNamespace {
        if !o.borrow().has_own(prop) {
            return Ok(Value::Undefined);
        }
        let getter = o.borrow().get_getter(prop).and_then(|g| g.func.clone());
        let value = if let Some(getter) = getter {
            crate::eval::function::call_value_with_this(
                getter,
                vec![],
                Value::Object(Rc::clone(o)),
            )?
        } else {
            let object = o.borrow();
            object
                .get_own_value(prop)
                .ok_or_else(|| JsError::new("property not found"))?
        };
        let symbol_tag = prop.contains('\0') && prop.contains("toStringTag");
        return Ok(make_descriptor_value(
            PropertyFlags {
                value: Some(value.clone()),
                writable: !symbol_tag,
                enumerable: !symbol_tag,
                configurable: false,
            },
            value,
        ));
    }
    let obj = o.borrow();

    if prop == "length" && obj.kind == ObjectKind::Array {
        let writable = obj
            .get_descriptor(prop)
            .map(|flags| flags.writable)
            .unwrap_or(true);
        let value = obj
            .get("length")
            .unwrap_or(Value::Number(obj.elements.len() as f64));
        return Ok(make_descriptor_value(
            PropertyFlags {
                value: Some(value.clone()),
                writable,
                enumerable: false,
                configurable: false,
            },
            value,
        ));
    }

    if prop.contains('\0') {
        if let Some(value) = obj.symbol_properties.get(prop) {
            let flags = obj.get_descriptor(prop).unwrap_or_else(|| PropertyFlags {
                value: Some(value.clone()),
                writable: true,
                enumerable: true,
                configurable: true,
            });
            return Ok(make_descriptor_value(flags, value.clone()));
        }
    }

    if let Some(idx) = as_array_index(prop) {
        if let crate::value::object::helpers::ObjData::Args { mapped } = &obj.data {
            if mapped.contains_key(&(idx as u32)) {
                let mut flags = obj.get_descriptor(prop).unwrap_or(PropertyFlags {
                    writable: true,
                    enumerable: true,
                    configurable: true,
                    value: None,
                });
                let getter = obj.get_getter(prop).and_then(|getter| getter.func.clone());
                let value = getter
                    .and_then(|getter| {
                        crate::eval::function::call_value_with_this(
                            getter,
                            vec![],
                            Value::Object(Rc::clone(o)),
                        )
                        .ok()
                    })
                    .or_else(|| obj.get(prop))
                    .unwrap_or(Value::Undefined);
                flags.value = None;
                return Ok(make_descriptor_value(flags, value));
            }
        }
    }

    // Accessor property (get/set installed via defineProperty or object literal)
    if obj.has_getter(prop) || obj.has_setter(prop) {
        let flags = obj.get_descriptor(prop).unwrap_or(PropertyFlags {
            value: None,
            writable: false,
            enumerable: true,
            configurable: true,
        });
        // Return cached func (from set_getter/set_getter_func/set_setter/set_setter_func).
        // set_getter always sets func, preserving function identity for getOwnPropertyDescriptor.
        let get_val = if let Some(g) = obj.get_getter(prop) {
            g.func.clone().unwrap_or(Value::Undefined)
        } else {
            Value::Undefined
        };
        let set_val = if let Some(s) = obj.get_setter(prop) {
            s.func.clone().unwrap_or(Value::Undefined)
        } else {
            Value::Undefined
        };
        let mut desc = new_ordinary_with_object_proto();
        // Per ES FromPropertyDescriptor, get/set keys are always present,
        // set to undefined when no getter/setter exists on the accessor.
        desc.set("get", get_val);
        desc.set("set", set_val);
        desc.set("enumerable", Value::Boolean(flags.enumerable));
        desc.set("configurable", Value::Boolean(flags.configurable));
        return Ok(Value::Object(Rc::new(RefCell::new(desc))));
    }

    let has_property = obj.properties.contains_key(prop)
        || prop
            .parse::<usize>()
            .map(|index| index < obj.elements.len() && !obj.holes.contains(&index))
            .unwrap_or(false);

    if !has_property {
        return Ok(Value::Undefined);
    }
    let value = obj.get(prop).unwrap_or(Value::Undefined);
    let flags = obj.get_descriptor(prop).unwrap_or_else(|| PropertyFlags {
        value: Some(value.clone()),
        writable: true,
        enumerable: true,
        configurable: true,
    });
    Ok(make_descriptor_value(flags, value))
}

/// Get property descriptor from a Function value.
pub fn get_function_property_descriptor(
    f: &crate::value::ValueFunction,
    prop: &str,
) -> Result<Value, JsError> {
    if f.is_property_deleted(prop) {
        return Ok(Value::Undefined);
    }
    if prop == "name" {
        let value = f
            .get_property("name")
            .map(|v| match v {
                Value::String(s) => s,
                _ => String::new(),
            })
            .unwrap_or_else(|| f.name.clone().unwrap_or_default());
        // Per ES §9.2.4 FunctionInitialize, `name` is configurable: true.
        return make_property_descriptor_string(&value, false, false, true);
    }
    if prop == "length" {
        let len = f
            .get_property("length")
            .and_then(|v| match v {
                Value::Number(n) => Some(n),
                _ => None,
            })
            .unwrap_or_else(|| crate::value::function::expected_argument_count(&f.params));
        // Per ES §9.2.4 FunctionInitialize, the `length` property is
        // { [[Value]]: len, [[Writable]]: false, [[Enumerable]]: false,
        // [[Configurable]]: true }.
        return make_property_descriptor_number(len, false, false, true);
    }
    if prop == "prototype" {
        let proto = Value::Object(f.get_prototype());
        let configurable = false;
        return Ok(make_descriptor_value(
            PropertyFlags {
                value: Some(proto.clone()),
                writable: true,
                enumerable: false,
                configurable,
            },
            proto,
        ));
    }
    if let Some(value) = f.get_property(prop) {
        return Ok(make_descriptor_value(
            PropertyFlags {
                value: Some(value.clone()),
                writable: false,
                enumerable: false,
                configurable: false,
            },
            value,
        ));
    }
    Ok(Value::Undefined)
}

/// Get property descriptor from a NativeFunction value.
pub fn get_native_function_property_descriptor(
    nf: &crate::value::NativeFunction,
    prop: &str,
) -> Result<Value, JsError> {
    // Check for special properties before custom properties
    if prop == "name" {
        if nf.get_property("\0deleted:name").is_some() {
            return Ok(Value::Undefined);
        }
        let name = nf
            .get_property("name")
            .and_then(|value| match value {
                Value::String(name) => Some(name),
                _ => None,
            })
            .unwrap_or_else(|| nf.name.clone());
        let flags = nf.get_property_flags("name").unwrap_or(PropertyFlags {
            value: Some(Value::String(name.clone())),
            writable: false,
            enumerable: false,
            configurable: true,
        });
        return Ok(make_descriptor_value(flags, Value::Undefined));
    }
    if prop == "length" {
        if nf.get_property("\0deleted:length").is_some() {
            return Ok(Value::Undefined);
        }
        let length = nf
            .get_property("length")
            .and_then(|value| match value {
                Value::Number(length) => Some(length),
                _ => None,
            })
            .unwrap_or_else(|| if nf.name == "create" { 2.0 } else { 0.0 });
        let flags = nf.get_property_flags("length").unwrap_or(PropertyFlags {
            value: Some(Value::Number(length)),
            writable: false,
            enumerable: false,
            configurable: true,
        });
        return Ok(make_descriptor_value(flags, Value::Undefined));
    }
    if prop == "prototype" {
        let value = nf
            .get_property(prop)
            .or_else(|| nf.prototype.borrow().clone().map(Value::Object))
            .unwrap_or(Value::Undefined);
        let flags = nf.get_property_flags(prop).unwrap_or(PropertyFlags {
            value: Some(value.clone()),
            writable: true,
            enumerable: false,
            configurable: false,
        });
        return Ok(make_descriptor_value(flags, Value::Undefined));
    }
    // Check for custom properties
    if let Some(value) = nf.get_property(prop) {
        return Ok(make_descriptor_value(
            PropertyFlags {
                value: Some(value),
                writable: true,
                enumerable: false,
                configurable: true,
            },
            Value::Undefined,
        ));
    }
    Ok(Value::Undefined)
}

/// Get property descriptor from a NativeConstructor value.
pub fn get_native_constructor_property_descriptor(
    nc: &crate::value::NativeConstructor,
    prop: &str,
) -> Result<Value, JsError> {
    if nc.is_property_deleted(prop) {
        return Ok(Value::Undefined);
    }
    // Check for custom static methods first
    if let Some(value) = nc.get_static_method(prop) {
        let immutable = nc.is_non_deletable_static_method(prop);
        let length = prop == "length" && nc.name() == "AggregateError";
        return Ok(make_descriptor_value(
            PropertyFlags {
                value: Some(value),
                writable: !immutable && !length,
                enumerable: false,
                configurable: !immutable,
            },
            Value::Undefined,
        ));
    }

    let is_function_constructor = crate::builtins::function::get_function_prototype()
        .map(|fp| std::rc::Rc::ptr_eq(&fp, &nc.prototype))
        .unwrap_or(false);

    if prop == "name" {
        let name = if is_function_constructor {
            "Function".to_string()
        } else {
            nc.name().to_string()
        };
        return make_property_descriptor_string(&name, false, false, true);
    }
    if prop == "length" {
        let len = if nc.name() == "SuppressedError" {
            3.0
        } else if nc.name() == "AggregateError" {
            2.0
        } else if is_function_constructor
            || matches!(
                nc.name().as_str(),
                "Object"
                    | "BigInt"
                    | "GeneratorFunction"
                    | "AsyncFunction"
                    | "AsyncGeneratorFunction"
                    | "Error"
                    | "EvalError"
                    | "RangeError"
                    | "ReferenceError"
                    | "SyntaxError"
                    | "TypeError"
                    | "URIError"
                    | "WeakRef"
                    | "FinalizationRegistry"
            )
        {
            1.0
        } else {
            0.0
        };
        return make_property_descriptor_number(len, false, false, true);
    }
    if prop == "prototype" {
        // Per ES §19.1.2.15: Object.prototype is { Writable: false, Enumerable: false,
        // Configurable: false }. Other constructors vary, but prototype is always
        // non-enumerable.
        return Ok(make_descriptor_value(
            PropertyFlags {
                value: Some(Value::Object(std::rc::Rc::clone(&nc.prototype))),
                writable: false,
                enumerable: false,
                configurable: false,
            },
            Value::Undefined,
        ));
    }
    Ok(Value::Undefined)
}

/// Create an ordinary Object with Object.prototype as its [[Prototype]].
/// Use this for all internal descriptor/result objects that need
/// inherited methods like hasOwnProperty, toString, etc.
fn new_ordinary_with_object_proto() -> Object {
    let mut obj = Object::new(ObjectKind::Ordinary);
    if let Some(proto) = crate::builtins::get_object_prototype() {
        obj.prototype = Some(proto);
    }
    obj
}

/// Create a property descriptor value object from flags and value.
pub fn make_descriptor_value(flags: PropertyFlags, value: Value) -> Value {
    let mut desc = new_ordinary_with_object_proto();
    desc.properties
        .insert("value".to_string(), flags.value.unwrap_or(value));
    desc.properties
        .insert("writable".to_string(), Value::Boolean(flags.writable));
    desc.properties
        .insert("enumerable".to_string(), Value::Boolean(flags.enumerable));
    desc.properties.insert(
        "configurable".to_string(),
        Value::Boolean(flags.configurable),
    );
    Value::Object(Rc::new(RefCell::new(desc)))
}

/// Create a string property descriptor object
pub fn make_property_descriptor_string(
    value: &str,
    writable: bool,
    enumerable: bool,
    configurable: bool,
) -> Result<Value, JsError> {
    let mut desc = new_ordinary_with_object_proto();
    desc.properties
        .insert("value".to_string(), Value::String(value.to_string()));
    desc.properties
        .insert("writable".to_string(), Value::Boolean(writable));
    desc.properties
        .insert("enumerable".to_string(), Value::Boolean(enumerable));
    desc.properties
        .insert("configurable".to_string(), Value::Boolean(configurable));
    Ok(Value::Object(Rc::new(RefCell::new(desc))))
}

/// Create a numeric property descriptor object
pub fn make_property_descriptor_number(
    value: f64,
    writable: bool,
    enumerable: bool,
    configurable: bool,
) -> Result<Value, JsError> {
    let mut desc = new_ordinary_with_object_proto();
    desc.properties
        .insert("value".to_string(), Value::Number(value));
    desc.properties
        .insert("writable".to_string(), Value::Boolean(writable));
    desc.properties
        .insert("enumerable".to_string(), Value::Boolean(enumerable));
    desc.properties
        .insert("configurable".to_string(), Value::Boolean(configurable));
    Ok(Value::Object(Rc::new(RefCell::new(desc))))
}

/// Get property descriptor from a Class value.
pub fn get_class_property_descriptor(
    c: &crate::value::ClassValue,
    prop: &str,
) -> Result<Value, JsError> {
    // If this configurable property was deleted, return undefined
    if c.deleted_properties.borrow().contains(prop) {
        return Ok(Value::Undefined);
    }
    if prop != "prototype" {
        let eval_env = c
            .get_class_def_env()
            .unwrap_or_else(|| Rc::new(RefCell::new(Environment::new())));

        if let Some(val) = c.get_static_field(prop) {
            let mut desc = new_ordinary_with_object_proto();
            desc.set("value", val);
            desc.set("writable", Value::Boolean(true));
            desc.set("enumerable", Value::Boolean(true));
            desc.set("configurable", Value::Boolean(true));
            return Ok(Value::Object(Rc::new(RefCell::new(desc))));
        }

        for (name, params, body, is_async, is_generator) in &c.static_methods {
            let matches = match name {
                PropertyKey::Ident(s) => s == prop,
                PropertyKey::String(s) => s == prop,
                PropertyKey::Number(n) => n.to_string() == prop,
                PropertyKey::Computed(_) => {
                    prop_key_to_string(name, &eval_env, false).is_ok_and(|k| k == prop)
                }
            };
            if matches {
                let fn_name = method_function_name(name, prop, &eval_env)?;
                let mut func = ValueFunction::new(
                    Some(fn_name),
                    params.clone(),
                    body.clone(),
                    Rc::clone(&eval_env),
                    *is_async,
                    *is_generator,
                );
                func.strict = true;
                func.is_method = true;
                let mut desc = new_ordinary_with_object_proto();
                desc.set("value", Value::Function(func));
                desc.set("writable", Value::Boolean(true));
                desc.set("enumerable", Value::Boolean(false));
                desc.set("configurable", Value::Boolean(true));
                return Ok(Value::Object(Rc::new(RefCell::new(desc))));
            }
        }

        let static_getter_info = c.static_getters.iter().find_map(|(k, body)| {
            prop_key_to_string(k, &eval_env, false)
                .ok()
                .filter(|k_str| k_str == prop)
                .map(|_| (k.clone(), body.clone()))
        });

        let static_setter_info = c.static_setters.iter().find_map(|(k, param, body)| {
            prop_key_to_string(k, &eval_env, false)
                .ok()
                .filter(|k_str| k_str == prop)
                .map(|_| (k.clone(), param.clone(), body.clone()))
        });

        if static_getter_info.is_some() || static_setter_info.is_some() {
            let mut desc = new_ordinary_with_object_proto();

            if let Some((key, body)) = static_getter_info {
                let fn_name = accessor_function_name(&key, prop, &eval_env, "get")?;
                let mut func = ValueFunction::new(
                    Some(fn_name),
                    vec![],
                    body,
                    Rc::clone(&eval_env),
                    false,
                    false,
                );
                func.strict = true;
                func.is_method = true;
                desc.set("get", Value::Function(func));
            }

            if let Some((key, param, body)) = static_setter_info {
                let fn_name = accessor_function_name(&key, prop, &eval_env, "set")?;
                let mut func = ValueFunction::new(
                    Some(fn_name),
                    vec![param.clone()],
                    body,
                    Rc::clone(&eval_env),
                    false,
                    false,
                );
                func.strict = true;
                func.is_method = true;
                desc.set("set", Value::Function(func));
            }

            desc.set("enumerable", Value::Boolean(false));
            desc.set("configurable", Value::Boolean(true));
            return Ok(Value::Object(Rc::new(RefCell::new(desc))));
        }
    }
    match prop {
        "length" => {
            make_property_descriptor_number(c.constructor_params.len() as f64, false, false, true)
        }
        "name" => {
            make_property_descriptor_string(&c.name.clone().unwrap_or_default(), false, false, true)
        }
        "prototype" => {
            let proto_val = c
                .prototype_cell
                .borrow()
                .as_ref()
                .map(|o| Value::Object(Rc::clone(o)))
                .unwrap_or(Value::Undefined);
            let mut desc = new_ordinary_with_object_proto();
            desc.properties.insert("value".to_string(), proto_val);
            desc.properties
                .insert("writable".to_string(), Value::Boolean(false));
            desc.properties
                .insert("enumerable".to_string(), Value::Boolean(false));
            desc.properties
                .insert("configurable".to_string(), Value::Boolean(false));
            Ok(Value::Object(Rc::new(RefCell::new(desc))))
        }
        _ => Ok(Value::Undefined),
    }
}

fn push_static_key(names: &mut Vec<String>, key: &str) {
    if key.starts_with('#') || key.contains('\0') || key == "name" {
        return;
    }
    if !names.iter().any(|k| k == key) {
        names.push(key.to_string());
    }
}

fn ordinary_key_order(names: Vec<String>) -> Vec<String> {
    let mut indices = Vec::new();
    let mut strings = Vec::new();
    for name in names {
        let is_index = name
            .parse::<u32>()
            .ok()
            .is_some_and(|index| index < u32::MAX && index.to_string() == name);
        if is_index {
            indices.push(name);
        } else {
            strings.push(name);
        }
    }
    indices.sort_by_key(|name| name.parse::<u32>().unwrap_or(u32::MAX));
    indices.extend(strings);
    indices
}

/// Own property names for a class constructor (includes non-enumerable builtins).
pub fn class_own_property_names(c: &crate::value::ClassValue) -> Vec<String> {
    let deleted = c.deleted_properties.borrow();
    let mut names = vec!["length".to_string()];
    if !deleted.contains("name") {
        names.push("name".to_string());
    }
    if !deleted.contains("prototype") {
        names.push("prototype".to_string());
    }
    let eval_env = c
        .get_class_def_env()
        .unwrap_or_else(|| Rc::new(RefCell::new(Environment::new())));
    for (key, _, _, _, _) in &c.static_methods {
        if let Ok(k) = prop_key_to_string(key, &eval_env, false) {
            push_static_key(&mut names, &k);
        }
    }
    for (key, _) in &c.static_getters {
        if let Ok(k) = prop_key_to_string(key, &eval_env, false) {
            push_static_key(&mut names, &k);
        }
    }
    for (key, _, _) in &c.static_setters {
        if let Ok(k) = prop_key_to_string(key, &eval_env, false) {
            push_static_key(&mut names, &k);
        }
    }
    for index in 0.. {
        let Some(key) = c.static_field_key(index) else {
            break;
        };
        if !deleted.contains(&key) {
            push_static_key(&mut names, &key);
        }
    }
    ordinary_key_order(names)
}

pub fn class_own_property_symbols(c: &crate::value::ClassValue) -> Vec<Value> {
    let eval_env = c
        .get_class_def_env()
        .unwrap_or_else(|| Rc::new(RefCell::new(Environment::new())));
    c.static_methods
        .iter()
        .map(|(key, _, _, _, _)| key)
        .chain(c.static_getters.iter().map(|(key, _)| key))
        .chain(c.static_setters.iter().map(|(key, _, _)| key))
        .filter_map(|key| prop_key_to_string(key, &eval_env, false).ok())
        .filter_map(|key| {
            let (desc, id) = key.split_once('\0')?;
            Some(Value::Symbol(Rc::new(crate::value::Symbol {
                desc: if desc.is_empty() {
                    None
                } else {
                    Some(Rc::from(desc))
                },
                global: false,
                id: id.parse().ok()?,
            })))
        })
        .collect()
}

pub fn function_own_property_names(f: &ValueFunction) -> Vec<String> {
    f.own_property_names()
}

pub fn native_function_own_property_names(nf: &crate::value::NativeFunction) -> Vec<String> {
    let mut names = Vec::new();
    if nf.get_property("\0deleted:length").is_none() {
        names.push("length".to_string());
    }
    if nf.get_property("\0deleted:name").is_none() {
        names.push("name".to_string());
    }
    if nf.get_property("prototype").is_some() || nf.prototype.borrow().is_some() {
        names.push("prototype".to_string());
    }
    names
}

pub fn native_constructor_own_property_names(nc: &crate::value::NativeConstructor) -> Vec<String> {
    let mut names = Vec::new();
    if !nc.is_property_deleted("length") {
        names.push("length".to_string());
    }
    names.push("name".to_string());
    names.push("prototype".to_string());
    names
}
