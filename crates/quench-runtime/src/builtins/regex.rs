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

    fn eval(src: &str) -> Value {
        let mut ctx = crate::Context::new().unwrap();
        ctx.eval(src).unwrap()
    }

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

    #[test]
    fn regexp_escape_escapes_first_alphanumeric_and_hyphen() {
        assert_eq!(regexp_escape("a-b"), r#"\x61\x2db"#);
    }

    #[test]
    fn regexp_escape_rejects_non_strings() {
        assert!(regexp_escape_impl(vec![Value::Number(1.0)]).is_err());
    }

    #[test]
    fn regexp_escape_uses_lowercase_hex_digits() {
        assert_eq!(regexp_escape("jjj"), r#"\x6ajj"#);
    }

    #[test]
    fn regexp_escape_preserves_underscore() {
        assert_eq!(regexp_escape("_hello"), "_hello");
    }

    #[test]
    fn regexp_escape_has_builtin_function_metadata() {
        assert_eq!(
            eval("[RegExp.escape.name, RegExp.escape.length].join('|')"),
            Value::String("escape|1".to_string())
        );
        assert_eq!(
            eval("(function() { const d = Object.getOwnPropertyDescriptor(RegExp, 'escape'); return [d.writable, d.enumerable, d.configurable].join('|'); })()"),
            Value::String("true|false|true".to_string())
        );
    }

    #[test]
    fn regexp_escape_encodes_whitespace_and_line_terminators() {
        assert_eq!(
            regexp_escape(" \u{00a0}\u{2028}\u{2029}\u{202f}\u{feff}"),
            "\\x20\\xa0\\u2028\\u2029\\u202f\\ufeff"
        );
    }

    #[test]
    fn regexp_escape_decodes_wtf8_surrogates() {
        assert_eq!(regexp_escape("\u{fffd}d800"), "\\ud800");
    }

    #[test]
    fn regexp_has_species_getter() {
        assert_eq!(
            eval("typeof Object.getOwnPropertyDescriptor(RegExp, Symbol.species).get"),
            Value::String("function".to_string())
        );
        assert_eq!(
            eval("[Object.getOwnPropertyDescriptor(RegExp, Symbol.species).get.length, Object.getOwnPropertyDescriptor(Object.getOwnPropertyDescriptor(RegExp, Symbol.species).get, 'length').writable].join('|')"),
            Value::String("0|false".to_string())
        );
        assert_eq!(
            eval("Object.getOwnPropertyDescriptor(Object.getOwnPropertyDescriptor(RegExp, Symbol.species).get, 'name').value"),
            Value::String("get [Symbol.species]".to_string())
        );
    }

    #[test]
    fn regexp_symbol_match_calls_custom_exec() {
        assert_eq!(
            eval("var o = {exec: function() { return null; }}; RegExp.prototype[Symbol.match].call(o)"),
            Value::Null
        );
    }

    #[test]
    fn regexp_symbol_search_returns_custom_exec_index() {
        assert_eq!(
            eval("RegExp.prototype[Symbol.search].call({exec: function() { return {index: 86}; }}, 'abc')"),
            Value::Number(86.0)
        );
    }

    #[test]
    fn regexp_symbol_search_sets_and_restores_last_index() {
        assert_eq!(
            eval("var seen; var o = {lastIndex: 34, exec: function() { seen = this.lastIndex; return null; }}; RegExp.prototype[Symbol.search].call(o); [seen, o.lastIndex].join('|')"),
            Value::String("0|34".to_string())
        );
    }

    #[test]
    fn regexp_symbol_search_propagates_index_getter_errors() {
        let mut ctx = crate::Context::new().unwrap();
        assert!(ctx
            .eval("var o = {exec: function() { return {get index() { throw new Test262Error(); }}; }}; RegExp.prototype[Symbol.search].call(o)")
            .is_err());
    }

    #[test]
    fn regexp_symbol_search_propagates_last_index_getter_errors() {
        let mut ctx = crate::Context::new().unwrap();
        assert!(ctx
            .eval("var o = {get lastIndex() { throw new Test262Error(); }, exec: function() { return null; }}; RegExp.prototype[Symbol.search].call(o)")
            .is_err());
    }

    #[test]
    fn regexp_symbol_search_propagates_string_coercion_errors() {
        let mut ctx = crate::Context::new().unwrap();
        assert!(ctx
            .eval("var o = {toString: function() { throw new Test262Error(); }}; /./[Symbol.search](o)")
            .is_err());
    }

    #[test]
    fn regexp_symbol_search_distinguishes_negative_zero_last_index() {
        let mut ctx = crate::Context::new().unwrap();
        assert_eq!(
            ctx.eval("var seen; var r = /(?:)/; r.lastIndex = -0; r.exec = function() { seen = r.lastIndex; return null; }; r[Symbol.search](''); [seen, 1 / r.lastIndex].join('|')"),
            Ok(Value::String("0|-Infinity".to_string()))
        );
    }

    #[test]
    fn regexp_has_match_all_method() {
        let mut ctx = crate::Context::new().unwrap();
        assert_eq!(
            ctx.eval("typeof RegExp.prototype[Symbol.matchAll]"),
            Ok(Value::String("function".to_string()))
        );
    }

    #[test]
    fn regexp_match_all_iterator_returns_matches() {
        let mut ctx = crate::Context::new().unwrap();
        assert_eq!(
            ctx.eval("var i = /a/g[Symbol.matchAll]('aba'); [i.next().value[0], i.next().value[0], i.next().done].join('|')"),
            Ok(Value::String("a|a|true".to_string()))
        );
    }

    #[test]
    fn regexp_match_all_propagates_string_coercion_errors() {
        let mut ctx = crate::Context::new().unwrap();
        assert!(ctx
            .eval("var o = {toString: function() { throw new Test262Error(); }}; /a/g[Symbol.matchAll](o)")
            .is_err());
    }

    #[test]
    fn regexp_match_all_caches_last_index() {
        let mut ctx = crate::Context::new().unwrap();
        assert_eq!(
            ctx.eval("var r = /./g; r.lastIndex = 2; var i = r[Symbol.matchAll]('abcd'); r.lastIndex = 0; i.next().value[0]"),
            Ok(Value::String("c".to_string()))
        );
    }

    #[test]
    fn regexp_match_all_iterator_is_iterable() {
        let mut ctx = crate::Context::new().unwrap();
        assert_eq!(
            ctx.eval("var i = /a/g[Symbol.matchAll]('a'); i[Symbol.iterator]() === i"),
            Ok(Value::Boolean(true))
        );
    }

    #[test]
    fn regexp_match_all_propagates_last_index_conversion_errors() {
        let mut ctx = crate::Context::new().unwrap();
        assert!(ctx
            .eval("var r = /./; r.lastIndex = {valueOf: function() { throw new Test262Error(); }}; r[Symbol.matchAll]('')")
            .is_err());
    }

    #[test]
    fn regexp_match_all_uses_species_matcher() {
        let mut ctx = crate::Context::new().unwrap();
        assert_eq!(
            ctx.eval("var r = /\\d/u; r.constructor = {[Symbol.species]: function() { return /\\w/g; }}; r[Symbol.matchAll]('a*b').next().value[0]"),
            Ok(Value::String("a".to_string()))
        );
    }

    #[test]
    fn regexp_match_all_calls_species_constructor_with_regex_and_flags() {
        let mut ctx = crate::Context::new().unwrap();
        assert_eq!(
            ctx.eval("var n = 0; var a; var r = /\\d/u; r.constructor = {[Symbol.species]: function() { n++; a = arguments; return /\\w/g; }}; var i = r[Symbol.matchAll]('a*b'); [n, a.length, a[0] === r, a[1]].join('|')"),
            Ok(Value::String("1|2|true|u".to_string()))
        );
    }

    #[test]
    fn regexp_match_all_propagates_constructor_getter_errors() {
        let mut ctx = crate::Context::new().unwrap();
        assert!(ctx
            .eval("var r = /a/g; Object.defineProperty(r, 'constructor', {get: function() { throw new Test262Error(); }}); r[Symbol.matchAll]('a')")
            .is_err());
    }
}

use std::cell::RefCell;
use std::rc::Rc;

use regress::{Match, Regex};

use crate::value::convert::to_js_string;
use crate::value::{JsError, NativeFunction, Object, ObjectKind, PropertyFlags, Value};
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

/// Save the thread-local prototype cache (realm snapshot support)
pub(crate) fn save_regexp_prototype() -> Option<Rc<RefCell<Object>>> {
    REGEXP_PROTOTYPE.with(|rp| rp.borrow().clone())
}

/// Restore the thread-local prototype cache (realm snapshot support)
pub(crate) fn restore_regexp_prototype(proto: Option<Rc<RefCell<Object>>>) {
    REGEXP_PROTOTYPE.with(|rp| *rp.borrow_mut() = proto);
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

    if let Some(Value::Symbol(symbol)) =
        crate::builtins::symbol::get_well_known_symbol_no_ctx("match")
    {
        proto.borrow_mut().set(
            &symbol.property_key(),
            Value::NativeFunction(Rc::new(NativeFunction::new(regexp_symbol_match_impl))),
        );
    }
    if let Some(Value::Symbol(symbol)) =
        crate::builtins::symbol::get_well_known_symbol_no_ctx("search")
    {
        proto.borrow_mut().set(
            &symbol.property_key(),
            Value::NativeFunction(Rc::new(NativeFunction::new(regexp_symbol_search_impl))),
        );
    }

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
    // Note: lastIndex is NOT set on the prototype — it must be an own data
    // property on each instance per ES §21.2.6.1 ({ writable: true, enumerable: false, configurable: false }).
}

fn regexp_symbol_match_impl(args: Vec<Value>) -> Result<Value, JsError> {
    let this_val = crate::builtins::get_native_this()
        .ok_or_else(|| JsError::new("RegExp.prototype[@@match] requires 'this'".to_string()))?;
    let Value::Object(obj) = &this_val else {
        return Err(JsError::new("TypeError: incompatible receiver".to_string()));
    };
    let exec = obj.borrow().get("exec").ok_or_else(|| {
        JsError::new("TypeError: RegExp.prototype[@@match] requires callable exec".to_string())
    })?;
    crate::eval::function::call_value_with_this(
        exec,
        vec![Value::String(
            args.first().map(to_js_string).unwrap_or_default(),
        )],
        this_val.clone(),
    )
}

fn regexp_symbol_search_impl(args: Vec<Value>) -> Result<Value, JsError> {
    let this_val = crate::builtins::get_native_this()
        .ok_or_else(|| JsError::new("RegExp.prototype[@@search] requires 'this'".to_string()))?;
    let Value::Object(obj) = &this_val else {
        return Err(JsError::new("TypeError: incompatible receiver".to_string()));
    };
    let last_index = crate::eval::member::eval_object_member_value(
        obj,
        &Value::String("lastIndex".to_string()),
        current_regex_env().as_ref(),
    )?;
    if !same_value_search(&last_index, &Value::Number(0.0)) {
        set_search_last_index(obj, &this_val, Value::Number(0.0))?;
    }
    let exec = obj.borrow().get("exec").ok_or_else(|| {
        JsError::new("TypeError: RegExp.prototype[@@search] requires callable exec".to_string())
    })?;
    let result = crate::eval::function::call_value_with_this(
        exec,
        vec![Value::String(regexp_search_string(args.first())?)],
        this_val.clone(),
    )?;
    let current_last_index = crate::eval::member::eval_object_member_value(
        obj,
        &Value::String("lastIndex".to_string()),
        current_regex_env().as_ref(),
    )?;
    if !same_value_search(&current_last_index, &last_index) {
        set_search_last_index(obj, &this_val, last_index)?;
    }
    match result {
        Value::Null => Ok(Value::Number(-1.0)),
        Value::Object(result) => crate::eval::member::eval_object_member_value(
            &result,
            &Value::String("index".to_string()),
            current_regex_env().as_ref(),
        ),
        _ => Err(JsError::new("TypeError: invalid exec result".to_string())),
    }
}

fn regexp_symbol_match_all_impl(args: Vec<Value>) -> Result<Value, JsError> {
    let this_val = crate::builtins::get_native_this()
        .ok_or_else(|| JsError::new("TypeError: incompatible receiver".to_string()))?;
    let Value::Object(regex_obj) = this_val else {
        return Err(JsError::new("TypeError: incompatible receiver".to_string()));
    };
    let input = regexp_search_string(args.first())?;
    let flags_value = crate::eval::member::eval_object_member_value(
        &regex_obj,
        &Value::String("flags".to_string()),
        current_regex_env().as_ref(),
    )?;
    let flags = regexp_search_string(Some(&flags_value))?;
    let mut matcher_obj = Rc::clone(&regex_obj);
    let constructor = crate::eval::member::eval_object_member_value(
        &regex_obj,
        &Value::String("constructor".to_string()),
        current_regex_env().as_ref(),
    )?;
    if let Value::Object(constructor) = constructor {
        if let Some(Value::Symbol(species)) =
            crate::builtins::symbol::get_well_known_symbol_no_ctx("species")
        {
            let species_fn = crate::eval::member::eval_object_member_value(
                &constructor,
                &Value::Symbol(Rc::clone(&species)),
                current_regex_env().as_ref(),
            )?;
            if !matches!(species_fn, Value::Undefined | Value::Null) {
                if matches!(
                    species_fn,
                    Value::Function(_)
                        | Value::NativeFunction(_)
                        | Value::NativeConstructor(_)
                        | Value::Class(_)
                ) {
                    let matcher = crate::eval::function::call_value_with_this(
                        species_fn,
                        vec![
                            Value::Object(Rc::clone(&regex_obj)),
                            Value::String(flags.clone()),
                        ],
                        Value::Undefined,
                    )?;
                    if let Value::Object(matcher) = matcher {
                        matcher_obj = matcher;
                    }
                }
            }
        }
    }
    let matcher_flags = if Rc::ptr_eq(&matcher_obj, &regex_obj) {
        flags.clone()
    } else {
        matcher_obj
            .borrow()
            .internal_regex_flags
            .clone()
            .unwrap_or(flags)
    };
    if !matcher_flags.contains('g') {
        return Err(JsError::new(
            "TypeError: matchAll requires global RegExp".to_string(),
        ));
    }
    let source = matcher_obj
        .borrow()
        .internal_regex_source
        .clone()
        .unwrap_or_default();
    let regex =
        Regex::new(&source).map_err(|_| JsError::new("Invalid regular expression".to_string()))?;
    let last_index = crate::eval::member::eval_object_member_value(
        &matcher_obj,
        &Value::String("lastIndex".to_string()),
        current_regex_env().as_ref(),
    )?;
    let state = Rc::new(RefCell::new(
        regexp_match_all_last_index(&last_index)?.max(0.0) as usize,
    ));
    let iterator = Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary)));
    let next_state = Rc::clone(&state);
    let next = NativeFunction::new(move |_args| {
        let start = *next_state.borrow();
        let Some(matched) = regex.find_from(&input, start).next() else {
            let mut done = Object::new(ObjectKind::Ordinary);
            done.set("value", Value::Undefined);
            done.set("done", Value::Boolean(true));
            return Ok(Value::Object(Rc::new(RefCell::new(done))));
        };
        *next_state.borrow_mut() = matched.end().max(matched.start() + 1);
        let mut next_result = Object::new(ObjectKind::Ordinary);
        next_result.set("value", build_exec_result(&input, &matched, &regex));
        next_result.set("done", Value::Boolean(false));
        Ok(Value::Object(Rc::new(RefCell::new(next_result))))
    });
    iterator
        .borrow_mut()
        .set("next", Value::NativeFunction(Rc::new(next)));
    if let Some(Value::Symbol(symbol)) =
        crate::builtins::symbol::get_well_known_symbol_no_ctx("iterator")
    {
        let iterator_value = Value::Object(Rc::clone(&iterator));
        let iterator_method = NativeFunction::new(move |_args| Ok(iterator_value.clone()));
        iterator.borrow_mut().set_symbol(
            &symbol.property_key(),
            Value::NativeFunction(Rc::new(iterator_method)),
        );
    }
    Ok(Value::Object(iterator))
}

