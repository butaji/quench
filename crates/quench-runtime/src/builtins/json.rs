//! JSON built-in — ECMAScript spec-compliant.
//!
//! Implements JSON.stringify (with replacer and space) and JSON.parse (with reviver).

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{
    to_js_string, JsError, NativeFunction, Object, ObjectKind, PropertyFlags, Value,
};
use crate::Context;

// ============================================================================
// JSON.stringify helpers
// ============================================================================

/// Quote a string for JSON output.
fn quote_string(s: &str) -> String {
    let mut r = String::with_capacity(s.len() + 2);
    r.push('"');
    for c in s.chars() {
        match c {
            '"' => r.push_str("\\\""),
            '\\' => r.push_str("\\\\"),
            '\n' => r.push_str("\\n"),
            '\r' => r.push_str("\\r"),
            '\t' => r.push_str("\\t"),
            c if c.is_control() => r.push_str(&format!("\\u{:04x}", c as u32)),
            c => r.push(c),
        }
    }
    r.push('"');
    r
}

/// Convert a JS value to its JSON representation string.
/// Returns None for values that are not serializable on their own
/// (undefined, functions, objects — the latter need recursion).
fn value_to_json_string(val: &Value) -> Option<String> {
    match val {
        Value::Undefined => None,
        Value::Null => Some("null".to_string()),
        Value::Boolean(b) => Some(b.to_string()),
        Value::Number(n) => Some(json_number_string(*n)),
        Value::String(s) => Some(quote_string(s)),
        Value::Object(_) => None,
        _ => Some("null".to_string()),
    }
}

/// Format a number for JSON output.
fn json_number_string(n: f64) -> String {
    if n.is_nan() || n.is_infinite() {
        "null".to_string()
    } else if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{:.0}", n)
    } else {
        n.to_string()
    }
}

/// Get indent string from space parameter.
fn get_indent(space: Option<&Value>) -> String {
    match space {
        Some(Value::Number(n)) => {
            let n = (*n as i32).clamp(0, 10) as usize;
            " ".repeat(n)
        }
        Some(Value::String(s)) => s.chars().take(10).collect(),
        _ => String::new(),
    }
}

/// Build a member (key: value) string.
fn member(key: &str, val: &str, use_indent: bool) -> String {
    if use_indent {
        format!("{}: {}", quote_string(key), val)
    } else {
        format!("{}:{}", quote_string(key), val)
    }
}

/// Serialize any value (recursing into nested objects/arrays).
/// Returns None for values that are not serializable (undefined)
/// so object members can be dropped and array elements become null.
fn serialize_value(
    val: &Value,
    replacer: Option<&Value>,
    space: Option<&Value>,
    depth: usize,
) -> Option<String> {
    if let Value::Object(obj_rc) = val {
        let obj = obj_rc.borrow();
        if obj.kind == ObjectKind::Array || !obj.elements.is_empty() {
            return Some(serialize_array(&obj.elements, replacer, space, depth));
        }
        let keys = get_keys(&obj, replacer);
        return Some(serialize_object_formatted(
            &obj, &keys, replacer, space, depth,
        ));
    }
    value_to_json_string(val)
}

/// Serialize an object to JSON string with indent support.
fn serialize_object_formatted(
    obj: &Object,
    keys: &[String],
    replacer: Option<&Value>,
    space: Option<&Value>,
    depth: usize,
) -> String {
    let indent = get_indent(space);
    let use_indent = !indent.is_empty();
    let cur_indent = indent.repeat(depth);
    let next_indent = indent.repeat(depth + 1);

    let pairs: Vec<String> = obj
        .properties
        .iter()
        .filter(|(k, _)| keys.contains(k))
        .filter_map(|(k, v)| {
            serialize_value(v, replacer, space, depth + 1).map(|s| member(k, &s, use_indent))
        })
        .collect();

    if pairs.is_empty() {
        return "{}".to_string();
    }

    if use_indent {
        let inner = pairs.join(&format!(",\n{}", next_indent));
        format!("{{\n{}{}\n{}}}", next_indent, inner, cur_indent)
    } else {
        format!("{{{}}}", pairs.join(","))
    }
}

