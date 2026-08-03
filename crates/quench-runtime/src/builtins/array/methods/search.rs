//! Array search methods (indexOf, includes, find, findLast, findLastIndex)

use crate::value::{JsError, ObjectKind, Value};

/// Get the array elements from 'this'
fn get_this_array() -> Result<Vec<Value>, JsError> {
    crate::builtins::array::methods::transformation::get_this_array()
}

/// Call callback for find/findLast methods
fn call_find_callback(
    callback: &Value,
    elem: &Value,
    index: usize,
    elements: &[Value],
) -> Result<Value, JsError> {
    let array_copy = crate::builtins::array::methods::transformation::make_array(elements.to_vec());
    let callback_args = vec![elem.clone(), Value::Number(index as f64), array_copy];

    match callback {
        Value::Function(_) => {
            crate::eval::call_value_with_this(callback.clone(), callback_args, Value::Undefined)
        }
        Value::NativeFunction(nf) => nf.call(Value::Undefined, callback_args),
        _ => Err(JsError("Callback is not a function".to_string())),
    }
}

fn ensure_callable(callback: &Value) -> Result<(), JsError> {
    if matches!(callback, Value::Function(_) | Value::NativeFunction(_)) {
        Ok(())
    } else {
        Err(JsError::new("TypeError: predicate is not callable"))
    }
}

// ============================================================================
// Search method implementations
// ============================================================================

/// Resolve a fromIndex argument: negative values count back from the end.
fn resolve_from_index(arg: Option<&Value>, len: usize) -> Result<usize, JsError> {
    Ok(match arg {
        Some(v) => {
            let n = crate::value::try_to_number(v)?;
            if n < 0.0 {
                ((len as f64 + n).max(0.0)) as usize
            } else {
                (n as usize).min(len)
            }
        }
        None => 0,
    })
}

fn same_value_zero(a: &Value, b: &Value) -> bool {
    crate::value::compare::same_value_zero(a, b)
}

/// Array.prototype.indexOf(searchElement, fromIndex?)
pub fn proto_index_of(args: Vec<Value>) -> Result<Value, JsError> {
    let search = args.first().cloned().unwrap_or(Value::Undefined);
    let receiver = crate::builtins::get_native_this();
    let (length, array) = match receiver {
        Some(Value::Object(object)) if object.borrow().kind == ObjectKind::Array => {
            let length = object.borrow().elements.len();
            (length, Some(object))
        }
        Some(Value::Object(object)) => (get_this_array()?.len(), Some(object)),
        _ => (get_this_array()?.len(), None),
    };
    let from_idx = resolve_from_index(args.get(1), length)?;

    #[allow(clippy::needless_range_loop)]
    for i in from_idx..length {
        let value = match &array {
            Some(object) => crate::eval::member::eval_object_member_value(
                object,
                &Value::String(i.to_string()),
                None,
            )?,
            None => get_this_array()?
                .get(i)
                .cloned()
                .unwrap_or(Value::Undefined),
        };
        if crate::value::strict_eq(&value, &search) {
            return Ok(Value::Number(i as f64));
        }
    }
    Ok(Value::Number(-1.0))
}

