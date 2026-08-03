//! Array transformation methods (map, filter, reduce, forEach, flat, flatMap, some, every)

use std::cell::RefCell;
use std::rc::Rc;

use crate::eval::call_value_with_this;
use crate::value::{to_bool, to_number, JsError, Object, ObjectKind, Value};

// ============================================================================
// Helper functions for Array methods
// ============================================================================

/// Get the array elements from 'this'
/// Array methods are intentionally generic - they work on any array-like object
pub fn get_this_array() -> Result<Vec<Value>, JsError> {
    let receiver = crate::builtins::get_native_this()
        .ok_or_else(|| JsError("Array.prototype method called on non-object".to_string()))?;
    let receiver = match receiver {
        Value::Object(object) => Value::Object(object),
        primitive => crate::value::to_object(&primitive)?,
    };
    match receiver {
        Value::Function(function) => {
            let length = crate::value::try_to_number(&crate::eval::member::eval_function_member(
                &function, "length",
            )?)?
            .max(0.0) as usize;
            Ok((0..length)
                .map(|index| {
                    function
                        .get_property(&index.to_string())
                        .unwrap_or(Value::Undefined)
                })
                .collect())
        }
        Value::Object(o) => {
            let arr = o.borrow();
            if arr.kind == ObjectKind::Array {
                Ok(arr.elements.clone())
            } else if arr.exotic_kind == Some(crate::value::kind::ExoticKind::String) {
                match arr.get("_value") {
                    Some(Value::String(string)) => Ok(string
                        .chars()
                        .map(|character| Value::String(character.to_string()))
                        .collect()),
                    _ => Ok(Vec::new()),
                }
            } else {
                drop(arr);
                let mut elements = Vec::new();
                let mut i = 0u32;
                let length = crate::eval::member::eval_object_member_value(
                    &o,
                    &Value::String("length".to_string()),
                    None,
                )
                .ok()
                .and_then(|value| crate::value::try_to_number(&value).ok())
                .map(|length| length.max(0.0) as u32);
                while length.is_none_or(|limit| i < limit) {
                    let value = crate::eval::member::eval_object_member_value(
                        &o,
                        &Value::String(i.to_string()),
                        None,
                    )
                    .ok();
                    match value {
                        Some(value) => elements.push(value),
                        None if length.is_none() => break,
                        None => elements.push(Value::Undefined),
                    }
                    i += 1;
                }
                Ok(elements)
            }
        }
        _ => Err(JsError(
            "Array.prototype method called on non-object".to_string(),
        )),
    }
}

/// Create result array object from elements
pub fn make_array(elements: Vec<Value>) -> Value {
    let mut arr = Object::new_array_from(elements);
    // Set the prototype to the Array prototype so methods like filter work
    if let Some(proto) = crate::builtins::array::get_array_prototype() {
        arr.prototype = Some(proto);
    }
    Value::Object(Rc::new(RefCell::new(arr)))
}

fn make_filter_result(receiver: &Value, elements: Vec<Value>) -> Result<Value, JsError> {
    let Value::Object(receiver) = receiver else {
        return Ok(make_array(elements));
    };
    let Some(Value::Object(constructor)) = receiver.borrow().get("constructor") else {
        return Ok(make_array(elements));
    };
    let Some(Value::Symbol(species)) =
        crate::builtins::symbol::get_well_known_symbol_no_ctx("species")
    else {
        return Ok(make_array(elements));
    };
    let Some(Value::Function(function)) = constructor.borrow().get(&species.property_key()) else {
        return Ok(make_array(elements));
    };
    let prototype = crate::eval::class::get_constructor_prototype(&Value::Function(function.clone()))?
        .or_else(crate::builtins::get_object_prototype)
        .ok_or_else(|| JsError::new("Array species has no prototype"))?;
    let target = Value::Object(Rc::new(RefCell::new(Object::with_prototype(
        ObjectKind::Ordinary,
        prototype,
    ))));
    let result = call_value_with_this(
        Value::Function(function),
        vec![Value::Number(0.0)],
        target.clone(),
    )?;
    let result = match result {
        Value::Object(_) | Value::Function(_) | Value::NativeFunction(_) => result,
        _ => target,
    };
    if let Value::Object(object) = &result {
        if !object.borrow().extensible && !elements.is_empty() {
            return Err(JsError::new("TypeError: cannot define property"));
        }
        for (index, value) in elements.iter().enumerate() {
            if object
                .borrow()
                .get_descriptor(&index.to_string())
                .is_some_and(|flags| !flags.configurable)
            {
                return Err(JsError::new("TypeError: cannot redefine property"));
            }
            object.borrow_mut().set(&index.to_string(), value.clone());
        }
        object
            .borrow_mut()
            .set("length", Value::Number(elements.len() as f64));
    }
    Ok(result)
}