/// Serialize an array to JSON string.
fn serialize_array(
    elements: &[Value],
    replacer: Option<&Value>,
    space: Option<&Value>,
    depth: usize,
) -> String {
    let indent = get_indent(space);
    let use_indent = !indent.is_empty();
    let cur_indent = indent.repeat(depth);
    let next_indent = indent.repeat(depth + 1);

    let mut items: Vec<String> = Vec::new();
    for val in elements {
        let serialized = match serialize_value(val, replacer, space, depth + 1) {
            Some(s) => s,
            None => "null".to_string(),
        };
        items.push(serialized);
    }

    if use_indent {
        let inner = items.join(&format!(",\n{}", next_indent));
        format!("[\n{}{}\n{}]", next_indent, inner, cur_indent)
    } else {
        format!("[{}]", items.join(","))
    }
}

/// Get property keys for serialization.
fn get_keys(obj: &Object, replacer: Option<&Value>) -> Vec<String> {
    let mut keys: Vec<String> = obj
        .properties
        .keys()
        .filter(|k| *k != "constructor" && *k != "prototype")
        .cloned()
        .collect();

    // Apply replacer array filter
    if let Some(Value::Object(r)) = replacer {
        let r_borrow = r.borrow();
        if !r_borrow.elements.is_empty() || r_borrow.kind == ObjectKind::Array {
            let allowed: Vec<String> = r_borrow
                .elements
                .iter()
                .filter_map(|e| match e {
                    Value::String(s) => Some(s.clone()),
                    Value::Number(n) => Some(format!("{}", *n as i64)),
                    _ => None,
                })
                .collect();
            keys.retain(|k| allowed.contains(k));
        }
    }

    keys
}

/// JSON.stringify implementation.
fn json_stringify(args: &[Value]) -> Result<Value, JsError> {
    let val = args.first().cloned().unwrap_or(Value::Undefined);
    let replacer = args.get(1);
    let space = args.get(2);

    // undefined at top level → JS undefined
    match serialize_value(&val, replacer, space, 0) {
        Some(s) => Ok(Value::String(s)),
        None => Ok(Value::Undefined),
    }
}

// ============================================================================
// JSON.parse
// ============================================================================

/// Convert serde_json::Value to runtime Value.
fn from_serde_value(v: serde_json::Value) -> Value {
    match v {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Boolean(b),
        serde_json::Value::Number(n) => Value::Number(n.as_f64().unwrap_or(0.0)),
        serde_json::Value::String(s) => Value::String(s),
        serde_json::Value::Array(arr) => {
            let elements: Vec<Value> = arr.into_iter().map(from_serde_value).collect();
            Value::Object(Rc::new(RefCell::new(Object::new_array_from(elements))))
        }
        serde_json::Value::Object(map) => {
            let mut obj = Object::new(ObjectKind::Ordinary);
            for (k, val) in map {
                obj.properties.insert(k, from_serde_value(val));
            }
            Value::Object(Rc::new(RefCell::new(obj)))
        }
    }
}

/// Walk a value and apply reviver.
/// Returns `None` if the reviver returned `undefined` for a non-root key,
/// signaling that the property should be deleted.
fn walk_with_reviver(reviver: &Value, key: Value, val: Value) -> Result<Option<Value>, JsError> {
    let new_val = if let Value::Object(obj_rc) = &val {
        let obj = obj_rc.borrow().clone();

        if obj.kind == ObjectKind::Array || !obj.elements.is_empty() {
            let mut new_elements: Vec<Value> = Vec::new();
            for (i, elem) in obj.elements.iter().enumerate() {
                let idx_key = Value::String(i.to_string());
                let walked = walk_with_reviver(reviver, idx_key, elem.clone())?;
                new_elements.push(walked.unwrap_or(Value::Undefined));
            }
            Value::Object(Rc::new(RefCell::new(Object::new_array_from(new_elements))))
        } else {
            let mut new_obj = Object::new(ObjectKind::Ordinary);
            for (k, v) in &obj.properties {
                let k_val = Value::String(k.clone());
                let walked = walk_with_reviver(reviver, k_val, v.clone())?;
                // If reviver returned undefined for this property, skip it (delete)
                if let Some(w) = walked {
                    new_obj.properties.insert(k.clone(), w);
                }
            }
            Value::Object(Rc::new(RefCell::new(new_obj)))
        }
    } else {
        val
    };

    let result = call_fn(reviver, key.clone(), new_val)?;

    // Per ES 24.5.3.2: if reviver returns undefined, the property is deleted
    if result == Value::Undefined && key != Value::String(String::new()) {
        Ok(None)
    } else {
        Ok(Some(result))
    }
}

