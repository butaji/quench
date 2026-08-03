//! Basic string methods (length, charAt, charCodeAt)

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{to_number, NativeFunction, Object, Value};

fn char_at_impl(args: &[Value], s: &str) -> Value {
    let idx = args.first().map(|v| to_number(v) as usize).unwrap_or(0);
    // ES spec §21.1.3.1: charAt returns a string of length 1 (one UTF-16 code unit).
    // For surrogate pairs, this returns the individual surrogate code unit as a string.
    Value::String(
        s.encode_utf16()
            .nth(idx)
            .map(|cu| {
                // Convert the UTF-16 code unit to a Rust char, or to a placeholder
                std::char::from_u32(cu as u32)
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| format!("\\u{:04X}", cu))
            })
            .unwrap_or_default(),
    )
}

fn char_code_at_impl(args: &[Value], s: &str) -> Value {
    let idx = args.first().map(|v| to_number(v) as usize).unwrap_or(0);
    // ES spec §21.1.3.2: charCodeAt returns UTF-16 code unit at index,
    // treating supplementary characters as two surrogate code units.
    Value::Number(
        s.encode_utf16()
            .nth(idx)
            .map(|cu| cu as f64)
            .unwrap_or(f64::NAN),
    )
}

fn code_point_at_impl(args: &[Value], s: &str) -> Value {
    let idx = args.first().map(to_number).unwrap_or(0.0) as usize;
    let utf16: Vec<u16> = s.encode_utf16().collect();
    let Some(&first) = utf16.get(idx) else {
        return Value::Undefined;
    };
    let code_point = if (0xd800..=0xdbff).contains(&first) {
        match utf16.get(idx + 1) {
            Some(&second) if (0xdc00..=0xdfff).contains(&second) => {
                0x10000 + ((first as u32 - 0xd800) << 10) + second as u32 - 0xdc00
            }
            _ => first as u32,
        }
    } else {
        first as u32
    };
    Value::Number(code_point as f64)
}

pub fn install_basic_methods(proto: &Rc<RefCell<Object>>) {
    let proto_clone = Rc::clone(proto);
    proto_clone.borrow_mut().set(
        "__charAt",
        Value::NativeFunction(Rc::new(NativeFunction::new(
            move |args| match super::this_js_string() {
                Some(s) => Ok(char_at_impl(&args, &s)),
                _ => Ok(Value::Undefined),
            },
        ))),
    );
    proto_clone.borrow_mut().set(
        "__charCodeAt",
        Value::NativeFunction(Rc::new(NativeFunction::new(
            move |args| match super::this_js_string() {
                Some(s) => Ok(char_code_at_impl(&args, &s)),
                _ => Ok(Value::Undefined),
            },
        ))),
    );
    proto_clone.borrow_mut().set(
        "__codePointAt",
        Value::NativeFunction(Rc::new(NativeFunction::new(
            move |args| match super::this_js_string() {
                Some(s) => Ok(code_point_at_impl(&args, &s)),
                _ => Ok(Value::Undefined),
            },
        ))),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_point_at_returns_supplementary_code_point() {
        let mut ctx = crate::Context::new().unwrap();
        assert_eq!(
            ctx.eval("'😀'.codePointAt(0)"),
            Ok(Value::Number(0x1f600 as f64))
        );
    }
}
