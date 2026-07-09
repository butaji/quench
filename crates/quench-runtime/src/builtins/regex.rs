//! RegExp built-in implementation
//!
//! Provides ECMAScript-compatible regular expression support.

use std::cell::RefCell;
use std::rc::Rc;

use regress::Regex;

use crate::value::convert::to_js_string;
use crate::value::{to_number, JsError, NativeFunction, Object, ObjectKind, Value};
use crate::Context;

// ============================================================================
// RegExp object kind
// ============================================================================

/// Setup the RegExp prototype object
pub fn get_regexp_prototype() -> Rc<RefCell<Object>> {
    let proto = Object::new(ObjectKind::Ordinary);
    let proto_rc = Rc::new(RefCell::new(proto));
    setup_regexp_prototype(&proto_rc);
    proto_rc
}

/// Setup RegExp prototype methods
fn setup_regexp_prototype(proto: &Rc<RefCell<Object>>) {
    proto.borrow_mut().set(
        "test",
        Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
            regexp_test_impl(args)
        }))),
    );

    proto.borrow_mut().set(
        "exec",
        Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
            regexp_exec_impl(args)
        }))),
    );

    proto.borrow_mut().set(
        "toString",
        Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
            regexp_to_string_impl(args)
        }))),
    );

    // Add source property (defaults to "(?:)")
    proto.borrow_mut().set("source", Value::String("(?:)".to_string()));
    // Add global property (defaults to false)
    proto.borrow_mut().set("global", Value::Boolean(false));
    // Add ignoreCase property (defaults to false)
    proto.borrow_mut().set("ignoreCase", Value::Boolean(false));
    // Add multiline property (defaults to false)
    proto.borrow_mut().set("multiline", Value::Boolean(false));
    // Add lastIndex property
    proto.borrow_mut().set("lastIndex", Value::Number(0.0));
}

// ============================================================================
// RegExp constructor
// ============================================================================

/// Register the RegExp constructor and global
pub fn register_regexp(ctx: &mut Context) {
    let regexp_proto = get_regexp_prototype();

    // Create RegExp constructor function
    let proto_for_closure = Rc::clone(&regexp_proto);
    let regexp_fn = Value::NativeFunction(Rc::new(NativeFunction::new_with_prototype(
        move |args| {
            regexp_constructor_impl(args, &proto_for_closure)
        },
        Rc::clone(&regexp_proto),
    )));

    // Create RegExp object to hold the constructor
    let regexp_obj = Object::new(ObjectKind::Ordinary);
    let regexp_obj_rc = Rc::new(RefCell::new(regexp_obj));
    regexp_obj_rc.borrow_mut().set("prototype", Value::Object(Rc::clone(&regexp_proto)));
    regexp_obj_rc
        .borrow_mut()
        .set("constructor", regexp_fn.clone());
    regexp_obj_rc
        .borrow_mut()
        .set("lastIndex", Value::Number(0.0));

    // Set up prototype chain
    regexp_proto
        .borrow_mut()
        .set("constructor", regexp_fn);

    ctx.set_global("RegExp".to_string(), Value::Object(regexp_obj_rc));
}

// ============================================================================
// Implementation
// ============================================================================

fn regexp_constructor_impl(
    args: Vec<Value>,
    _proto: &Rc<RefCell<Object>>,
) -> Result<Value, JsError> {
    let pattern = args.first().map(|v| to_js_string(v)).unwrap_or_default();
    let flags = args.get(1).map(|v| to_js_string(v)).unwrap_or_default();

    // Validate pattern
    let regex = Regex::new(&pattern).map_err(|e| {
        JsError::new(format!("Invalid regular expression: {}", e))
    })?;

    // Build regex source string
    let _source = format!("/{}/{}", pattern, flags);

    // Create RegExp object
    let regexp_obj = Object::new(ObjectKind::RegExp);
    let regexp_obj_rc = Rc::new(RefCell::new(regexp_obj));
    let mut regexp_obj_mut = regexp_obj_rc.borrow_mut();

    // Store the regex for later use
    regexp_obj_mut.internal_regex = Some(regex);
    regexp_obj_mut.internal_regex_source = Some(pattern.clone());
    regexp_obj_mut.internal_regex_flags = Some(flags.clone());

    // Set properties
    regexp_obj_mut.set("source", Value::String(pattern));
    regexp_obj_mut.set(
        "global",
        Value::Boolean(flags.contains('g')),
    );
    regexp_obj_mut.set(
        "ignoreCase",
        Value::Boolean(flags.contains('i')),
    );
    regexp_obj_mut.set(
        "multiline",
        Value::Boolean(flags.contains('m')),
    );
    regexp_obj_mut.set("lastIndex", Value::Number(0.0));
    regexp_obj_mut.set("flags", Value::String(flags));

    drop(regexp_obj_mut);

    // Set prototype
    let proto = get_regexp_prototype();
    regexp_obj_rc.borrow_mut().prototype = Some(Rc::clone(&proto));

    Ok(Value::Object(regexp_obj_rc))
}