/// Call a callback function with standard arguments
pub fn call_callback(
    callback: &Value,
    elem: &Value,
    index: usize,
    elements: &[Value],
) -> Result<Value, JsError> {
    let array_copy = Value::Object(Rc::new(RefCell::new(Object::new_array_from(
        elements.to_vec(),
    ))));
    let callback_args = vec![elem.clone(), Value::Number(index as f64), array_copy];

    match callback {
        Value::Function(_) => {
            call_value_with_this(callback.clone(), callback_args, Value::Undefined)
        }
        Value::NativeFunction(nf) => nf.call(Value::Undefined, callback_args),
        _ => Err(JsError("Callback is not a function".to_string())),
    }
}

// ============================================================================
// Transformation method implementations
// ============================================================================

/// Array.prototype.map(callback, thisArg?)
pub fn proto_map(args: Vec<Value>) -> Result<Value, JsError> {
    let callback = args.first().cloned().unwrap_or(Value::Undefined);
    let this_arg = args.get(1).cloned().unwrap_or(Value::Undefined);
    let receiver = match crate::builtins::get_native_this()
        .ok_or_else(|| JsError("Array.prototype method called on non-object".to_string()))?
    {
        Value::Object(object) => Value::Object(object),
        primitive => crate::value::to_object(&primitive)?,
    };
    let length = array_like_length(&receiver)?;

    let mut result = Vec::new();
    for i in 0..length {
        let Some(elem) = array_like_value(&receiver, i)? else {
            result.push(Value::Undefined);
            continue;
        };
        let callback_args = vec![elem, Value::Number(i as f64), receiver.clone()];
        let mapped = match &callback {
            Value::Function(_) => {
                call_value_with_this(callback.clone(), callback_args, this_arg.clone())?
            }
            Value::NativeFunction(native) => native.call(this_arg.clone(), callback_args)?,
            _ => return Err(JsError("Callback is not a function".to_string())),
        };
        result.push(mapped);
    }
    Ok(make_array(result))
}

/// Array.prototype.filter(callback, thisArg?)
pub fn proto_filter(args: Vec<Value>) -> Result<Value, JsError> {
    let callback = args.first().cloned().unwrap_or(Value::Undefined);
    let receiver = match crate::builtins::get_native_this()
        .ok_or_else(|| JsError("Array.prototype method called on non-object".to_string()))?
    {
        Value::Object(object) => Value::Object(object),
        primitive => crate::value::to_object(&primitive)?,
    };
    let length = array_like_length(&receiver)?;

    let mut result = Vec::new();
    for i in 0..length {
        let Some(elem) = array_like_value(&receiver, i)? else {
            continue;
        };
        let callback_args = vec![elem.clone(), Value::Number(i as f64), receiver.clone()];
        let callback_result = match &callback {
            Value::Function(_) => {
                call_value_with_this(callback.clone(), callback_args, Value::Undefined)?
            }
            Value::NativeFunction(native) => native.call(Value::Undefined, callback_args)?,
            _ => return Err(JsError("Callback is not a function".to_string())),
        };
        let keep = to_bool(&callback_result);
        if keep {
            result.push(elem);
        }
    }
    make_filter_result(&receiver, result)
}