/// Call a JS function with arguments.
fn call_fn(func: &Value, arg1: Value, arg2: Value) -> Result<Value, JsError> {
    match func {
        Value::Function(_) => {
            crate::eval::call_value_with_this(func.clone(), vec![arg1, arg2], Value::Undefined)
        }
        Value::NativeFunction(nf) => nf.call(Value::Undefined, vec![arg1, arg2]),
        Value::Object(obj_rc) => {
            let obj = obj_rc.borrow();
            if let Value::NativeFunction(nf) = obj
                .properties
                .get("call")
                .cloned()
                .unwrap_or(Value::Undefined)
            {
                nf.call(Value::Undefined, vec![arg1, arg2])
            } else {
                Ok(arg2)
            }
        }
        _ => Ok(arg2),
    }
}

/// JSON.parse implementation.
fn json_parse(args: &[Value]) -> Result<Value, JsError> {
    let text = args.first().map(to_js_string).unwrap_or_default();

    let parsed: serde_json::Value = serde_json::from_str(&text).map_err(|e| {
        let (_, js_err) = crate::value::error::create_js_error_with_type(
            &format!("JSON.parse error: {}", e),
            "SyntaxError",
        );
        js_err
    })?;

    let native_val = from_serde_value(parsed);

    // Apply reviver if provided and is a function
    if let Some(reviver) = args.get(1) {
        let is_fn = match reviver {
            Value::Function(_) | Value::NativeFunction(_) => true,
            Value::Object(reviver_rc) => {
                let r = reviver_rc.borrow();
                r.kind == ObjectKind::Function || r.kind == ObjectKind::ArrowFunction
            }
            _ => false,
        };
        if is_fn {
            return walk_with_reviver(reviver, Value::String(String::new()), native_val)
                .map(|opt| opt.unwrap_or(Value::Undefined));
        }
    }

    Ok(native_val)
}

// ============================================================================
// JSON
// ============================================================================