fn regexp_match_all_last_index(value: &Value) -> Result<f64, JsError> {
    if let Value::Object(object) = value {
        let method = object
            .borrow()
            .get("valueOf")
            .ok_or_else(|| JsError::new("TypeError: missing valueOf".to_string()))?;
        let primitive = crate::eval::function::call_value_with_this(
            method,
            Vec::new(),
            Value::Object(Rc::clone(object)),
        )?;
        return Ok(crate::value::to_number(&primitive));
    }
    Ok(crate::value::to_number(value))
}

fn current_regex_env() -> Option<Rc<RefCell<crate::env::Environment>>> {
    crate::context::CURRENT_CONTEXT
        .with(|cell| cell.borrow().map(|ptr| unsafe { (&*ptr).env().clone() }))
}

fn same_value_search(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Number(a), Value::Number(b)) if a.is_nan() && b.is_nan() => true,
        (Value::Number(a), Value::Number(b)) => a.to_bits() == b.to_bits(),
        _ => left == right,
    }
}

fn regexp_search_string(value: Option<&Value>) -> Result<String, JsError> {
    let value = value.unwrap_or(&Value::Undefined);
    if matches!(value, Value::Symbol(_)) {
        return Err(JsError::new(
            "TypeError: Cannot convert Symbol to string".to_string(),
        ));
    }
    if let Value::Object(object) = value {
        let method = object
            .borrow()
            .get("toString")
            .ok_or_else(|| JsError::new("TypeError: missing toString".to_string()))?;
        let result = crate::eval::function::call_value_with_this(
            method,
            Vec::new(),
            Value::Object(Rc::clone(object)),
        )?;
        return Ok(to_js_string(&result));
    }
    Ok(to_js_string(value))
}

