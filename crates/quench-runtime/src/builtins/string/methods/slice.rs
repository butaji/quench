//! String slice methods (substring, slice)

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{to_number, NativeFunction, Object, Value};

/// Get the UTF-16 code unit length of a string (per ES spec).
fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// Extract a substring by UTF-16 code unit indices.
/// Panics if from > to or to > utf16_len(s).
fn utf16_substring(s: &str, from: usize, to: usize) -> String {
    let code_units: Vec<u16> = s.encode_utf16().skip(from).take(to - from).collect();
    String::from_utf16(&code_units).unwrap_or_else(|_| String::new())
}

fn substring_impl(args: &[Value], s: &str) -> Value {
    let len = utf16_len(s);
    let start = args.first().map(|v| to_number(v) as usize).unwrap_or(0);
    let end = args.get(1).map(|v| to_number(v) as usize).unwrap_or(len);
    let start = start.min(len);
    let end = end.min(len);
    let (from, to) = if start > end {
        (end, start)
    } else {
        (start, end)
    };
    Value::String(utf16_substring(s, from, to))
}

fn slice_impl(args: &[Value], s: &str) -> Value {
    let len = utf16_len(s) as isize;
    let start = args.first().map(|v| to_number(v) as i64).unwrap_or(0) as isize;
    let end = args
        .get(1)
        .map(|v| to_number(v) as i64)
        .unwrap_or(len as i64) as isize;
    let start_idx = if start < 0 {
        (len + start).max(0).min(len) as usize
    } else {
        (start as usize).min(len as usize)
    };
    let end_idx = if end < 0 {
        (len + end).max(0).min(len) as usize
    } else {
        (end as usize).min(len as usize)
    };
    let end_idx = end_idx.max(start_idx);
    Value::String(utf16_substring(s, start_idx, end_idx))
}

/// Install slice/substring methods
pub fn install_slice_methods(proto: &Rc<RefCell<Object>>) {
    let proto_clone = Rc::clone(proto);
    proto_clone.borrow_mut().set(
        "__substring",
        Value::NativeFunction(Rc::new(NativeFunction::new(
            move |args| match super::this_js_string() {
                Some(s) => Ok(substring_impl(&args, &s)),
                _ => Ok(Value::Undefined),
            },
        ))),
    );

    proto_clone.borrow_mut().set(
        "__slice",
        Value::NativeFunction(Rc::new(NativeFunction::new(
            move |args| match super::this_js_string() {
                Some(s) => Ok(slice_impl(&args, &s)),
                _ => Ok(Value::Undefined),
            },
        ))),
    );
}