pub fn proto_last_index_of(args: Vec<Value>) -> Result<Value, JsError> {
    let search = args.first().cloned().unwrap_or(Value::Undefined);
    let receiver = crate::builtins::get_native_this()
        .ok_or_else(|| JsError("Array.prototype method called on non-object".to_string()))?;
    let (elements, object) = match receiver {
        Value::Object(object) if object.borrow().kind == ObjectKind::Array => {
            let elements = object.borrow().elements.clone();
            let length = elements.len();
            (elements, Some((object, length)))
        }
        Value::Object(object) => {
            let length_value = crate::eval::member::eval_object_member_value(
                &object,
                &Value::String("length".to_string()),
                None,
            )?;
            let length = crate::value::try_to_number(&length_value)?.max(0.0) as usize;
            (Vec::new(), Some((object, length)))
        }
        _ => {
            return Err(JsError(
                "Array.prototype method called on non-object".to_string(),
            ))
        }
    };
    let length = object
        .as_ref()
        .map_or(elements.len(), |(_, length)| *length);
    if length == 0 {
        return Ok(Value::Number(-1.0));
    }
    let from = match args.get(1) {
        Some(value) => crate::value::try_to_number(value)?,
        None => (length - 1) as f64,
    };
    if from.is_nan() {
        return Ok(Value::Number(-1.0));
    }
    let start = if from < 0.0 {
        let adjusted = length as f64 + from;
        if adjusted < 0.0 {
            return Ok(Value::Number(-1.0));
        }
        adjusted as usize
    } else {
        (from as usize).min(length - 1)
    };
    for i in (0..=start).rev() {
        let value = if let Some((object, _)) = &object {
            crate::eval::member::eval_object_member_value(
                object,
                &Value::String(i.to_string()),
                None,
            )?
        } else {
            elements[i].clone()
        };
        if crate::value::strict_eq(&value, &search) {
            return Ok(Value::Number(i as f64));
        }
    }
    Ok(Value::Number(-1.0))
}

/// Array.prototype.includes(searchElement, fromIndex?)
pub fn proto_includes(args: Vec<Value>) -> Result<Value, JsError> {
    let elements = get_this_array()?;
    let search = args.first().cloned().unwrap_or(Value::Undefined);
    let from_idx = resolve_from_index(args.get(1), elements.len())?;

    #[allow(clippy::needless_range_loop)]
    for i in from_idx..elements.len() {
        if same_value_zero(&elements[i], &search) {
            return Ok(Value::Boolean(true));
        }
    }
    Ok(Value::Boolean(false))
}

/// Array.prototype.find(predicate, thisArg?)
pub fn proto_find(args: Vec<Value>) -> Result<Value, JsError> {
    let callback = args.first().cloned().unwrap_or(Value::Undefined);
    ensure_callable(&callback)?;
    let elements = get_this_array()?;

    for (i, elem) in elements.iter().enumerate() {
        let result = crate::builtins::array::methods::transformation::call_callback(
            &callback, elem, i, &elements,
        )?;
        if crate::value::to_bool(&result) {
            return Ok(elem.clone());
        }
    }
    Ok(Value::Undefined)
}

pub fn proto_find_index(args: Vec<Value>) -> Result<Value, JsError> {
    let elements = get_this_array()?;
    let callback = args.first().cloned().unwrap_or(Value::Undefined);
    ensure_callable(&callback)?;
    let receiver = crate::builtins::get_native_this();
    for i in 0..elements.len() {
        let elem = match &receiver {
            Some(Value::Object(object)) if object.borrow().kind == ObjectKind::Array => object
                .borrow()
                .elements
                .get(i)
                .cloned()
                .unwrap_or(Value::Undefined),
            Some(Value::Object(object)) => crate::eval::member::eval_object_member_value(
                object,
                &Value::String(i.to_string()),
                None,
            )
            .unwrap_or(Value::Undefined),
            _ => Value::Undefined,
        };
        let result = crate::builtins::array::methods::transformation::call_callback(
            &callback, &elem, i, &elements,
        )?;
        if crate::value::to_bool(&result) {
            return Ok(Value::Number(i as f64));
        }
    }
    Ok(Value::Number(-1.0))
}

/// Array.prototype.findLast(predicate, thisArg?)
/// Iterates from the end of the array
pub fn proto_find_last(args: Vec<Value>) -> Result<Value, JsError> {
    let elements = get_this_array()?;
    let callback = args.first().cloned().unwrap_or(Value::Undefined);
    ensure_callable(&callback)?;
    let len = elements.len();

    for i in (0..len).rev() {
        let result = call_find_callback(&callback, &elements[i], i, &elements)?;
        if crate::value::to_bool(&result) {
            return Ok(elements[i].clone());
        }
    }
    Ok(Value::Undefined)
}