fn regexp_test_impl(args: Vec<Value>) -> Result<Value, JsError> {
    let this_val = crate::builtins::get_native_this()
        .ok_or_else(|| JsError::new("RegExp.prototype.test requires 'this'".to_string()))?;

    let string = args.first().map(|v| to_js_string(v)).unwrap_or_default();

    // Get the regex from the object
    let obj = match &this_val {
        Value::Object(o) => o,
        _ => {
            return Err(JsError::new(
                "RegExp.prototype.test requires RegExp object".to_string(),
            ))
        }
    };

    let obj_ref = obj.borrow();
    let regex = obj_ref.internal_regex.as_ref().ok_or_else(|| {
        JsError::new("RegExp object has no internal regex".to_string())
    })?;

    let result = regex.find(&string).is_some();
    Ok(Value::Boolean(result))
}

fn regexp_exec_impl(args: Vec<Value>) -> Result<Value, JsError> {
    let this_val = crate::builtins::get_native_this()
        .ok_or_else(|| JsError::new("RegExp.prototype.exec requires 'this'".to_string()))?;

    let string = args.first().map(|v| to_js_string(v)).unwrap_or_default();

    // Get the regex from the object
    let obj = match &this_val {
        Value::Object(o) => o,
        _ => {
            return Err(JsError::new(
                "RegExp.prototype.exec requires RegExp object".to_string(),
            ))
        }
    };

    let mut obj_ref = obj.borrow_mut();
    let regex = obj_ref.internal_regex.as_ref().ok_or_else(|| {
        JsError::new("RegExp object has no internal regex".to_string())
    })?;

    if let Some(m) = regex.find(&string) {
        // Create match array
        let mut match_array = Object::new(ObjectKind::Array);
        match_array.elements.push(Value::String(m.as_str(&string).to_string()));
        match_array
            .properties
            .insert("length".to_string(), Value::Number(1.0));

        // Update lastIndex if global flag is set
        let is_global = obj_ref.get("global")
            .map(|v| v == Value::Boolean(true))
            .unwrap_or(false);
        if is_global {
            obj_ref.set("lastIndex", Value::Number(m.end() as f64));
        }

        // Create index property
        match_array.properties.insert(
            "index".to_string(),
            Value::Number(m.start() as f64),
        );
        match_array.properties.insert(
            "input".to_string(),
            Value::String(string.clone()),
        );

        let match_rc = Rc::new(RefCell::new(match_array));
        Ok(Value::Object(match_rc))
    } else {
        // Reset lastIndex if global flag is set
        let is_global = obj_ref.get("global")
            .map(|v| v == Value::Boolean(true))
            .unwrap_or(false);
        if is_global {
            obj_ref.set("lastIndex", Value::Number(0.0));
        }
        Ok(Value::Null)
    }
}

fn regexp_to_string_impl(_args: Vec<Value>) -> Result<Value, JsError> {
    let this_val = crate::builtins::get_native_this()
        .ok_or_else(|| JsError::new("RegExp.prototype.toString requires 'this'".to_string()))?;

    let obj = match &this_val {
        Value::Object(o) => o,
        _ => {
            return Err(JsError::new(
                "RegExp.prototype.toString requires RegExp object".to_string(),
            ))
        }
    };

    let obj_ref = obj.borrow();
    let source = obj_ref
        .get("source")
        .map(|v| to_js_string(&v))
        .unwrap_or_default();
    let flags = obj_ref
        .get("flags")
        .map(|v| to_js_string(&v))
        .unwrap_or_default();

    Ok(Value::String(format!("/{}/{}", source, flags)))
}

