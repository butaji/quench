//! String.prototype methods that use RegExp

use std::cell::RefCell;
use std::rc::Rc;

use regress::Regex;

use crate::value::convert::to_js_string;
use crate::value::{to_number, JsError, NativeFunction, Object, Value};
use crate::Context;

/// Register String.prototype methods that use RegExp
pub fn register_string_regex_methods(ctx: &mut Context) {
    if let Some(proto) = get_string_prototype(ctx) {
        let mut proto_mut = proto.borrow_mut();
        proto_mut.set(
            "match",
            Value::NativeFunction(Rc::new(NativeFunction::new(string_match_impl))),
        );
        proto_mut.set(
            "search",
            Value::NativeFunction(Rc::new(NativeFunction::new(string_search_impl))),
        );
        proto_mut.set(
            "replace",
            Value::NativeFunction(Rc::new(NativeFunction::new(string_replace_impl))),
        );
        proto_mut.set(
            "split",
            Value::NativeFunction(Rc::new(NativeFunction::new(string_split_impl))),
        );
        proto_mut.set(
            "replaceAll",
            Value::NativeFunction(Rc::new(NativeFunction::new(string_replace_all_impl))),
        );
    }
}

fn get_string_prototype(ctx: &mut Context) -> Option<Rc<RefCell<Object>>> {
    let string_proto = ctx.get_global("String")?;
    if let Value::Object(o) = string_proto {
        if let Some(Value::Object(po)) = o.borrow().get("prototype") {
            return Some(Rc::clone(&po));
        }
    }
    None
}