/// Array.prototype.reduce(callback, initialValue?)
pub fn proto_reduce(args: Vec<Value>) -> Result<Value, JsError> {
    let receiver = crate::builtins::get_native_this()
        .ok_or_else(|| JsError("Array.prototype method called on non-object".to_string()))?;
    let receiver = match receiver {
        Value::Object(object) => Value::Object(object),
        primitive => crate::value::to_object(&primitive)?,
    };
    let length = array_like_length(&receiver)?;
    let callback = args.first().cloned().unwrap_or(Value::Undefined);
    let initial = args.get(1).cloned();

    let (mut accumulator, mut index) = match initial {
        Some(value) => (value, 0),
        None => {
            let mut first = None;
            let mut position = 0;
            while position < length && first.is_none() {
                first = array_like_value(&receiver, position)?;
                position += 1;
            }
            (
                first.ok_or_else(|| {
                    JsError("Reduce of empty array with no initial value".to_string())
                })?,
                position,
            )
        }
    };
    while index < length {
        let Some(elem) = array_like_value(&receiver, index)? else {
            index += 1;
            continue;
        };
        let callback_args = vec![
            accumulator.clone(),
            elem,
            Value::Number(index as f64),
            receiver.clone(),
        ];
        accumulator = match &callback {
            Value::Function(_) => {
                call_value_with_this(callback.clone(), callback_args, Value::Undefined)?
            }
            Value::NativeFunction(nf) => nf.call(Value::Undefined, callback_args)?,
            _ => return Err(JsError("Callback is not a function".to_string())),
        };
        index += 1;
    }
    Ok(accumulator)
}

pub fn proto_reduce_right(args: Vec<Value>) -> Result<Value, JsError> {
    let receiver = crate::builtins::get_native_this()
        .ok_or_else(|| JsError("Array.prototype method called on non-object".to_string()))?;
    let receiver = match receiver {
        Value::Object(object) => Value::Object(object),
        primitive => crate::value::to_object(&primitive)?,
    };
    let length = array_like_length(&receiver)?;
    let callback = args.first().cloned().unwrap_or(Value::Undefined);
    let (mut accumulator, mut index) = match args.get(1).cloned() {
        Some(initial) => (initial, length),
        None => {
            let mut position = length;
            let mut last = None;
            while position > 0 && last.is_none() {
                position -= 1;
                last = array_like_value(&receiver, position)?;
            }
            (
                last.ok_or_else(|| {
                    JsError("Reduce of empty array with no initial value".to_string())
                })?,
                position,
            )
        }
    };
    while index > 0 {
        index -= 1;
        let Some(elem) = array_like_value(&receiver, index)? else {
            continue;
        };
        let callback_args = vec![
            accumulator,
            elem,
            Value::Number(index as f64),
            receiver.clone(),
        ];
        accumulator = match &callback {
            Value::Function(_) => {
                call_value_with_this(callback.clone(), callback_args, Value::Undefined)?
            }
            Value::NativeFunction(nf) => nf.call(Value::Undefined, callback_args)?,
            _ => return Err(JsError("Callback is not a function".to_string())),
        };
    }
    Ok(accumulator)
}

/// Array.prototype.forEach(callback, thisArg?)
pub fn proto_for_each(args: Vec<Value>) -> Result<Value, JsError> {
    let callback = args.first().cloned().unwrap_or(Value::Undefined);
    let this_arg = args.get(1).cloned().unwrap_or(Value::Undefined);
    let receiver = match crate::builtins::get_native_this()
        .ok_or_else(|| JsError("Array.prototype method called on non-object".to_string()))?
    {
        Value::Object(object) => Value::Object(object),
        primitive => crate::value::to_object(&primitive)?,
    };
    let length = array_like_length(&receiver)?;

    for i in 0..length {
        let Some(elem) = array_like_value(&receiver, i)? else {
            continue;
        };
        let callback_args = vec![elem, Value::Number(i as f64), receiver.clone()];
        match &callback {
            Value::Function(_) => {
                call_value_with_this(callback.clone(), callback_args, this_arg.clone())?;
            }
            Value::NativeFunction(native) => {
                native.call(this_arg.clone(), callback_args)?;
            }
            _ => return Err(JsError("Callback is not a function".to_string())),
        }
    }
    Ok(Value::Undefined)
}

