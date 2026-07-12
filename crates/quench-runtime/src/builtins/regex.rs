//! RegExp built-in implementation
//!
//! Provides ECMAScript-compatible regular expression support.

mod string_methods;

use std::cell::RefCell;
use std::rc::Rc;

use regress::Regex;

use crate::value::convert::to_js_string;
use crate::value::{JsError, NativeFunction, Object, ObjectKind, Value};
use crate::Context;

pub use string_methods::register_string_regex_methods;

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
        Value::NativeFunction(Rc::new(NativeFunction::new(regexp_test_impl))),
    );

    proto.borrow_mut().set(
        "exec",
        Value::NativeFunction(Rc::new(NativeFunction::new(regexp_exec_impl))),
    );

    proto.borrow_mut().set(
        "toString",
        Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
            regexp_to_string_impl(args)
        }))),
    );

    // Add source property (defaults to "(?:)")
    proto
        .borrow_mut()
        .set("source", Value::String("(?:)".to_string()));
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
        move |args| regexp_constructor_impl(args, &proto_for_closure),
        Rc::clone(&regexp_proto),
    )));

    // Create RegExp object to hold the constructor
    let regexp_obj = Object::new(ObjectKind::Ordinary);
    let regexp_obj_rc = Rc::new(RefCell::new(regexp_obj));
    regexp_obj_rc
        .borrow_mut()
        .set("prototype", Value::Object(Rc::clone(&regexp_proto)));
    regexp_obj_rc
        .borrow_mut()
        .set("constructor", regexp_fn.clone());
    regexp_obj_rc
        .borrow_mut()
        .set("lastIndex", Value::Number(0.0));

    // Set up prototype chain
    regexp_proto.borrow_mut().set("constructor", regexp_fn);

    ctx.set_global("RegExp".to_string(), Value::Object(regexp_obj_rc));
}

// ============================================================================
// Implementation
// ============================================================================

fn regexp_constructor_impl(
    args: Vec<Value>,
    regexp_proto: &Rc<RefCell<Object>>,
) -> Result<Value, JsError> {
    let pattern = args.first().map(to_js_string).unwrap_or_default();
    let flags = args.get(1).map(to_js_string).unwrap_or_default();

    // Validate flags: unique characters from the valid set
    let mut seen = std::collections::HashSet::new();
    for c in flags.chars() {
        if !"dgimsuvy".contains(c) || !seen.insert(c) {
            return Err(JsError::new(format!(
                "SyntaxError: Invalid regular expression flags '{}'",
                flags
            )));
        }
    }

    // Compile the pattern (regress understands the i, m, s, u flags)
    let compile_flags: String = flags.chars().filter(|c| "imsu".contains(*c)).collect();
    let compiled = Regex::with_flags(&pattern, compile_flags.as_str())
        .map_err(|e| JsError::new(format!("Invalid regular expression: {}", e)))?;

    // Check if called with 'new' - use the passed-in object
    let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
    if let Value::Object(obj_rc) = this_val {
        // Called with 'new' - configure the passed object
        let mut obj = obj_rc.borrow_mut();
        obj.kind = ObjectKind::RegExp;
        obj.internal_regex_source = Some(pattern.clone());
        obj.internal_regex_flags = Some(flags.clone());
        obj.set("source", Value::String(pattern.clone()));
        obj.set("global", Value::Boolean(flags.contains('g')));
        obj.set("ignoreCase", Value::Boolean(flags.contains('i')));
        obj.set("multiline", Value::Boolean(flags.contains('m')));
        obj.set("lastIndex", Value::Number(0.0));
        obj.set("flags", Value::String(flags.clone()));
        obj.internal_regex = Some(compiled);
        Ok(Value::Object(Rc::clone(&obj_rc)))
    } else {
        // Direct call: RegExp() - create and return new object
        let mut obj = Object::new(ObjectKind::RegExp);
        obj.internal_regex_source = Some(pattern.clone());
        obj.internal_regex_flags = Some(flags.clone());
        obj.set("source", Value::String(pattern));
        obj.set("global", Value::Boolean(flags.contains('g')));
        obj.set("ignoreCase", Value::Boolean(flags.contains('i')));
        obj.set("multiline", Value::Boolean(flags.contains('m')));
        obj.set("lastIndex", Value::Number(0.0));
        obj.set("flags", Value::String(flags.clone()));
        obj.internal_regex = Some(compiled);
        obj.prototype = Some(Rc::clone(regexp_proto));
        Ok(Value::Object(Rc::new(RefCell::new(obj))))
    }
}