fn set_search_last_index(
    obj: &Rc<RefCell<Object>>,
    this_val: &Value,
    value: Value,
) -> Result<(), JsError> {
    let setter = { obj.borrow().get_setter_func("lastIndex") };
    if let Some(setter) = setter {
        crate::eval::function::call_value_with_this(setter, vec![value], this_val.clone())?;
    } else {
        if matches!(obj.borrow().get_descriptor("lastIndex"), Some(flags) if !flags.writable) {
            return Err(JsError::new(
                "TypeError: lastIndex is not writable".to_string(),
            ));
        }
        obj.borrow_mut().set("lastIndex", value);
    }
    Ok(())
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
    let species_default = regexp_fn.clone();

    // Create RegExp object to hold the constructor
    let mut regexp_obj = Object::new(ObjectKind::Ordinary);
    regexp_obj.callable = true;
    let regexp_obj_rc = Rc::new(RefCell::new(regexp_obj));
    if let Some(Value::Symbol(species)) =
        crate::builtins::symbol::get_well_known_symbol_no_ctx("species")
    {
        let getter = NativeFunction::new(move |_args| {
            Ok(crate::builtins::get_native_this().unwrap_or(species_default.clone()))
        });
        getter.define_property(
            "name",
            Value::String("get [Symbol.species]".to_string()),
            PropertyFlags {
                value: Some(Value::String("get [Symbol.species]".to_string())),
                writable: false,
                enumerable: false,
                configurable: true,
            },
        );
        getter.define_property(
            "length",
            Value::Number(0.0),
            PropertyFlags {
                value: Some(Value::Number(0.0)),
                writable: false,
                enumerable: false,
                configurable: true,
            },
        );
        let getter = Value::NativeFunction(Rc::new(getter));
        regexp_obj_rc.borrow_mut().define_accessor(
            &species.property_key(),
            Some(getter),
            None,
            PropertyFlags {
                value: None,
                writable: false,
                enumerable: false,
                configurable: true,
            },
        );
    }
    if let Some(Value::Symbol(symbol)) =
        crate::builtins::symbol::get_well_known_symbol_no_ctx("match")
    {
        regexp_proto.borrow_mut().set(
            &symbol.property_key(),
            Value::NativeFunction(Rc::new(NativeFunction::new(regexp_symbol_match_impl))),
        );
    }
    if let Some(Value::Symbol(symbol)) =
        crate::builtins::symbol::get_well_known_symbol_no_ctx("search")
    {
        let search = NativeFunction::new(regexp_symbol_search_impl);
        search.define_property(
            "name",
            Value::String("[Symbol.search]".to_string()),
            PropertyFlags {
                value: Some(Value::String("[Symbol.search]".to_string())),
                writable: false,
                enumerable: false,
                configurable: true,
            },
        );
        search.define_property(
            "length",
            Value::Number(1.0),
            PropertyFlags {
                value: Some(Value::Number(1.0)),
                writable: false,
                enumerable: false,
                configurable: true,
            },
        );
        regexp_proto.borrow_mut().define(
            &symbol.property_key(),
            Value::NativeFunction(Rc::new(search)),
            PropertyFlags {
                value: None,
                writable: true,
                enumerable: false,
                configurable: true,
            },
        );
    }
    if let Some(Value::Symbol(symbol)) =
        crate::builtins::symbol::get_well_known_symbol_no_ctx("matchAll")
    {
        let method = NativeFunction::new(regexp_symbol_match_all_impl);
        method.define_property(
            "name",
            Value::String("[Symbol.matchAll]".to_string()),
            PropertyFlags {
                value: Some(Value::String("[Symbol.matchAll]".to_string())),
                writable: false,
                enumerable: false,
                configurable: true,
            },
        );
        method.define_property(
            "length",
            Value::Number(1.0),
            PropertyFlags {
                value: Some(Value::Number(1.0)),
                writable: false,
                enumerable: false,
                configurable: true,
            },
        );
        regexp_proto.borrow_mut().define(
            &symbol.property_key(),
            Value::NativeFunction(Rc::new(method)),
            PropertyFlags {
                value: None,
                writable: true,
                enumerable: false,
                configurable: true,
            },
        );
    }
    regexp_obj_rc
        .borrow_mut()
        .set("prototype", Value::Object(Rc::clone(&regexp_proto)));
    regexp_obj_rc
        .borrow_mut()
        .set("constructor", regexp_fn.clone());
    regexp_obj_rc
        .borrow_mut()
        .set("lastIndex", Value::Number(0.0));
    let escape_fn = NativeFunction::new(regexp_escape_impl);
    escape_fn.define_property(
        "name",
        Value::String("escape".to_string()),
        PropertyFlags {
            value: Some(Value::String("escape".to_string())),
            writable: false,
            enumerable: false,
            configurable: true,
        },
    );
    escape_fn.define_property(
        "length",
        Value::Number(1.0),
        PropertyFlags {
            value: Some(Value::Number(1.0)),
            writable: false,
            enumerable: false,
            configurable: true,
        },
    );
    regexp_obj_rc.borrow_mut().define(
        "escape",
        Value::NativeFunction(Rc::new(escape_fn)),
        PropertyFlags {
            value: None,
            writable: true,
            enumerable: false,
            configurable: true,
        },
    );

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
        // lastIndex per ES §21.2.6.1: own data property, writable, non-enumerable, non-configurable
        obj.define(
            "lastIndex",
            Value::Number(0.0),
            PropertyFlags {
                value: Some(Value::Number(0.0)),
                writable: true,
                enumerable: false,
                configurable: false,
            },
        );
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
        // lastIndex per ES §21.2.6.1: own data property, writable, non-enumerable, non-configurable
        obj.define(
            "lastIndex",
            Value::Number(0.0),
            PropertyFlags {
                value: Some(Value::Number(0.0)),
                writable: true,
                enumerable: false,
                configurable: false,
            },
        );
        obj.set("flags", Value::String(flags.clone()));
        obj.internal_regex = Some(compiled);
        obj.prototype = Some(Rc::clone(regexp_proto));
        Ok(Value::Object(Rc::new(RefCell::new(obj))))
    }
}