/// Array.prototype.some(callback, thisArg?)
pub fn proto_some(args: Vec<Value>) -> Result<Value, JsError> {
    let elements = get_this_array()?;
    let callback = args.first().cloned().unwrap_or(Value::Undefined);

    for (i, elem) in elements.iter().enumerate() {
        let result = call_callback(&callback, elem, i, &elements)?;
        if to_bool(&result) {
            return Ok(Value::Boolean(true));
        }
    }
    Ok(Value::Boolean(false))
}

fn array_like_length(receiver: &Value) -> Result<usize, JsError> {
    let length = match receiver {
        Value::Object(object) => {
            if matches!(
                object.borrow().get_own_value("__quench_proxy_revoked"),
                Some(Value::Boolean(true))
            ) {
                return Err(JsError::new("TypeError: revoked proxy"));
            }
            if object.borrow().kind == ObjectKind::Array {
                return Ok(object.borrow().elements.len());
            }
            crate::eval::member::eval_object_member_value(
                object,
                &Value::String("length".to_string()),
                None,
            )?
        }
        Value::Function(function) => crate::eval::member::eval_function_member(function, "length")?,
        _ => {
            return Err(JsError(
                "Array.prototype method called on non-object".to_string(),
            ))
        }
    };
    Ok(crate::value::try_to_number(&length)?.max(0.0) as usize)
}

fn array_like_value(receiver: &Value, index: usize) -> Result<Option<Value>, JsError> {
    let key = index.to_string();
    match receiver {
        Value::Object(object) => {
            if !object.borrow().has(&key) {
                return Ok(None);
            }
            Ok(Some(crate::eval::member::eval_object_member_value(
                object,
                &Value::String(key),
                None,
            )?))
        }
        Value::Function(function) => Ok(function.get_property(&key)),
        _ => Ok(None),
    }
}

/// Array.prototype.every(callback, thisArg?)
pub fn proto_every(args: Vec<Value>) -> Result<Value, JsError> {
    let callback = args.first().cloned().unwrap_or(Value::Undefined);
    let this_arg = args.get(1).cloned().unwrap_or(Value::Undefined);
    let receiver = match crate::builtins::get_native_this()
        .ok_or_else(|| JsError("Array.prototype method called on non-object".to_string()))?
    {
        Value::Object(object) => Value::Object(object),
        primitive => crate::value::to_object(&primitive)?,
    };
    let length = array_like_length(&receiver)?;

    for i in 0..length {
        let Some(elem) = array_like_value(&receiver, i)? else {
            continue;
        };
        let callback_args = vec![elem, Value::Number(i as f64), receiver.clone()];
        let result = match &callback {
            Value::Function(_) => {
                call_value_with_this(callback.clone(), callback_args, this_arg.clone())?
            }
            Value::NativeFunction(native) => native.call(this_arg.clone(), callback_args)?,
            _ => return Err(JsError("Callback is not a function".to_string())),
        };
        if !to_bool(&result) {
            return Ok(Value::Boolean(false));
        }
    }
    Ok(Value::Boolean(true))
}

#[cfg(test)]
mod tests {
    use crate::{Context, Value};