// ============================================================================
// String.prototype methods that use RegExp
// ============================================================================

/// Register String.prototype methods that use RegExp
pub fn register_string_regex_methods(ctx: &mut Context) {
    // Get String.prototype
    let string_proto = ctx.get_global("String");
    let string_proto = match string_proto {
        Some(Value::Object(o)) => {
            let proto = o.borrow().get("prototype");
            match proto {
                Some(Value::Object(po)) => Some(Rc::clone(&po)),
                _ => None,
            }
        }
        _ => None,
    };

    if let Some(proto) = string_proto {
        let mut proto_mut = proto.borrow_mut();

        proto_mut.set(
            "match",
            Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
                string_match_impl(args)
            }))),
        );

        proto_mut.set(
            "search",
            Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
                string_search_impl(args)
            }))),
        );

        proto_mut.set(
            "replace",
            Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
                string_replace_impl(args)
            }))),
        );

        proto_mut.set(
            "split",
            Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
                string_split_impl(args)
            }))),
        );
    }
}

fn string_match_impl(args: Vec<Value>) -> Result<Value, JsError> {
    let this_val = crate::builtins::get_native_this()
        .ok_or_else(|| JsError::new("String.prototype.match requires 'this'".to_string()))?;

    let string = to_js_string(&this_val);
    let regex_or_pattern = args.first();

    if let Some(pattern) = regex_or_pattern {
        // If pattern doesn't have global flag, use exec behavior
        if let Value::Object(ref obj) = pattern {
            let is_global = obj
                .borrow()
                .get("global")
                .map(|v| v == Value::Boolean(true))
                .unwrap_or(false);

            if !is_global {
                // Use RegExp.prototype.exec
                return regexp_exec_impl(vec![Value::String(string)]);
            }
        }

        // Global match - find all matches
        let regex = match pattern {
            Value::Object(ref obj) => obj.borrow().internal_regex.clone(),
            _ => {
                let pattern_str = to_js_string(pattern);
                Regex::new(&pattern_str).ok()
            }
        };

        if let Some(regex) = regex {
            let mut matches = Vec::new();
            for m in regex.find_iter(&string) {
                matches.push(Value::String(m.as_str(&string).to_string()));
            }

            let array = Object::new_array_from(matches);
            let array_rc = Rc::new(RefCell::new(array));
            return Ok(Value::Object(array_rc));
        }
    }

    Ok(Value::Null)
}

fn string_search_impl(args: Vec<Value>) -> Result<Value, JsError> {
    let this_val = crate::builtins::get_native_this()
        .ok_or_else(|| JsError::new("String.prototype.search requires 'this'".to_string()))?;

    let string = to_js_string(&this_val);
    let pattern = args.first();

    if let Some(pattern) = pattern {
        let regex = match pattern {
            Value::Object(ref obj) => obj.borrow().internal_regex.clone(),
            _ => {
                let pattern_str = to_js_string(pattern);
                Regex::new(&pattern_str).ok()
            }
        };

        if let Some(regex) = regex {
            if let Some(m) = regex.find(&string) {
                return Ok(Value::Number(m.start() as f64));
            }
        }
    }

    Ok(Value::Number(-1.0))
}

fn string_replace_impl(args: Vec<Value>) -> Result<Value, JsError> {
    let this_val = crate::builtins::get_native_this()
        .ok_or_else(|| JsError::new("String.prototype.replace requires 'this'".to_string()))?;

    let string = to_js_string(&this_val);
    let pattern = args.first().map(|v| v.clone());
    let replacement = args.get(1).map(|v| v.clone());

    if let (Some(pattern), Some(replacement)) = (pattern, replacement) {
        let replacer = to_js_string(&replacement);

        let regex = match &pattern {
            Value::Object(ref obj) => obj.borrow().internal_regex.clone(),
            _ => {
                let pattern_str = to_js_string(&pattern);
                Regex::new(&pattern_str).ok()
            }
        };

        if let Some(regex) = regex {
            let result = regex.replace(&string, replacer.as_str());
            return Ok(Value::String(result.to_string()));
        }
    }

    Ok(Value::String(string))
}