/// Apply $-substitution to replacement string based on match info.
/// Handles: $$ -> $, $& -> matched, $` -> before match, $' -> after match, $n -> capture n
/// Per ECMAScript spec: for unrecognized $X where X is not one of the above,
/// the $ is kept literal and X is processed normally.
fn apply_substitution(
    replacement: &str,
    matched: &str,
    before: &str,
    after: &str,
    captures: &[&str],
) -> String {
    let mut result = String::new();
    let mut chars = replacement.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '$' {
            if let Some(&next) = chars.peek() {
                match next {
                    '$' => {
                        chars.next();
                        result.push('$');
                    }
                    '&' => {
                        chars.next();
                        result.push_str(matched);
                    }
                    '`' => {
                        chars.next();
                        result.push_str(before);
                    }
                    '\'' => {
                        chars.next();
                        result.push_str(after);
                    }
                    '0'..='9' => {
                        chars.next();
                        // Collect the full number after $
                        let mut num_str = String::new();
                        num_str.push(next);
                        while let Some(&peeked) = chars.peek() {
                            if peeked.is_ascii_digit() {
                                chars.next();
                                num_str.push(peeked);
                            } else {
                                break;
                            }
                        }
                        let n: usize = num_str.parse().unwrap_or(0);
                        result.push_str(&handle_dollar_n(captures, n));
                    }
                    _ => {
                        // Non-special character after $, emit $ literally
                        // and let the next char be processed normally
                        result.push(c);
                    }
                }
            } else {
                // Trailing $, emit literally
                result.push(c);
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Handle $n replacement patterns (1-99)
/// Per ECMAScript spec: $n where n is 1-99 refers to the nth captured group.
/// If n > number of captures, use "$n" literally.
fn handle_dollar_n(captures: &[&str], n: usize) -> String {
    if n > 0 && n <= captures.len() {
        captures[n - 1].to_string()
    } else {
        format!("${}", n)
    }
}

pub(crate) fn string_match_impl(args: Vec<Value>) -> Result<Value, JsError> {
    let this_val = crate::builtins::get_native_this()
        .ok_or_else(|| JsError::new("String.prototype.match requires 'this'".to_string()))?;

    let string = to_js_string(&this_val);
    let pattern = args.first();

    if let Some(pattern) = pattern {
        return match_all_impl(&string, pattern);
    }
    Ok(Value::Null)
}

fn match_all_impl(string: &str, pattern: &Value) -> Result<Value, JsError> {
    // Check if pattern is a RegExp object
    if let Value::Object(ref obj) = pattern {
        if let Some(regex) = obj.borrow().internal_regex.clone() {
            let is_global = obj
                .borrow()
                .get("global")
                .map(|v| v == Value::Boolean(true))
                .unwrap_or(false);

            if !is_global {
                return super::regexp_exec_impl(vec![Value::String(string.to_string())]);
            }
            return Ok(make_match_array(string, &regex));
        }
    }

    // String pattern: convert to regex
    let pattern_str = to_js_string(pattern);
    if let Ok(regex) = Regex::new(&pattern_str) {
        return Ok(make_match_array(string, &regex));
    }
    Ok(Value::Null)
}

fn make_match_array(string: &str, regex: &Regex) -> Value {
    let matches: Vec<Value> = regex
        .find_iter(string)
        .map(|m| Value::String(m.as_str(string).to_string()))
        .collect();

    if matches.is_empty() {
        return Value::Null;
    }
    let array = Object::new_array_from(matches);
    Value::Object(Rc::new(RefCell::new(array)))
}

pub(crate) fn string_search_impl(args: Vec<Value>) -> Result<Value, JsError> {
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

pub(crate) fn string_replace_impl(args: Vec<Value>) -> Result<Value, JsError> {
    let this_val = crate::builtins::get_native_this()
        .ok_or_else(|| JsError::new("String.prototype.replace requires 'this'".to_string()))?;

    let string = to_js_string(&this_val);
    let pattern = args.first().cloned();
    let replacement = args.get(1).cloned();

    if let (Some(pattern), Some(replacement)) = (pattern, replacement) {
        let regex = match &pattern {
            Value::Object(ref obj) => obj.borrow().internal_regex.clone(),
            _ => {
                let pattern_str = to_js_string(&pattern);
                Regex::new(&pattern_str).ok()
            }
        };

        if let Some(regex) = regex {
            // Global RegExp replaces every match
            let is_global = match &pattern {
                Value::Object(ref obj) => obj
                    .borrow()
                    .get("global")
                    .map(|v| v == Value::Boolean(true))
                    .unwrap_or(false),
                _ => false,
            };
            if is_global {
                return Ok(Value::String(replace_all_with_value(
                    &string,
                    &regex,
                    &replacement,
                )));
            }
            // Find first match
            if let Some(m) = regex.find(&string) {
                let start = m.start();
                let end = m.end();
                let matched = &string[start..end];
                let before = &string[..start];
                let after = &string[end..];

                let captures = match_captures(&m, &string);
                let replaced = replace_using_value(
                    matched,
                    &before,
                    &after,
                    &captures,
                    start as f64,
                    &string,
                    &replacement,
                )?;
                let result = format!("{}{}{}", before, replaced, after);
                return Ok(Value::String(result));
            }
        }
    }

    Ok(Value::String(string))
}

/// Perform replacement using a Value (either string or function)
fn replace_using_value(
    matched: &str,
    before: &str,
    after: &str,
    captures: &[&str],
    position: f64,
    string: &str,
    replacement: &Value,
) -> Result<String, JsError> {
    // If replacement is a function, call it
    if matches!(
        replacement,
        Value::Function(_) | Value::NativeFunction(_) | Value::NativeConstructor(_)
    ) {
        let args: Vec<Value> = std::iter::once(Value::String(matched.to_string()))
            .chain(captures.iter().map(|s| Value::String(s.to_string())))
            .chain([Value::Number(position), Value::String(string.to_string())])
            .collect();

        let result =
            crate::eval::call_value_with_this(replacement.clone(), args, Value::Undefined)?;
        Ok(to_js_string(&result))
    } else {
        // String replacement with $-substitution
        let replacer = to_js_string(replacement);
        Ok(apply_substitution(
            &replacer, matched, before, after, captures,
        ))
    }
}

/// Replace all matches using a Value (either string or function)
fn replace_all_with_value(string: &str, regex: &Regex, replacement: &Value) -> String {
    if matches!(
        replacement,
        Value::Function(_) | Value::NativeFunction(_) | Value::NativeConstructor(_)
    ) {
        let mut result = String::new();
        let mut last_end = 0;

        for m in regex.find_iter(string) {
            let start = m.start();
            let end = m.end();
            let matched = &string[start..end];
            let before = &string[..start];
            let after = &string[end..];
            let captures = match_captures(&m, string);

            // For now, call the function without proper 'this' handling
            // (this should use the global object or undefined in strict mode)
            let args: Vec<Value> = std::iter::once(Value::String(matched.to_string()))
                .chain(captures.iter().map(|s| Value::String(s.to_string())))
                .chain([
                    Value::Number(start as f64),
                    Value::String(string.to_string()),
                ])
                .collect();

            let func_result =
                crate::eval::call_value_with_this(replacement.clone(), args, Value::Undefined);
            let replaced = match func_result {
                Ok(val) => to_js_string(&val),
                Err(_) => matched.to_string(), // On error, keep original
            };

            result.push_str(&string[last_end..start]);
            result.push_str(&replaced);
            last_end = end;
        }
        result.push_str(&string[last_end..]);
        result
    } else {
        let replacer = to_js_string(replacement);
        replace_all_matches(string, regex, &replacer)
    }
}

pub(crate) fn string_replace_all_impl(args: Vec<Value>) -> Result<Value, JsError> {
    let this_val = crate::builtins::get_native_this()
        .ok_or_else(|| JsError::new("String.prototype.replaceAll requires 'this'".to_string()))?;

    let string = to_js_string(&this_val);
    let pattern = args.first().cloned();
    let replacement = args.get(1).cloned();

    if let (Some(pattern), Some(replacement)) = (pattern, replacement) {
        return replace_all_inner(&string, &pattern, &replacement);
    }
    Ok(Value::String(string))
}

fn replace_all_inner(string: &str, pattern: &Value, replacement: &Value) -> Result<Value, JsError> {
    // Check if pattern is a RegExp with global flag
    if let Value::Object(ref obj) = pattern {
        if obj.borrow().internal_regex.is_some() {
            let has_global = obj
                .borrow()
                .get("global")
                .map(|v| v == Value::Boolean(true))
                .unwrap_or(false);
            if !has_global {
                return Err(JsError::new(
                    "TypeError: String.prototype.replaceAll called with non-global RegExp"
                        .to_string(),
                ));
            }
        }
    }

    let pattern_str = to_js_string(pattern);

    // Handle empty string pattern FIRST - before regex creation
    if pattern_str.is_empty() {
        // For empty pattern, replacement must be a string (per spec)
        let replacer = to_js_string(replacement);
        return Ok(Value::String(replace_all_empty(string, &replacer)));
    }

    let regex = match pattern {
        Value::Object(ref obj) => obj.borrow().internal_regex.clone(),
        _ => Regex::new(&pattern_str).ok(),
    };

    if let Some(regex) = regex {
        let result = replace_all_with_value(string, &regex, replacement);
        return Ok(Value::String(result));
    }
    Ok(Value::String(string.to_string()))
}

fn replace_all_empty(string: &str, replacer: &str) -> String {
    // Per ECMAScript spec, replaceAll with empty string pattern
    // inserts replacement at the beginning, between each character, and at the end
    // For "abc" with "-" replacement: "-a-b-c-"
    let mut result = String::new();
    if string.is_empty() {
        return result;
    }
    result.push_str(replacer);
    for (i, c) in string.chars().enumerate() {
        if i > 0 {
            result.push_str(replacer);
        }
        result.push(c);
    }
    result.push_str(replacer);
    result
}

fn replace_all_matches(string: &str, regex: &Regex, replacer: &str) -> String {
    let mut result = String::new();
    let mut last_end = 0;

    for m in regex.find_iter(string) {
        let start = m.start();
        let end = m.end();
        let matched = &string[start..end];

        result.push_str(&string[last_end..start]);

        let before = &string[..start];
        let after = &string[end..];
        let captures = match_captures(&m, string);
        let replaced = apply_substitution(replacer, matched, before, after, &captures);
        result.push_str(&replaced);

        last_end = end;
    }
    result.push_str(&string[last_end..]);
    result
}

/// Extract capture-group strings from a match (group 0 excluded; unmatched
/// groups become empty strings).
fn match_captures<'a>(m: &regress::Match, string: &'a str) -> Vec<&'a str> {
    m.captures
        .iter()
        .map(|c| c.as_ref().map(|r| &string[r.clone()]).unwrap_or(""))
        .collect()
}

pub(crate) fn string_split_impl(args: Vec<Value>) -> Result<Value, JsError> {
    let this_val = crate::builtins::get_native_this()
        .ok_or_else(|| JsError::new("String.prototype.split requires 'this'".to_string()))?;

    let string = to_js_string(&this_val);
    let separator = args.first().cloned();

    if let Some(separator) = separator {
        let limit = args
            .get(1)
            .map(|v| to_number(v) as usize)
            .unwrap_or(usize::MAX);
        if separator == Value::String("".to_string()) {
            let chars: Vec<Value> = string
                .chars()
                .take(limit)
                .map(|c| Value::String(c.to_string()))
                .collect();
            return Ok(make_value_array(chars));
        }
        if let Some(regex) = get_separator_regex(&separator) {
            return Ok(split_by_regex(&string, &regex, limit));
        }
    }
    Ok(make_value_array(vec![Value::String(string)]))
}

fn get_separator_regex(separator: &Value) -> Option<Regex> {
    match separator {
        Value::Object(ref obj) => obj.borrow().internal_regex.clone(),
        _ => {
            let sep_str = to_js_string(separator);
            if sep_str.is_empty() {
                None
            } else {
                Regex::new(&sep_str).ok()
            }
        }
    }
}

fn split_by_regex(string: &str, regex: &Regex, limit: usize) -> Value {
    let mut parts = Vec::new();
    let mut last_end = 0;

    for m in regex.find_iter(string) {
        if parts.len() >= limit {
            break;
        }
        parts.push(Value::String(string[last_end..m.start()].to_string()));
        last_end = m.end();
    }

    if parts.len() < limit {
        parts.push(Value::String(string[last_end..].to_string()));
    }
    make_value_array(parts)
}

fn make_value_array(values: Vec<Value>) -> Value {
    let array = Object::new_array_from(values);
    Value::Object(Rc::new(RefCell::new(array)))
}

#[cfg(test)]
mod tests {
    use crate::value::Value;
    use crate::Context;

    // Tests for match()
    #[test]
    fn test_match_returns_array() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("'hello world'.match('o')");
        assert!(result.is_ok());
        let val = result.unwrap();
        assert!(matches!(val, Value::Object(_)));
    }

    #[test]
    fn test_match_returns_null_on_no_match() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("'hello'.match('x')");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Value::Null);
    }

    #[test]
    fn test_match_global_returns_all_matches() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("'abab'.match(/ab/g)");
        assert!(result.is_ok());
    }

    // Tests for replace() with $-substitution
    #[test]
    fn test_replace_dollar_ampersand() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("'hello'.replace('l', '-$&-')");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Value::String("he-l-lo".to_string()));
    }

    #[test]
    fn test_replace_dollar_dollar() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("'hello'.replace('l', '$$')");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Value::String("he$lo".to_string()));
    }

    #[test]
    fn test_replace_dollar_backtick() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("'hello'.replace('l', '$`')");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Value::String("hehelo".to_string()));
    }

    #[test]
    fn test_replace_dollar_quote() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("'hello'.replace('l', \"$'\")");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Value::String("helolo".to_string()));
    }

    #[test]
    fn test_replace_capture_group_substitution() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("'abc'.replace(/(b)/, '[$1]')");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Value::String("a[b]c".to_string()));
    }

    #[test]
    fn test_replace_global_regex_replaces_all() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("'aaa'.replace(/a/g, 'b')");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Value::String("bbb".to_string()));
    }

    #[test]
    fn test_replace_non_global_regex_replaces_first() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("'hello world'.replace(/o/, '0')");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Value::String("hell0 world".to_string()));
    }

    // Tests for replaceAll()
    #[test]
    fn test_replace_all_empty_search() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("'abc'.replaceAll('', '-')");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Value::String("-a-b-c-".to_string()));
    }

    #[test]
    fn test_replace_all_basic() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("'hello world'.replaceAll('o', '0')");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Value::String("hell0 w0rld".to_string()));
    }

    #[test]
    fn test_replace_all_with_substitution() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("'abab'.replaceAll('ab', '($&)')");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Value::String("(ab)(ab)".to_string()));
    }
}