    #[test]
    fn filter_uses_array_like_length() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("Array.prototype.filter.call({0: 12, 1: 11, 2: 9, length: 2}, function() { return true; }).length"),
            Ok(Value::Number(2.0))
        );
    }

    #[test]
    fn filter_reads_inherited_array_like_length() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("var p = {}; Object.defineProperty(p, 'length', {get: function() { return 2; }}); var C = function() {}; C.prototype = p; var o = new C(); o[0] = 12; o[1] = 11; o[2] = 9; Array.prototype.filter.call(o, function() { return true; }).length"),
            Ok(Value::Number(2.0))
        );
    }

    #[test]
    fn filter_reads_string_object_indices() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("Array.prototype.filter.call(new String('012'), function() { return true; }).length"),
            Ok(Value::Number(3.0))
        );
    }

    #[test]
    fn filter_has_standard_length_descriptor() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("var d=Object.getOwnPropertyDescriptor(Array.prototype.filter,'length'); [d.value,d.writable,d.enumerable,d.configurable].join('|')"),
            Ok(Value::String("1|false|false|true".to_string()))
        );
    }

    #[test]
    fn filter_uses_custom_species_prototype() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("var a=[1]; var C=function(){}; a.constructor={}; a.constructor[Symbol.species]=C; var r=a.filter(function(){return true}); Object.getPrototypeOf(r)===C.prototype"),
            Ok(Value::Boolean(true))
        );
    }

    #[test]
    fn filter_throws_when_species_result_is_non_extensible() {
        let mut ctx = Context::new().unwrap();
        assert!(ctx
            .eval("var A=function(){this.length=0; Object.preventExtensions(this)}; var a=[1]; a.constructor={}; a.constructor[Symbol.species]=A; a.filter(function(){return true})")
            .is_err());
    }

    #[test]
    fn filter_throws_when_species_index_is_non_configurable() {
        let mut ctx = Context::new().unwrap();
        assert!(ctx
            .eval("var A=function(){Object.defineProperty(this,'0',{writable:true,configurable:false})}; var a=[1]; a.constructor={}; a.constructor[Symbol.species]=A; a.filter(function(){return true})")
            .is_err());
    }

    #[test]
    fn array_search_transform_methods_have_standard_lengths() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("[Array.prototype.findLast,Array.prototype.findLastIndex,Array.prototype.flatMap].map(function(f){return f.length}).join('|')"),
            Ok(Value::String("1|1|1".to_string()))
        );
    }

    #[test]
    fn every_has_standard_length_descriptor() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("var d=Object.getOwnPropertyDescriptor(Array.prototype.every,'length'); [d.value,d.writable,d.enumerable,d.configurable].join('|')"),
            Ok(Value::String("1|false|false|true".to_string()))
        );
    }

    #[test]
    fn filter_rejects_revoked_proxy_before_callback() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("var o=Proxy.revocable([],{}); var called=false; o.revoke(); try { Array.prototype.filter.call(o.proxy,function(){called=true;}); false } catch(e) { !called }"),
            Ok(Value::Boolean(true))
        );
    }

    #[test]
    fn array_copy_within_and_fill_are_registered() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("var a = [1, 2, 3]; a.copyWithin(1, 0); var b = [1, 2, 3]; b.fill(4, 1); [a.join(','), b.join(',')].join('|')"),
            Ok(Value::String("1,1,2|1,4,4".to_string()))
        );
    }

    #[test]
    fn copy_within_supports_negative_end() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("[0, 1, 2, 3].copyWithin(1, 0, '-2').join(',')"),
            Ok(Value::String("0,0,1,3".to_string()))
        );
    }

    #[test]
    fn copy_within_reloads_elements_after_start_coercion() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("var a = []; a.length = 20; var b = []; for (var i = 0; i < 1024; i++) b[i] = i; b.copyWithin(0, {valueOf: function() { b.length = 20; return 1000; }}); [b.length, b[0]].join('|')"),
            Ok(Value::String("20|undefined".to_string()))
        );
    }

    #[test]
    fn fill_treats_explicit_undefined_end_as_length() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("[0, 0].fill(1, 0, undefined).join(',')"),
            Ok(Value::String("1,1".to_string()))
        );
    }

    #[test]
    fn fill_propagates_end_coercion_errors() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("var err = {}; var end = {valueOf: function() { throw err; }}; try { [].fill(1, 0, end); false; } catch (e) { e === err; }")
                .unwrap(),
            Value::Boolean(true)
        );
    }

    #[test]
    fn fill_propagates_array_like_length_errors() {
        let mut ctx = Context::new().unwrap();
        assert!(ctx
            .eval("var o = {}; Object.defineProperty(o, 'length', {get: function() { throw new Error('length'); }}); [].fill.call(o, 1)")
            .is_err());
    }

    #[test]
    fn copy_within_uses_array_like_length() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("var o = {0: 1, 1: 2, 2: 3, length: 3}; Array.prototype.copyWithin.call(o, 0, 1); [o[0], o[1], o[2]].join('|')"),
            Ok(Value::String("2|3|3".to_string()))
        );
    }

    #[test]
    fn array_find_index_is_registered() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("[10, 20, 30].findIndex(function(value) { return value === 20; })"),
            Ok(Value::Number(1.0))
        );
    }

    #[test]
    fn array_find_index_reads_each_element_at_call_time() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("var a=['x','y']; var r=[]; a.findIndex(function(v) { r.push(v); if (r.length === 1) a.shift(); }); r.join('|')"),
            Ok(Value::String("x|undefined".to_string()))
        );
    }

    #[test]
    fn array_last_index_of_is_registered() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("[1, 2, 1].lastIndexOf(1, -1)"),
            Ok(Value::Number(2.0))
        );
    }

    #[test]
    fn array_reduce_right_is_registered() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("[1, 2, 3].reduceRight(function(a, b) { return a - b; })"),
            Ok(Value::Number(0.0))
        );
    }

    #[test]
    fn array_every_coerces_array_like_length() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("var accessed=false; var o={0:1,1:1}; Object.defineProperty(o,'length',{get:function(){return {toString:function(){accessed=true;return '2';}}}}); Array.prototype.every.call(o,function(v){return v===1;}); accessed"),
            Ok(Value::Boolean(true))
        );
    }

    #[test]
    fn array_every_reads_holes_after_callback_mutation() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("var seen=false; var a=[1,2,,4]; a.every(function(v,i){if(i===0)a[2]=3;if(v===3)seen=true;return true;}); seen"),
            Ok(Value::Boolean(true))
        );
    }

    #[test]
    fn array_every_defers_array_like_getters() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("var accessed=false; var o={length:2}; Object.defineProperty(o,'1',{get:function(){return 6.99;},configurable:true}); Object.defineProperty(o,'0',{get:function(){delete o[1];return 0;},configurable:true}); Array.prototype.every.call(o,function(){accessed=true;return true;}); accessed"),
            Ok(Value::Boolean(true))
        );
    }

    #[test]
    fn array_for_each_passes_original_receiver() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("var seen=false; Math.length=1; Math[0]=1; Array.prototype.forEach.call(Math,function(v,i,obj){seen=Object.prototype.toString.call(obj)==='[object Math]';}); seen"),
            Ok(Value::Boolean(true))
        );
    }

    #[test]
    fn array_concat_boxes_primitive_receiver() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("Array.prototype.concat.call(101).length"),
            Ok(Value::Number(1.0))
        );
    }

    #[test]
    fn array_every_boxes_boolean_receiver() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("var accessed=false; Boolean.prototype[0]=1; Boolean.prototype.length=1; Array.prototype.every.call(false,function(v,i,obj){accessed=obj instanceof Boolean;return accessed;}); accessed"),
            Ok(Value::Boolean(true))
        );
    }

    #[test]
    fn array_filter_boxes_boolean_receiver() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("Boolean.prototype[0]=true; Boolean.prototype.length=1; Array.prototype.filter.call(false,function(v,i,obj){return obj instanceof Boolean;}).length"),
            Ok(Value::Number(1.0))
        );
    }

    #[test]
    fn array_find_boxes_boolean_receiver() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("Boolean.prototype[0]=true; Boolean.prototype.length=1; Array.prototype.find.call(false,function(v){return v;})"),
            Ok(Value::Boolean(true))
        );
    }

    #[test]
    fn array_for_each_preserves_json_receiver_tag() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("var seen=false; JSON.length=1; JSON[0]=1; Array.prototype.forEach.call(JSON,function(v,i,obj){seen=Object.prototype.toString.call(obj)==='[object JSON]';}); seen"),
            Ok(Value::Boolean(true))
        );
    }

    #[test]
    fn array_to_reversed_is_registered() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("[1, 2, 3].toReversed().join(',')"),
            Ok(Value::String("3,2,1".to_string()))
        );
    }

    #[test]
    fn array_to_sorted_is_registered() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("[3, 1, 2].toSorted().join(',')"),
            Ok(Value::String("1,2,3".to_string()))
        );
    }

    #[test]
    fn array_to_spliced_is_registered() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("[1, 2, 3].toSpliced(1, 1, 4).join(',')"),
            Ok(Value::String("1,4,3".to_string()))
        );
    }

    #[test]
    fn array_index_of_reads_function_properties() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("var f=function(a,b){}; f[1]=true; Array.prototype.indexOf.call(f,true)"),
            Ok(Value::Number(1.0))
        );
    }

    #[test]
    fn array_iterator_aliases_values() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("Array.prototype[Symbol.iterator] === Array.prototype.values"),
            Ok(Value::Boolean(true))
        );
    }

    #[test]
    fn array_copy_within_throws_on_nonconfigurable_delete() {
        let mut ctx = Context::new().unwrap();
        assert!(ctx
            .eval("var o={length:43}; Object.defineProperty(o,'42',{configurable:false,writable:true}); Array.prototype.copyWithin.call(o,42,0)")
            .is_err());
    }

    #[test]
    fn array_map_passes_original_receiver() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("var a=[1]; var same=false; a.map(function(v,i,o){same=o===a;}); same"),
            Ok(Value::Boolean(true))
        );
    }

    #[test]
    fn array_map_preserves_math_receiver_tag() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("Math.length=1; Math[0]=1; Array.prototype.map.call(Math,function(v,i,o){return Object.prototype.toString.call(o)==='[object Math]';})[0]"),
            Ok(Value::Boolean(true))
        );
    }

    #[test]
    fn array_from_reads_array_like_length_and_index() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("Array.from({0:'a',length:1}).length"),
            Ok(Value::Number(1.0))
        );
    }

    #[test]
    fn array_from_applies_map_function() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("Array.from([1,2],function(value,index){return value+index;}).join(',')"),
            Ok(Value::String("1,3".to_string()))
        );
    }

    #[test]
    fn array_for_each_boxes_string_with_string_prototype() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("var result=false; Array.prototype.forEach.call('a',function(v,i,o){result=o instanceof String;}); result"),
            Ok(Value::Boolean(true))
        );
    }

    #[test]
    fn array_for_each_binds_this_argument() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("var receiver={}; var same=false; [1].forEach(function(){same=this===receiver;},receiver); same"),
            Ok(Value::Boolean(true))
        );
    }

    #[test]
    fn array_reduce_reads_sparse_accessor_elements() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("var seen=false; var a=[,1,2]; Object.defineProperty(a,'0',{get:function(){return 0;}}); a.reduce(function(prev,value,index){if(index===1) seen=prev===0; return value;}); seen"),
            Ok(Value::Boolean(true))
        );
    }

    #[test]
    fn array_reduce_reads_string_wrapper_indices() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("var seen=false; var value=new String('012'); Array.prototype.reduce.call(value,function(prev,current,index){if(index===1) seen=prev==='0'; return current;}); seen"),
            Ok(Value::Boolean(true))
        );
    }

    #[test]
    fn array_reduce_right_reads_sparse_accessor_elements() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("var seen=false; var a=[0,1,,3]; Object.defineProperty(a,'2',{get:function(){return 2;}}); a.reduceRight(function(prev,value,index){if(index===2) seen=value===2; return value;}); seen"),
            Ok(Value::Boolean(true))
        );
    }

    #[test]
    fn array_from_async_is_registered() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("typeof Array.fromAsync"),
            Ok(Value::String("function".to_string()))
        );
    }

    #[test]
    fn array_from_has_one_argument_length() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(ctx.eval("Array.from.length"), Ok(Value::Number(1.0)));
    }

    #[test]
    fn array_from_reads_sync_iterables() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("Array.from(new Set([1,2])).join(',')"),
            Ok(Value::String("1,2".to_string()))
        );
    }

    #[test]
    fn array_map_binds_this_argument() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("var receiver={}; [1].map(function(){return this===receiver;},receiver)[0]"),
            Ok(Value::Boolean(true))
        );
    }

    #[test]
    fn array_from_closes_iterator_when_map_throws() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("var closed=0; var items={}; items[Symbol.iterator]=function(){return {next:function(){return {done:false,value:1};},return:function(){closed++;}};}; try {Array.from(items,function(){throw new Error();});} catch(e) {} closed"),
            Ok(Value::Number(1.0))
        );
    }

    #[test]
    fn array_at_has_one_argument_length() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("Array.prototype.at.length"),
            Ok(Value::Number(1.0))
        );
    }

    #[test]
    fn array_every_binds_this_argument() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("var receiver={}; [1].every(function(){return this===receiver;},receiver)"),
            Ok(Value::Boolean(true))
        );
    }

    #[test]
    fn array_at_tracks_shared_resizable_buffer_shrink() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("var b=new ArrayBuffer(4,{maxByteLength:8}); var fixed=new Uint8Array(b,0,4); var writer=new Uint8Array(b); writer[3]=3; var before=Array.prototype.at.call(fixed,-1); b.resize(3); before===3 && Array.prototype.at.call(fixed,-1)===undefined"),
            Ok(Value::Boolean(true))
        );
    }

    #[test]
    fn array_at_reads_typed_array_after_index_coercion() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("var b=new ArrayBuffer(4,{maxByteLength:8}); var a=new Uint8Array(b,0,4); a[0]=3; var evil={valueOf:function(){b.resize(2); return 0;}}; Array.prototype.at.call(a,evil)"),
            Ok(Value::Undefined)
        );
    }

    #[test]
    fn array_from_propagates_iterator_errors() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("var b=new ArrayBuffer(4,{maxByteLength:8}); var a=new Uint8Array(b,0,4); b.resize(3); try {Array.from(Array.prototype.entries.call(a)); false;} catch(e) {e instanceof TypeError;}"),
            Ok(Value::Boolean(true))
        );
    }

    #[test]
    fn array_prototype_is_an_exotic_array() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("Array.prototype[2]=42; Array.prototype.length"),
            Ok(Value::Number(3.0))
        );
        assert_eq!(
            ctx.eval("({}).toString.call(Array.prototype)"),
            Ok(Value::String("[object Array]".to_string()))
        );
    }
}