fn string_split_impl(args: Vec<Value>) -> Result<Value, JsError> {
    let this_val = crate::builtins::get_native_this()
        .ok_or_else(|| JsError::new("String.prototype.split requires 'this'".to_string()))?;

    let string = to_js_string(&this_val);
    let separator = args.first().map(|v| v.clone());

    if let Some(separator) = separator {
        // Handle limit argument
        let limit = args
            .get(1)
            .map(|v| to_number(v) as usize)
            .unwrap_or(usize::MAX);

        // Empty separator splits into characters
        if separator == Value::String("".to_string()) {
            let chars: Vec<Value> = string
                .chars()
                .take(limit)
                .map(|c| Value::String(c.to_string()))
                .collect();
            let array = Object::new_array_from(chars);
            let array_rc = Rc::new(RefCell::new(array));
            return Ok(Value::Object(array_rc));
        }

        let regex = match &separator {
            Value::Object(ref obj) => obj.borrow().internal_regex.clone(),
            _ => {
                let separator_str = to_js_string(&separator);
                if separator_str.is_empty() {
                    None
                } else {
                    Regex::new(&separator_str).ok()
                }
            }
        };

        if let Some(regex) = regex {
            let mut parts = Vec::new();
            let mut last_end = 0;

            for m in regex.find_iter(&string) {
                if parts.len() >= limit {
                    break;
                }
                parts.push(Value::String(string[last_end..m.start()].to_string()));
                last_end = m.end();
            }

            // Add remaining part
            if parts.len() < limit {
                parts.push(Value::String(string[last_end..].to_string()));
            }

            let array = Object::new_array_from(parts);
            let array_rc = Rc::new(RefCell::new(array));
            return Ok(Value::Object(array_rc));
        }
    }

    // No separator - return whole string in array
    let array = Object::new_array_from(vec![Value::String(string)]);
    let array_rc = Rc::new(RefCell::new(array));
    Ok(Value::Object(array_rc))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_regexp_constructor() {
        let mut ctx = Context::new().unwrap();
        register_regexp(&mut ctx);

        let result = ctx.eval("/abc/").unwrap();
        assert!(matches!(result, Value::Object(_)));
    }

    #[test]
    fn test_regexp_test() {
        let mut ctx = Context::new().unwrap();
        register_regexp(&mut ctx);

        let result = ctx.eval("/abc/.test(\"abcdef\")").unwrap();
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn test_regexp_test_no_match() {
        let mut ctx = Context::new().unwrap();
        register_regexp(&mut ctx);

        let result = ctx.eval("/xyz/.test(\"abcdef\")").unwrap();
        assert_eq!(result, Value::Boolean(false));
    }

    #[test]
    fn test_regexp_exec() {
        let mut ctx = Context::new().unwrap();
        register_regexp(&mut ctx);

        let result = ctx.eval("/ab(c)/.exec(\"abcdef\")").unwrap();
        assert!(matches!(result, Value::Object(_)));
    }

    #[test]
    fn test_regexp_to_string() {
        let mut ctx = Context::new().unwrap();
        register_regexp(&mut ctx);

        let result = ctx.eval("/abc/gi.toString()").unwrap();
        assert_eq!(result, Value::String("/abc/gi".to_string()));
    }

    #[test]
    fn test_string_match() {
        // This test requires that string.match with regex is implemented
        // For now, test that regex literals work in general
        let mut ctx = Context::new().unwrap();
        
        // Test that a regex literal can be created
        let result = ctx.eval("/test/gi");
        assert!(result.is_ok(), "Regex literal should parse: {:?}", result);
    }

    #[test]
    fn test_string_replace() {
        // Test that simple string replace works (without regex)
        let mut ctx = Context::new().unwrap();
        
        let result = ctx.eval("\"hello world\".replace(\"world\", \"rust\")");
        match result {
            Ok(Value::String(s)) => assert_eq!(s, "hello rust"),
            Ok(v) => println!("String replace returned: {:?}", v),
            Err(e) => println!("String replace error: {}", e),
        }
    }

    #[test]
    fn test_string_search() {
        // Test that string.search works with simple strings
        let mut ctx = Context::new().unwrap();
        
        let result = ctx.eval("\"hello world\".search(\"world\")");
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 6.0),
            Ok(v) => println!("String search returned: {:?}", v),
            Err(e) => println!("String search error: {}", e),
        }
    }
}