/// Read the flags and lastIndex of a RegExp object, and whether it is
/// global or sticky (the modes that consult and update lastIndex).
fn regexp_match_state(obj: &Rc<RefCell<Object>>) -> (String, bool, bool) {
    let flags = obj
        .borrow()
        .internal_regex_flags
        .clone()
        .unwrap_or_default();
    let is_global_or_sticky = flags.contains('g') || flags.contains('y');
    let is_sticky = flags.contains('y');
    (flags, is_global_or_sticky, is_sticky)
}

/// Find the next match, honoring lastIndex for global/sticky regexes.
/// Returns the match and updates lastIndex per spec (end of match on
/// success, 0 on failure; untouched for non-global regexes).
fn regexp_find(obj: &Rc<RefCell<Object>>, regex: &Regex, haystack: &str) -> Option<regress::Match> {
    let (_flags, is_global_or_sticky, is_sticky) = regexp_match_state(obj);
    if !is_global_or_sticky {
        return regex.find(haystack);
    }
    let mut start = obj
        .borrow()
        .get("lastIndex")
        .map(|v| crate::value::to_number(&v) as usize)
        .unwrap_or(0);
    if start > haystack.len() {
        obj.borrow_mut().set("lastIndex", Value::Number(0.0));
        return None;
    }
    // Floor to a char boundary so user-set lastIndex can't panic
    while start > 0 && !haystack.is_char_boundary(start) {
        start -= 1;
    }
    let m = regex
        .find_from(haystack, start)
        .next()
        .filter(|m| !is_sticky || m.start() == start);
    match m {
        Some(m) => {
            obj.borrow_mut()
                .set("lastIndex", Value::Number(m.end() as f64));
            Some(m)
        }
        None => {
            obj.borrow_mut().set("lastIndex", Value::Number(0.0));
            None
        }
    }
}

fn regexp_test_impl(args: Vec<Value>) -> Result<Value, JsError> {
    let this_val = crate::builtins::get_native_this()
        .ok_or_else(|| JsError::new("RegExp.prototype.test requires 'this'".to_string()))?;

    if let Value::Object(ref obj) = this_val {
        let test_string = args.first().map(to_js_string).unwrap_or_default();
        let regex = obj.borrow().internal_regex.clone().or_else(|| {
            obj.borrow()
                .internal_regex_source
                .as_ref()
                .and_then(|s| Regex::new(s).ok())
        });

        if let Some(ref regex) = regex {
            if regexp_find(obj, regex, &test_string).is_some() {
                return Ok(Value::Boolean(true));
            }
        }
        Ok(Value::Boolean(false))
    } else {
        Err(JsError::new(
            "RegExp.prototype.test requires RegExp 'this'".to_string(),
        ))
    }
}

pub(crate) fn regexp_exec_impl(args: Vec<Value>) -> Result<Value, JsError> {
    let this_val = crate::builtins::get_native_this()
        .ok_or_else(|| JsError::new("RegExp.prototype.exec requires 'this'".to_string()))?;

    if let Value::Object(ref obj) = this_val {
        let search_string = args.first().map(to_js_string).unwrap_or_default();
        let regex = obj.borrow().internal_regex.clone().or_else(|| {
            obj.borrow()
                .internal_regex_source
                .as_ref()
                .and_then(|s| Regex::new(s).ok())
        });

        if let Some(ref regex) = regex {
            if let Some(m) = regexp_find(obj, regex, &search_string) {
                let result = build_exec_result(&search_string, &m, regex);
                return Ok(result);
            }
        }
        Ok(Value::Null)
    } else {
        Err(JsError::new(
            "RegExp.prototype.exec requires RegExp 'this'".to_string(),
        ))
    }
}

