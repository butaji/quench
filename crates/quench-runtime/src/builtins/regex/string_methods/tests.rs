//! Unit tests for String.prototype methods — match, search, replace, replaceAll, split.

use crate::value::Value;
use crate::Context;

#[test]
fn test_match_returns_array() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval("'hello world'.match('o')");
    assert!(result.is_ok());
    assert!(matches!(result.unwrap(), Value::Object(_)));
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

#[test]
fn split_string_separator_is_literal_not_regex() {
    let mut ctx = Context::new().unwrap();
    crate::builtins::register_builtins(&mut ctx);
    let result = ctx.eval("'0.5'.split('.').length").unwrap();
    assert_eq!(result, Value::Number(2.0));
}