fn regexp_escape_impl(args: Vec<Value>) -> Result<Value, JsError> {
    let input = match args.first() {
        Some(Value::String(value)) => value.clone(),
        _ => {
            return Err(JsError::new(
                "TypeError: argument must be a string".to_string(),
            ))
        }
    };
    Ok(Value::String(regexp_escape(&input)))
}

fn regexp_escape(input: &str) -> String {
    let mut escaped = String::new();
    let mut chars = input.chars().peekable();
    let mut index = 0;
    while let Some(ch) = chars.next() {
        if ch == '\u{fffd}' {
            let mut digits = String::new();
            for _ in 0..4 {
                let Some(next) = chars.peek().copied() else {
                    break;
                };
                if !next.is_ascii_hexdigit() {
                    break;
                }
                digits.push(next);
                chars.next();
            }
            if digits.len() == 4 {
                let code_unit = u16::from_str_radix(&digits, 16).unwrap_or(0);
                if (0xd800..=0xdfff).contains(&code_unit) {
                    escaped.push_str(&format!(r#"\u{:04x}"#, code_unit));
                    index += 1;
                    continue;
                }
                escaped.push(ch);
                escaped.push_str(&digits);
                index += 5;
                continue;
            }
        }
        if index == 0 && ch.is_ascii_alphanumeric() {
            escaped.push_str(&format!(r#"\x{:02x}"#, ch as u32));
        } else if "^$\\.*+?()[]{}|/".contains(ch) {
            escaped.push('\\');
            escaped.push(ch);
        } else if ch == ' ' {
            escaped.push_str(r#"\x20"#);
        } else if ch == '\u{00a0}' {
            escaped.push_str(r#"\xa0"#);
        } else if matches!(ch, '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{feff}') {
            escaped.push_str(&format!(r#"\u{:04x}"#, ch as u32));
        } else if ch.is_ascii_punctuation() && ch != '_' {
            escaped.push_str(&format!(r#"\x{:02x}"#, ch as u32));
        } else if ch == '\n' {
            escaped.push_str(r#"\n"#);
        } else if ch == '\r' {
            escaped.push_str(r#"\r"#);
        } else if ch == '\t' {
            escaped.push_str(r#"\t"#);
        } else if ch == '\u{000B}' {
            escaped.push_str(r#"\v"#);
        } else if ch == '\u{000C}' {
            escaped.push_str(r#"\f"#);
        } else {
            escaped.push(ch);
        }
        index += 1;
    }
    escaped
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
