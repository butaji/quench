//! RegExp built-in implementation
//!
//! Provides ECMAScript-compatible regular expression support.

mod string_methods;

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    // ------------------------------------------------------------------------
    // validate_unicode_backreferences
    // ------------------------------------------------------------------------

    #[test]
    fn validate_backref_no_capturing_groups() {
        // Pattern with no capturing groups should return false (valid).
        let regex = regress::Regex::new("abc").unwrap();
        let haystack = "abcdef";
        let m = regex.find(haystack).unwrap();
        // No groups → loop never enters → returns false
        assert!(!validate_unicode_backreferences(haystack, &m));
    }

    #[test]
    fn validate_backref_valid_backreference() {
        // Backreference that fits within the haystack is valid.
        // Pattern (a?)\\1 on "a": the greedy a? first tries "a", backref fails
        // (no second 'a'), so it backtracks to empty. Group 1 captures empty (0..0),
        // backref matches empty at pos 0. Formula: backref_pos = m.end() = 0,
        // captured_len = 0, check 0 + 0 > 1 → false (valid).
        let regex = regress::Regex::new("(a?)\\1").unwrap();
        let haystack = "a";
        let m = regex.find(haystack).unwrap();
        assert!(!validate_unicode_backreferences(haystack, &m));
    }

    #[test]
    fn validate_backref_extends_past_end() {
        // Pattern (ab)\\1? on "ab": group 1 captures "ab" at 0..2, backref at pos 2
        // needs 2 chars but only 0 remain → extends past end → invalid.
        let regex = regress::Regex::new("(ab)\\1?").unwrap();
        let haystack = "ab";
        let m = regex.find(haystack).unwrap();
        // The overall match succeeds (backref is optional), but backref extends past string.
        assert!(validate_unicode_backreferences(haystack, &m));
    }

    #[test]
    fn validate_backref_multiple_groups_mixed() {
        // Pattern: (a)\\1(b)\\1? — first backref valid, second extends past end.
        // (a)\\1 matches "aa", group 1 = "a" at 0..1; backref at 1 needs 1 char, ok.
        // (b)\\1? — group 2 = "b" at 2..3; backref at 3 needs 1 char but len is 3 → extends.
        // Since at least one group is invalid, function returns true.
        let regex = regress::Regex::new("(a)\\1(b)\\1?").unwrap();
        let haystack = "aab";
        let m = regex.find(haystack).unwrap();
        assert!(validate_unicode_backreferences(haystack, &m));
    }

    // ------------------------------------------------------------------------
    // regexp_match_state
    // ------------------------------------------------------------------------

    fn make_regexp_object(flags: &str) -> Rc<RefCell<Object>> {
        let mut obj = Object::new(ObjectKind::RegExp);
        obj.internal_regex_flags = Some(flags.to_string());
        Rc::new(RefCell::new(obj))
    }

    #[test]
    fn regexp_match_state_no_flags() {
        let obj = make_regexp_object("");
        let (flags, is_global_or_sticky, is_sticky) = regexp_match_state(&obj);
        assert_eq!(flags, "");
        assert!(!is_global_or_sticky);
        assert!(!is_sticky);
    }

    #[test]
    fn regexp_match_state_global_flag() {
        let obj = make_regexp_object("g");
        let (flags, is_global_or_sticky, is_sticky) = regexp_match_state(&obj);
        assert_eq!(flags, "g");
        assert!(is_global_or_sticky);
        assert!(!is_sticky);
    }

    #[test]
    fn regexp_match_state_sticky_flag() {
        let obj = make_regexp_object("y");
        let (flags, is_global_or_sticky, is_sticky) = regexp_match_state(&obj);
        assert_eq!(flags, "y");
        assert!(is_global_or_sticky);
        assert!(is_sticky);
    }

    #[test]
    fn regexp_match_state_gy_flags() {
        let obj = make_regexp_object("gy");
        let (flags, is_global_or_sticky, is_sticky) = regexp_match_state(&obj);
        assert_eq!(flags, "gy");
        assert!(is_global_or_sticky);
        assert!(is_sticky);
    }
}

use std::cell::RefCell;
use std::rc::Rc;

use regress::{Match, Regex};

use crate::value::convert::to_js_string;
use crate::value::{JsError, NativeFunction, Object, ObjectKind, Value};
use crate::Context;

pub use string_methods::register_string_regex_methods;

// ============================================================================
// RegExp object kind
// ============================================================================

thread_local! {
    static REGEXP_PROTOTYPE: RefCell<Option<Rc<RefCell<Object>>>> = const { RefCell::new(None) };
}

/// Get the cached RegExp prototype object
pub fn get_regexp_prototype() -> Rc<RefCell<Object>> {
    // Check if cached
    if let Some(p) = REGEXP_PROTOTYPE.with(|rp| rp.borrow().clone()) {
        return p;
    }
    // Not cached yet - create and cache it
    let proto_rc = create_regexp_prototype();
    REGEXP_PROTOTYPE.with(|rp| {
        *rp.borrow_mut() = Some(proto_rc.clone());
    });
    proto_rc
}

/// Create the RegExp prototype object
fn create_regexp_prototype() -> Rc<RefCell<Object>> {
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

/// Returns true if the regress match violates ES spec §21.2.2.9 backreference
/// semantics for the `u` flag: a backreference must match the exact code units
/// captured by the group. If the backref would extend past the end of the
/// string, the match is invalid and should be rejected (return None).
fn validate_unicode_backreferences(haystack: &str, m: &Match) -> bool {
    for i in 1.. {
        let Some(grp_range) = m.group(i) else {
            break;
        };
        let captured_len = grp_range.end - grp_range.start;
        let backref_pos = m.start() + (m.end() - grp_range.start);
        // Backref must match `captured_len` code units starting at `backref_pos`.
        // If that would extend past the string, the match is invalid.
        if backref_pos + captured_len > haystack.len() {
            return true; // invalid
        }
    }
    false // valid
}

/// Find the next match, honoring lastIndex for global/sticky regexes.
/// Returns the match and updates lastIndex per spec (end of match on
/// success, 0 on failure; untouched for non-global regexes).
fn regexp_find(obj: &Rc<RefCell<Object>>, regex: &Regex, haystack: &str) -> Option<regress::Match> {
    let (flags, is_global_or_sticky, is_sticky) = regexp_match_state(obj);
    if !is_global_or_sticky {
        let m = regex.find(haystack);
        // With `u` flag, backreferences must match exact code units (ES §21.2.2.9).
        // Only validate when there are capturing groups to avoid overhead on simple
        // patterns like /a/ (where S7.8.5_A1.1_T2.js creates 60000+ regexes).
        if let Some(ref m) = m {
            if flags.contains('u')
                && m.group(1).is_some()
                && validate_unicode_backreferences(haystack, m)
            {
                return None;
            }
        }
        return m;
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
            if flags.contains('u')
                && m.group(1).is_some()
                && validate_unicode_backreferences(haystack, &m)
            {
                obj.borrow_mut().set("lastIndex", Value::Number(0.0));
                return None;
            }
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