/// Array.prototype.findLastIndex(predicate, thisArg?)
/// Iterates from the end of the array, returns index or -1
pub fn proto_find_last_index(args: Vec<Value>) -> Result<Value, JsError> {
    let elements = get_this_array()?;
    let callback = args.first().cloned().unwrap_or(Value::Undefined);
    ensure_callable(&callback)?;
    let len = elements.len();

    for i in (0..len).rev() {
        let result = call_find_callback(&callback, &elements[i], i, &elements)?;
        if crate::value::to_bool(&result) {
            return Ok(Value::Number(i as f64));
        }
    }
    Ok(Value::Number(-1.0))
}

#[cfg(test)]
mod tests {
    fn create_test_context() -> crate::Context {
        let mut ctx = crate::Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        ctx
    }

    #[test]
    fn test_includes_nan() {
        // Bug fix: includes uses SameValueZero, so [NaN].includes(NaN) is true
        let mut ctx = create_test_context();
        let result = ctx.eval("[NaN].includes(NaN)");
        assert_eq!(result.unwrap(), crate::value::Value::Boolean(true));
    }

    #[test]
    fn includes_rejects_symbol_from_index() {
        let mut ctx = create_test_context();
        assert!(ctx.eval("[7].includes(7, Symbol('1'))").is_err());
    }

    #[test]
    fn index_of_propagates_first_accessor_error() {
        let mut ctx = create_test_context();
        assert!(ctx
            .eval("var accessed=false; var a=[]; Object.defineProperty(a,'0',{get:function(){throw new TypeError},configurable:true}); Object.defineProperty(a,'1',{get:function(){accessed=true;return true},configurable:true}); try { a.indexOf(true); false } catch(e) { !accessed }")
            .unwrap()
            == crate::value::Value::Boolean(true));
    }

    #[test]
    fn index_of_accepts_array_like_objects() {
        let mut ctx = create_test_context();
        assert_eq!(
            ctx.eval("Array.prototype.indexOf.call({1:true,length:2}, true)")
                .unwrap(),
            crate::value::Value::Number(1.0)
        );
    }

    #[test]
    fn index_of_reads_array_like_properties_added_during_iteration() {
        let mut ctx = create_test_context();
        assert_eq!(
            ctx.eval("var a={length:2}; Object.defineProperty(a,'0',{get:function(){Object.defineProperty(a,'1',{value:1,configurable:true,writable:true});return 0},configurable:true}); Array.prototype.indexOf.call(a,0)===0 && Array.prototype.indexOf.call(a,1)===1").unwrap(),
            crate::value::Value::Boolean(true)
        );
    }

    #[test]
    fn last_index_of_propagates_from_index_conversion_error() {
        let mut ctx = create_test_context();
        assert!(ctx.eval("[0,null].lastIndexOf(null,{toString:function(){return {}},valueOf:function(){return {}}})").is_err());
    }

    #[test]
    fn last_index_of_propagates_accessor_error_in_reverse_order() {
        let mut ctx = create_test_context();
        assert!(ctx
            .eval("var accessed=false; var a=[]; Object.defineProperty(a,'2',{get:function(){throw new TypeError},configurable:true}); Object.defineProperty(a,'1',{get:function(){accessed=true;return true},configurable:true}); try { a.lastIndexOf(true); false } catch(e) { !accessed }")
            .unwrap()
            == crate::value::Value::Boolean(true));
    }

    #[test]
    fn find_rejects_non_callable_predicate_on_empty_array() {
        let mut ctx = create_test_context();
        assert!(ctx.eval("[].find({})").is_err());
        assert!(ctx.eval("[].findIndex({})").is_err());
        assert!(ctx.eval("[].findLast({})").is_err());
        assert!(ctx.eval("[].findLastIndex({})").is_err());
    }

    #[test]
    fn test_index_of_negative_from_index() {
        // Bug fix: negative fromIndex counts back from the end
        let mut ctx = create_test_context();
        let result = ctx.eval("[1,2,3].indexOf(2, -2)");
        assert_eq!(result.unwrap(), crate::value::Value::Number(1.0));
        let result = ctx.eval("[1,2,3].includes(1, -3)");
        assert_eq!(result.unwrap(), crate::value::Value::Boolean(true));
    }
}