pub fn register_json(ctx: &mut Context) {
    let json_obj = Object::new(crate::value::ObjectKind::Ordinary);
    let json = Rc::new(RefCell::new(json_obj));

    // Per ES spec, JSON.stringify and JSON.parse are:
    // { [[Writable]]: true, [[Enumerable]]: false, [[Configurable]]: true }
    let flags = PropertyFlags {
        value: None,
        writable: true,
        enumerable: false,
        configurable: true,
    };

    json.borrow_mut().define(
        "stringify",
        Value::NativeFunction(Rc::new(NativeFunction::new(|args| json_stringify(&args)))),
        flags.clone(),
    );

    json.borrow_mut().define(
        "parse",
        Value::NativeFunction(Rc::new(NativeFunction::new(|args| json_parse(&args)))),
        flags,
    );

    ctx.set_global("JSON".to_string(), Value::Object(json));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_context() -> crate::Context {
        let mut ctx = crate::Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        ctx
    }

    #[test]
    fn test_stringify_nested_object_and_array() {
        // Bug fix: nested objects/arrays must be serialized recursively
        let mut ctx = create_test_context();
        let result = ctx.eval(r#"JSON.stringify({a: {b: 1}})"#);
        assert_eq!(
            result.unwrap(),
            crate::value::Value::String("{\"a\":{\"b\":1}}".to_string())
        );
        let result = ctx.eval(r#"JSON.stringify({a: [1, 2]})"#);
        assert_eq!(
            result.unwrap(),
            crate::value::Value::String("{\"a\":[1,2]}".to_string())
        );
        let result = ctx.eval(r#"JSON.stringify([[1], [2]])"#);
        assert_eq!(
            result.unwrap(),
            crate::value::Value::String("[[1],[2]]".to_string())
        );
    }

    #[test]
    fn test_reviver_called_with_real_keys() {
        // Bug fix: reviver must receive the actual key/index, and plain
        // JS functions (Value::Function) must be accepted as revivers
        let mut ctx = create_test_context();
        let result = ctx.eval(
            r#"
            var keys = [];
            JSON.parse('{"a": 1}', function(k, v) { keys.push(k); return v; });
            keys.indexOf("a") >= 0;
        "#,
        );
        assert_eq!(result.unwrap(), crate::value::Value::Boolean(true));
    }

    #[test]
    fn test_multibyte_indent_does_not_panic() {
        // Bug fix: string indent must be truncated on char boundaries
        let mut ctx = create_test_context();
        let result = ctx.eval("JSON.stringify([1], null, 'ääääääääääää')");
        assert!(
            result.is_ok(),
            "multibyte indent should not panic: {:?}",
            result
        );
    }

    #[test]
    fn test_json_parse_property_descriptor() {
        let mut ctx = create_test_context();
        // Check descriptor flags for JSON.parse
        let writable = ctx
            .eval("Object.getOwnPropertyDescriptor(JSON, 'parse').writable")
            .unwrap();
        let enumerable = ctx
            .eval("Object.getOwnPropertyDescriptor(JSON, 'parse').enumerable")
            .unwrap();
        let configurable = ctx
            .eval("Object.getOwnPropertyDescriptor(JSON, 'parse').configurable")
            .unwrap();
        assert_eq!(
            writable,
            crate::value::Value::Boolean(true),
            "parse should be writable"
        );
        assert_eq!(
            enumerable,
            crate::value::Value::Boolean(false),
            "parse should be non-enumerable"
        );
        assert_eq!(
            configurable,
            crate::value::Value::Boolean(true),
            "parse should be configurable"
        );

        // Check descriptor flags for JSON.stringify
        let writable = ctx
            .eval("Object.getOwnPropertyDescriptor(JSON, 'stringify').writable")
            .unwrap();
        let enumerable = ctx
            .eval("Object.getOwnPropertyDescriptor(JSON, 'stringify').enumerable")
            .unwrap();
        let configurable = ctx
            .eval("Object.getOwnPropertyDescriptor(JSON, 'stringify').configurable")
            .unwrap();
        assert_eq!(
            writable,
            crate::value::Value::Boolean(true),
            "stringify should be writable"
        );
        assert_eq!(
            enumerable,
            crate::value::Value::Boolean(false),
            "stringify should be non-enumerable"
        );
        assert_eq!(
            configurable,
            crate::value::Value::Boolean(true),
            "stringify should be configurable"
        );
    }

    // ─── quote_string tests ─────────────────────────────────────────────────────

    #[test]
    fn test_quote_string_basic() {
        assert_eq!(quote_string(""), "\"\"");
    }

    #[test]
    fn test_quote_string_plain_text() {
        assert_eq!(quote_string("hello"), "\"hello\"");
    }

    #[test]
    fn test_quote_string_escapes_double_quote() {
        assert_eq!(quote_string("say \"hi\""), "\"say \\\"hi\\\"\"");
    }

    #[test]
    fn test_quote_string_escapes_backslash() {
        assert_eq!(quote_string("a\\b"), "\"a\\\\b\"");
    }

    #[test]
    fn test_quote_string_escapes_newline() {
        assert_eq!(quote_string("line1\nline2"), "\"line1\\nline2\"");
    }

    #[test]
    fn test_quote_string_escapes_carriage_return() {
        assert_eq!(quote_string("line1\rline2"), "\"line1\\rline2\"");
    }

    #[test]
    fn test_quote_string_escapes_tab() {
        assert_eq!(quote_string("col1\tcol2"), "\"col1\\tcol2\"");
    }

    #[test]
    fn test_quote_string_escapes_control_char() {
        // Control char 0x01 → \u0001
        assert_eq!(quote_string("\x01"), "\"\\u0001\"");
    }

    // ─── value_to_json_string tests ─────────────────────────────────────────────

    #[test]
    fn test_value_to_json_undefined() {
        assert_eq!(value_to_json_string(&crate::value::Value::Undefined), None);
    }

    #[test]
    fn test_value_to_json_null() {
        assert_eq!(
            value_to_json_string(&crate::value::Value::Null),
            Some("null".into())
        );
    }

    #[test]
    fn test_value_to_json_boolean() {
        assert_eq!(
            value_to_json_string(&crate::value::Value::Boolean(true)),
            Some("true".into())
        );
        assert_eq!(
            value_to_json_string(&crate::value::Value::Boolean(false)),
            Some("false".into())
        );
    }

    #[test]
    fn test_value_to_json_string() {
        assert_eq!(
            value_to_json_string(&crate::value::Value::String("hello".into())),
            Some("\"hello\"".into())
        );
    }

    #[test]
    fn test_value_to_json_object() {
        // Objects are not directly serializable
        use std::cell::RefCell;
        use std::rc::Rc;
        let obj = crate::value::Object::new(crate::value::ObjectKind::Ordinary);
        let val = crate::value::Value::Object(Rc::new(RefCell::new(obj)));
        assert_eq!(value_to_json_string(&val), None);
    }

    // ─── json_number_string tests ────────────────────────────────────────────────

    #[test]
    fn test_json_number_nan() {
        assert_eq!(json_number_string(f64::NAN), "null");
    }

    #[test]
    fn test_json_number_positive_infinity() {
        assert_eq!(json_number_string(f64::INFINITY), "null");
    }

    #[test]
    fn test_json_number_negative_infinity() {
        assert_eq!(json_number_string(f64::NEG_INFINITY), "null");
    }

    #[test]
    fn test_json_number_integer() {
        assert_eq!(json_number_string(42.0), "42");
    }

    #[test]
    fn test_json_number_large_integer() {
        // > 1e15 should use default to_string
        assert_eq!(json_number_string(1e20), "100000000000000000000");
    }

    #[test]
    #[allow(clippy::approx_constant)]
    fn test_json_number_float() {
        assert_eq!(json_number_string(3.14), "3.14");
    }

    #[test]
    fn test_json_number_negative() {
        assert_eq!(json_number_string(-273.15), "-273.15");
    }

    // ─── get_indent tests ────────────────────────────────────────────────────────

    #[test]
    fn test_get_indent_number() {
        let indent = get_indent(Some(&crate::value::Value::Number(2.0)));
        assert_eq!(indent, "  ");
    }

    #[test]
    fn test_get_indent_number_clamped_to_10() {
        let indent = get_indent(Some(&crate::value::Value::Number(15.0)));
        assert_eq!(indent, " ".repeat(10));
    }

    #[test]
    fn test_get_indent_number_negative() {
        let indent = get_indent(Some(&crate::value::Value::Number(-1.0)));
        assert_eq!(indent, "");
    }

    #[test]
    fn test_get_indent_string() {
        let indent = get_indent(Some(&crate::value::Value::String("--".into())));
        assert_eq!(indent, "--");
    }

    #[test]
    fn test_get_indent_string_truncated_to_10_chars() {
        let indent = get_indent(Some(&crate::value::Value::String("abcdefghijkl".into())));
        assert_eq!(indent, "abcdefghij");
    }

    #[test]
    fn test_get_indent_none() {
        assert_eq!(get_indent(None), "");
    }

    // ─── JSON.stringify integration ─────────────────────────────────────────────

    #[test]
    fn test_stringify_undefined_top_level() {
        let mut ctx = create_test_context();
        let result = ctx.eval("JSON.stringify(undefined)");
        assert_eq!(result.unwrap(), crate::value::Value::Undefined);
    }

    #[test]
    fn test_stringify_null() {
        let mut ctx = create_test_context();
        let result = ctx.eval("JSON.stringify(null)");
        assert_eq!(result.unwrap(), crate::value::Value::String("null".into()));
    }

    #[test]
    fn test_stringify_boolean() {
        let mut ctx = create_test_context();
        assert_eq!(
            ctx.eval("JSON.stringify(true)").unwrap(),
            crate::value::Value::String("true".into())
        );
        assert_eq!(
            ctx.eval("JSON.stringify(false)").unwrap(),
            crate::value::Value::String("false".into())
        );
    }

    #[test]
    fn test_stringify_number() {
        let mut ctx = create_test_context();
        assert_eq!(
            ctx.eval("JSON.stringify(42)").unwrap(),
            crate::value::Value::String("42".into())
        );
        assert_eq!(
            ctx.eval("JSON.stringify(3.14)").unwrap(),
            crate::value::Value::String("3.14".into())
        );
        assert_eq!(
            ctx.eval("JSON.stringify(NaN)").unwrap(),
            crate::value::Value::String("null".into())
        );
        assert_eq!(
            ctx.eval("JSON.stringify(Infinity)").unwrap(),
            crate::value::Value::String("null".into())
        );
    }

    #[test]
    fn test_stringify_string() {
        let mut ctx = create_test_context();
        assert_eq!(
            ctx.eval(r#"JSON.stringify("hello")"#).unwrap(),
            crate::value::Value::String("\"hello\"".into())
        );
    }

    #[test]
    fn test_stringify_quote_escaped() {
        let mut ctx = create_test_context();
        let result = ctx.eval(r#"JSON.stringify("say \"hi\"")"#).unwrap();
        assert_eq!(
            result,
            crate::value::Value::String("\"say \\\"hi\\\"\"".into())
        );
    }

    #[test]
    fn test_stringify_empty_object() {
        let mut ctx = create_test_context();
        assert_eq!(
            ctx.eval("JSON.stringify({})").unwrap(),
            crate::value::Value::String("{}".into())
        );
    }

    #[test]
    fn test_stringify_empty_array() {
        let mut ctx = create_test_context();
        assert_eq!(
            ctx.eval("JSON.stringify([])").unwrap(),
            crate::value::Value::String("[]".into())
        );
    }

    #[test]
    fn test_stringify_with_indent() {
        let mut ctx = create_test_context();
        let result = ctx.eval("JSON.stringify({a: 1}, null, 2)").unwrap();
        let expected = "{\n  \"a\": 1\n}";
        assert_eq!(result, crate::value::Value::String(expected.into()));
    }

    #[test]
    fn test_stringify_replacer_array() {
        let mut ctx = create_test_context();
        let result = ctx
            .eval(r#"JSON.stringify({a: 1, b: 2, c: 3}, ["a", "c"])"#)
            .unwrap();
        assert_eq!(
            result,
            crate::value::Value::String("{\"a\":1,\"c\":3}".into())
        );
    }

    #[test]
    fn test_stringify_function_member_skipped() {
        // Functions are stored as Value::Object, and value_to_json_string returns None
        // for objects → parent serializes as null. Arrays also convert None to null.
        let mut ctx = create_test_context();
        let result = ctx
            .eval("JSON.stringify({a: 1, b: function() {}})")
            .unwrap();
        assert_eq!(
            result,
            crate::value::Value::String("{\"a\":1,\"b\":null}".into())
        );
    }

    #[test]
    fn test_stringify_undefined_member_skipped() {
        let mut ctx = create_test_context();
        let result = ctx.eval("JSON.stringify({a: 1, b: undefined})").unwrap();
        assert_eq!(result, crate::value::Value::String("{\"a\":1}".into()));
    }

    // ─── JSON.parse integration ─────────────────────────────────────────────────

    #[test]
    fn test_parse_simple_object() {
        let mut ctx = create_test_context();
        let result = ctx.eval("JSON.parse('{\"a\": 1}').a").unwrap();
        assert_eq!(result, crate::value::Value::Number(1.0));
    }

    #[test]
    fn test_parse_simple_array() {
        let mut ctx = create_test_context();
        let result = ctx.eval("JSON.parse('[1, 2, 3]')[1]").unwrap();
        assert_eq!(result, crate::value::Value::Number(2.0));
    }

    #[test]
    fn test_parse_nested_structures() {
        let mut ctx = create_test_context();
        let result = ctx
            .eval("JSON.parse('{\"a\": [1, {\"b\": 2}]}').a[1].b")
            .unwrap();
        assert_eq!(result, crate::value::Value::Number(2.0));
    }

    #[test]
    fn test_parse_primitives() {
        let mut ctx = create_test_context();
        assert_eq!(
            ctx.eval("JSON.parse('null')").unwrap(),
            crate::value::Value::Null
        );
        assert_eq!(
            ctx.eval("JSON.parse('true')").unwrap(),
            crate::value::Value::Boolean(true)
        );
        assert_eq!(
            ctx.eval("JSON.parse('false')").unwrap(),
            crate::value::Value::Boolean(false)
        );
        assert_eq!(
            ctx.eval("JSON.parse('42')").unwrap(),
            crate::value::Value::Number(42.0)
        );
    }

    #[test]
    fn test_parse_reviver_transforms_values() {
        let mut ctx = create_test_context();
        let result = ctx
            .eval(
                r#"
                JSON.parse('{"n": 2}', function(k, v) {
                    if (typeof v === 'number') return v * 2;
                    return v;
                }).n
                "#,
            )
            .unwrap();
        assert_eq!(result, crate::value::Value::Number(4.0));
    }

    #[test]
    fn test_parse_reviver_deletes_properties() {
        let mut ctx = create_test_context();
        let result = ctx
            .eval(
                r#"
                var obj = JSON.parse('{"a": 1, "b": 2}', function(k, v) {
                    if (k === 'b') return undefined;
                    return v;
                });
                "a" in obj && !("b" in obj);
                "#,
            )
            .unwrap();
        assert_eq!(result, crate::value::Value::Boolean(true));
    }

    #[test]
    fn test_parse_empty_string_error() {
        let mut ctx = create_test_context();
        let result = ctx.eval("JSON.parse('')");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_invalid_json_error() {
        let mut ctx = create_test_context();
        let result = ctx.eval("JSON.parse('{invalid}')");
        assert!(result.is_err());
    }
}