/// Build the result array from a regex match.
fn build_exec_result(search_string: &str, m: &regress::Match, _regex: &Regex) -> Value {
    let mut matches = vec![Value::String(m.as_str(search_string).to_string())];
    for i in 1.. {
        if let Some(range) = m.group(i) {
            matches.push(Value::String(
                search_string[range.start..range.end].to_string(),
            ));
        } else {
            break;
        }
    }
    let result = Object::new_array_from(matches);
    let result_rc = Rc::new(RefCell::new(result));
    result_rc
        .borrow_mut()
        .set("index", Value::Number(m.start() as f64));
    result_rc
        .borrow_mut()
        .set("input", Value::String(search_string.to_string()));
    Value::Object(result_rc)
}

fn regexp_to_string_impl(_args: Vec<Value>) -> Result<Value, JsError> {
    let this_val = crate::builtins::get_native_this()
        .ok_or_else(|| JsError::new("RegExp.prototype.toString requires 'this'".to_string()))?;

    if let Value::Object(ref obj) = this_val {
        let source = obj
            .borrow()
            .internal_regex_source
            .clone()
            .unwrap_or_default();
        let flags = obj
            .borrow()
            .internal_regex_flags
            .clone()
            .unwrap_or_default();

        Ok(Value::String(format!("/{}/{}", source, flags)))
    } else {
        Err(JsError::new(
            "RegExp.prototype.toString requires RegExp 'this'".to_string(),
        ))
    }
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
    fn test_regexp_invalid_flags_throw_syntax_error() {
        let mut ctx = Context::new().unwrap();
        register_regexp(&mut ctx);

        let result = ctx.eval("new RegExp('a', 'zz')");
        assert!(result.is_err(), "invalid flags must throw");
        assert!(result.unwrap_err().0.contains("SyntaxError"));
        let dup = ctx.eval("new RegExp('a', 'gg')");
        assert!(dup.is_err(), "duplicate flags must throw");
    }

    #[test]
    fn test_regexp_global_test_advances_last_index() {
        let mut ctx = Context::new().unwrap();
        register_regexp(&mut ctx);

        // Global regex: successive test() calls start from lastIndex
        assert_eq!(
            ctx.eval("var re = /a/g; re.test('aa')").unwrap(),
            Value::Boolean(true)
        );
        assert_eq!(ctx.eval("re.lastIndex").unwrap(), Value::Number(1.0));
        assert_eq!(ctx.eval("re.test('aa')").unwrap(), Value::Boolean(true));
        assert_eq!(ctx.eval("re.lastIndex").unwrap(), Value::Number(2.0));
        // Failure resets lastIndex to 0
        assert_eq!(ctx.eval("re.test('aa')").unwrap(), Value::Boolean(false));
        assert_eq!(ctx.eval("re.lastIndex").unwrap(), Value::Number(0.0));
    }

    #[test]
    fn test_regexp_non_global_does_not_touch_last_index() {
        let mut ctx = Context::new().unwrap();
        register_regexp(&mut ctx);

        assert_eq!(
            ctx.eval("var re2 = /a/; re2.lastIndex = 5; re2.test('cat')")
                .unwrap(),
            Value::Boolean(true)
        );
        assert_eq!(ctx.eval("re2.lastIndex").unwrap(), Value::Number(5.0));
    }

    #[test]
    fn test_regexp_global_exec_starts_from_last_index() {
        let mut ctx = Context::new().unwrap();
        register_regexp(&mut ctx);

        ctx.eval("var re3 = /b/g; re3.lastIndex = 2;").unwrap();
        let result = ctx.eval("re3.exec('abcabc').index").unwrap();
        assert_eq!(result, Value::Number(4.0));
        assert_eq!(ctx.eval("re3.lastIndex").unwrap(), Value::Number(5.0));
    }
}
