//! Pure helper functions for string methods — no runtime state needed.

use std::cell::RefCell;
use std::rc::Rc;

use regress::Regex;

use crate::value::{Object, Value};

/// Apply $-substitution to replacement string based on match info.
/// Handles: $$ -> $, $& -> matched, $` -> before match, $' -> after match, $n -> capture n
/// Per ECMAScript spec: for unrecognized $X where X is not one of the above,
/// the $ is kept literal and X is processed normally.
pub fn apply_substitution(
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
                        result.push(c);
                    }
                }
            } else {
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
pub fn handle_dollar_n(captures: &[&str], n: usize) -> String {
    if n > 0 && n <= captures.len() {
        captures[n - 1].to_string()
    } else {
        format!("${}", n)
    }
}

/// Extract capture-group strings from a match (group 0 excluded; unmatched
/// groups become empty strings).
pub fn match_captures<'a>(m: &regress::Match, string: &'a str) -> Vec<&'a str> {
    m.captures
        .iter()
        .map(|c| c.as_ref().map(|r| &string[r.clone()]).unwrap_or(""))
        .collect()
}

/// Split a string by a regex, returning a Value array.
pub fn split_by_regex(string: &str, regex: &Regex, limit: usize) -> Value {
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

/// Replace all matches with $-substitution.
pub fn replace_all_matches(string: &str, regex: &Regex, replacer: &str) -> String {
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

/// Build a match array from all regex matches over a string.
pub fn make_match_array(string: &str, regex: &Regex) -> Value {
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

/// Wrap a Vec<Value> in an Object array and return as Value::Object.
pub fn make_value_array(values: Vec<Value>) -> Value {
    let array = Object::new_array_from(values);
    Value::Object(Rc::new(RefCell::new(array)))
}