/// Flatten helper for Array.prototype.flat
pub fn flatten_array(arr: Vec<Value>, depth: i32) -> Vec<Value> {
    if depth <= 0 {
        return arr;
    }
    let mut result = Vec::new();
    for elem in arr {
        match elem {
            Value::Object(o) if o.borrow().kind == ObjectKind::Array => {
                let inner = o.borrow().elements.clone();
                result.extend(flatten_array(inner, depth - 1));
            }
            _ => result.push(elem),
        }
    }
    result
}

/// Array.prototype.flat(depth?)
pub fn proto_flat(args: Vec<Value>) -> Result<Value, JsError> {
    let elements = get_this_array()?;
    let depth = args.first().map(|v| to_number(v) as i32).unwrap_or(1);
    Ok(make_array(flatten_array(elements, depth)))
}

/// Array.prototype.flatMap(callback, thisArg?)
pub fn proto_flat_map(args: Vec<Value>) -> Result<Value, JsError> {
    let elements = get_this_array()?;
    let callback = args.first().cloned().unwrap_or(Value::Undefined);

    let mut result = Vec::new();
    for (i, elem) in elements.iter().enumerate() {
        let mapped = call_callback(&callback, elem, i, &elements)?;
        // Flatten by one level if array
        if let Value::Object(ref o) = mapped {
            let inner = o.borrow();
            if inner.kind == ObjectKind::Array {
                result.extend(inner.elements.clone());
                continue;
            }
        }
        result.push(mapped);
    }
    Ok(make_array(result))
}
