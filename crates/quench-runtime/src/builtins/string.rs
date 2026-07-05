//! String built-in - shared String.prototype object

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{to_js_string, to_number, NativeFunction, Object, ObjectKind, Value};
use crate::Context;

// Thread-local storage for String.prototype (created once, shared)
thread_local! {
    static STRING_PROTOTYPE: RefCell<Option<Rc<RefCell<Object>>>> = const { RefCell::new(None) };
}

/// Get the String.prototype object
pub fn get_string_prototype() -> Option<Rc<RefCell<Object>>> {
    STRING_PROTOTYPE.with(|sp| sp.borrow().clone())
}

/// Register String.fromCharCode and String.fromCodePoint methods
fn register_string_static_methods(string_obj: &Rc<RefCell<Object>>) {
    string_obj.borrow_mut().set("fromCharCode", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let chars: String = args.iter()
            .map(|v| {
                let code = to_number(v) as u16;
                std::char::from_u32(code as u32).unwrap_or('\u{FFFD}')
            })
            .collect();
        Ok(Value::String(chars))
    }))));

    string_obj.borrow_mut().set("fromCodePoint", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let chars: String = args.iter()
            .map(|v| {
                let code = to_number(v) as u32;
                std::char::from_u32(code).unwrap_or('\u{FFFD}')
            })
            .collect();
        Ok(Value::String(chars))
    }))));
}

/// Register the String object and String.prototype
pub fn register_string(ctx: &mut Context) {
    let string_obj = Object::new(ObjectKind::Ordinary);
    let string_obj = Rc::new(RefCell::new(string_obj));

    register_string_static_methods(&string_obj);

    // Create String.prototype and attach methods
    let string_proto = Object::new(ObjectKind::Ordinary);
    let string_proto_rc = Rc::new(RefCell::new(string_proto));

    install_string_methods(&string_proto_rc);
    string_obj.borrow_mut().set("prototype", Value::Object(Rc::clone(&string_proto_rc)));

    STRING_PROTOTYPE.with(|sp| {
        *sp.borrow_mut() = Some(Rc::clone(&string_proto_rc));
    });

    ctx.set_global("String".to_string(), Value::Object(string_obj));
}

// ============================================================================
// String.prototype methods - grouped by function
// ============================================================================

/// Get string from 'this' binding or return Undefined
fn get_string_this() -> Value {
    match crate::builtins::get_native_this() {
        Some(Value::String(s)) => Value::String(s),
        _ => Value::Undefined,
    }
}

/// Install basic string methods (length, charAt, charCodeAt, etc.)
fn install_basic_methods(proto: &Rc<RefCell<Object>>) {
    let proto_clone = Rc::clone(proto);

    proto_clone.borrow_mut().set("length", Value::NativeFunction(Rc::new(NativeFunction::new(move |_| {
        match crate::builtins::get_native_this() {
            Some(Value::String(s)) => Ok(Value::Number(s.len() as f64)),
            _ => Ok(Value::Undefined),
        }
    }))));

    proto_clone.borrow_mut().set("charAt", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        match crate::builtins::get_native_this() {
            Some(Value::String(s)) => {
                let idx = args.first().map(|v| to_number(v) as usize).unwrap_or(0);
                Ok(Value::String(s.chars().nth(idx).map(|c| c.to_string()).unwrap_or_default()))
            }
            _ => Ok(Value::Undefined),
        }
    }))));

    proto_clone.borrow_mut().set("charCodeAt", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        match crate::builtins::get_native_this() {
            Some(Value::String(s)) => {
                let idx = args.first().map(|v| to_number(v) as usize).unwrap_or(0);
                let code = s.chars().nth(idx).map(|c| c as u16 as f64).unwrap_or(f64::NAN);
                Ok(Value::Number(code))
            }
            _ => Ok(Value::Undefined),
        }
    }))));
}

/// Install indexOf and lastIndexOf methods
fn install_index_methods(proto: &Rc<RefCell<Object>>) {
    let proto_clone = Rc::clone(proto);

    proto_clone.borrow_mut().set("indexOf", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        match crate::builtins::get_native_this() {
            Some(Value::String(s)) => {
                let needle = args.first().map(to_js_string).unwrap_or_default();
                let start = args.get(1).map(|v| to_number(v) as usize).unwrap_or(0);
                Ok(Value::Number(s[start..].find(&needle).map(|i| (start + i) as f64).unwrap_or(-1.0)))
            }
            _ => Ok(Value::Undefined),
        }
    }))));

    proto_clone.borrow_mut().set("lastIndexOf", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        match crate::builtins::get_native_this() {
            Some(Value::String(s)) => {
                let needle = args.first().map(to_js_string).unwrap_or_default();
                let pos = args.get(1).map(|v| to_number(v) as usize).unwrap_or(usize::MAX);
                let pos = pos.min(s.len());
                let result = s[..pos].rfind(&needle).map(|i| i as f64).unwrap_or(-1.0);
                Ok(Value::Number(result))
            }
            _ => Ok(Value::Undefined),
        }
    }))));
}

/// Install includes, startsWith, endsWith methods
fn install_prefix_suffix_methods(proto: &Rc<RefCell<Object>>) {
    let proto_clone = Rc::clone(proto);

    proto_clone.borrow_mut().set("includes", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        match crate::builtins::get_native_this() {
            Some(Value::String(s)) => {
                let needle = args.first().map(to_js_string).unwrap_or_default();
                let start = args.get(1).map(|v| to_number(v) as usize).unwrap_or(0);
                Ok(Value::Boolean(s[start..].contains(&needle)))
            }
            _ => Ok(Value::Undefined),
        }
    }))));

    proto_clone.borrow_mut().set("startsWith", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        match crate::builtins::get_native_this() {
            Some(Value::String(s)) => {
                let needle = args.first().map(to_js_string).unwrap_or_default();
                let start = args.get(1).map(|v| to_number(v) as usize).unwrap_or(0);
                Ok(Value::Boolean(s[start..].starts_with(&needle)))
            }
            _ => Ok(Value::Undefined),
        }
    }))));

    proto_clone.borrow_mut().set("endsWith", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        match crate::builtins::get_native_this() {
            Some(Value::String(s)) => {
                let needle = args.first().map(to_js_string).unwrap_or_default();
                let end_pos = args.get(1).map(|v| to_number(v) as usize);
                let matches = if let Some(pos) = end_pos {
                    s[..pos.min(s.len())].ends_with(&needle)
                } else {
                    s.ends_with(&needle)
                };
                Ok(Value::Boolean(matches))
            }
            _ => Ok(Value::Undefined),
        }
    }))));
}

/// Install search methods (delegates to index and prefix/suffix methods)
fn install_search_methods(proto: &Rc<RefCell<Object>>) {
    install_index_methods(proto);
    install_prefix_suffix_methods(proto);
}

/// Install case methods (toUpperCase, toLowerCase)
fn install_case_methods(proto: &Rc<RefCell<Object>>) {
    let proto_clone = Rc::clone(proto);

    proto_clone.borrow_mut().set("toUpperCase", Value::NativeFunction(Rc::new(NativeFunction::new(move |_| {
        match crate::builtins::get_native_this() {
            Some(Value::String(s)) => Ok(Value::String(s.to_uppercase())),
            _ => Ok(Value::Undefined),
        }
    }))));

    proto_clone.borrow_mut().set("toLowerCase", Value::NativeFunction(Rc::new(NativeFunction::new(move |_| {
        match crate::builtins::get_native_this() {
            Some(Value::String(s)) => Ok(Value::String(s.to_lowercase())),
            _ => Ok(Value::Undefined),
        }
    }))));
}

/// Install trim methods (trim, trimStart, trimLeft, trimEnd, trimRight)
fn install_trim_methods(proto: &Rc<RefCell<Object>>) {
    let proto_clone = Rc::clone(proto);

    proto_clone.borrow_mut().set("trim", Value::NativeFunction(Rc::new(NativeFunction::new(move |_| {
        match crate::builtins::get_native_this() {
            Some(Value::String(s)) => Ok(Value::String(s.trim().to_string())),
            _ => Ok(Value::Undefined),
        }
    }))));

    proto_clone.borrow_mut().set("trimStart", Value::NativeFunction(Rc::new(NativeFunction::new(move |_| {
        match crate::builtins::get_native_this() {
            Some(Value::String(s)) => Ok(Value::String(s.trim_start().to_string())),
            _ => Ok(Value::Undefined),
        }
    }))));

    proto_clone.borrow_mut().set("trimLeft", Value::NativeFunction(Rc::new(NativeFunction::new(move |_| {
        match crate::builtins::get_native_this() {
            Some(Value::String(s)) => Ok(Value::String(s.trim_start().to_string())),
            _ => Ok(Value::Undefined),
        }
    }))));

    proto_clone.borrow_mut().set("trimEnd", Value::NativeFunction(Rc::new(NativeFunction::new(move |_| {
        match crate::builtins::get_native_this() {
            Some(Value::String(s)) => Ok(Value::String(s.trim_end().to_string())),
            _ => Ok(Value::Undefined),
        }
    }))));

    proto_clone.borrow_mut().set("trimRight", Value::NativeFunction(Rc::new(NativeFunction::new(move |_| {
        match crate::builtins::get_native_this() {
            Some(Value::String(s)) => Ok(Value::String(s.trim_end().to_string())),
            _ => Ok(Value::Undefined),
        }
    }))));
}

/// Install slice/substring methods
fn install_slice_methods(proto: &Rc<RefCell<Object>>) {
    let proto_clone = Rc::clone(proto);

    proto_clone.borrow_mut().set("substring", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        match crate::builtins::get_native_this() {
            Some(Value::String(s)) => {
                let start = args.get(0).map(|v| to_number(v) as usize).unwrap_or(0);
                let end = args.get(1).map(|v| to_number(v) as usize).unwrap_or(s.len());
                let start = start.min(s.len());
                let end = end.min(s.len());
                let (start, end) = if start > end { (end, start) } else { (start, end) };
                Ok(Value::String(s.chars().skip(start).take(end - start).collect()))
            }
            _ => Ok(Value::Undefined),
        }
    }))));

    proto_clone.borrow_mut().set("slice", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        match crate::builtins::get_native_this() {
            Some(Value::String(s)) => {
                let start = args.get(0).map(|v| to_number(v) as i64).unwrap_or(0) as isize;
                let end = args.get(1).map(|v| to_number(v) as i64).unwrap_or(s.len() as i64) as isize;
                let len = s.len() as isize;
                
                let start_idx = if start < 0 { (len + start).max(0).min(len) as usize } else { (start as usize).min(len as usize) };
                let end_idx = if end < 0 { (len + end).max(0).min(len) as usize } else { (end as usize).min(len as usize) };
                let end_idx = end_idx.max(start_idx);
                
                Ok(Value::String(s.chars().skip(start_idx).take(end_idx - start_idx).collect()))
            }
            _ => Ok(Value::Undefined),
        }
    }))));
}

/// Install split and concat methods
fn install_split_concat_methods(proto: &Rc<RefCell<Object>>) {
    let proto_clone = Rc::clone(proto);

    proto_clone.borrow_mut().set("split", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        match crate::builtins::get_native_this() {
            Some(Value::String(s)) => {
                let sep = args.first().map(to_js_string).unwrap_or_default();
                let limit = args.get(1).map(|v| to_number(v) as usize);
                let parts: Vec<Value> = if sep.is_empty() {
                    s.chars().map(|c| Value::String(c.to_string())).collect()
                } else {
                    s.split(&sep).map(|p| Value::String(p.to_string())).collect()
                };
                let parts = if let Some(l) = limit {
                    parts.into_iter().take(l).collect()
                } else {
                    parts
                };
                let arr = Object::new_array_from(parts);
                Ok(Value::Object(Rc::new(RefCell::new(arr))))
            }
            _ => Ok(Value::Undefined),
        }
    }))));

    proto_clone.borrow_mut().set("concat", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        match crate::builtins::get_native_this() {
            Some(Value::String(s)) => {
                let rest: String = args.iter().map(to_js_string).collect();
                Ok(Value::String(format!("{}{}", s, rest)))
            }
            _ => Ok(Value::Undefined),
        }
    }))));
}

/// Install repeat method
fn install_repeat_method(proto: &Rc<RefCell<Object>>) {
    let proto_clone = Rc::clone(proto);
    proto_clone.borrow_mut().set("repeat", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        match crate::builtins::get_native_this() {
            Some(Value::String(s)) => {
                let count = args.first().map(|v| to_number(v) as usize).unwrap_or(0);
                Ok(Value::String(s.repeat(count)))
            }
            _ => Ok(Value::Undefined),
        }
    }))));
}

/// Install padStart method
fn install_pad_start_method(proto: &Rc<RefCell<Object>>) {
    let proto_clone = Rc::clone(proto);
    proto_clone.borrow_mut().set("padStart", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        match crate::builtins::get_native_this() {
            Some(Value::String(s)) => {
                let target = args.first().map(|v| to_number(v) as usize).unwrap_or(0);
                let pad = args.get(1).map(to_js_string).unwrap_or_else(|| " ".to_string());
                if s.len() >= target {
                    return Ok(Value::String(s.clone()));
                }
                let pad_len = target - s.len();
                let pad_count = (pad_len + pad.len() - 1) / pad.len();
                let padding: String = pad.repeat(pad_count);
                Ok(Value::String(format!("{}{}", &padding[..pad_len], s)))
            }
            _ => Ok(Value::Undefined),
        }
    }))));
}

/// Install padEnd method
fn install_pad_end_method(proto: &Rc<RefCell<Object>>) {
    let proto_clone = Rc::clone(proto);
    proto_clone.borrow_mut().set("padEnd", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        match crate::builtins::get_native_this() {
            Some(Value::String(s)) => {
                let target = args.first().map(|v| to_number(v) as usize).unwrap_or(0);
                let pad = args.get(1).map(to_js_string).unwrap_or_else(|| " ".to_string());
                if s.len() >= target {
                    return Ok(Value::String(s.clone()));
                }
                let pad_len = target - s.len();
                let pad_count = (pad_len + pad.len() - 1) / pad.len();
                let padding: String = pad.repeat(pad_count);
                Ok(Value::String(format!("{}{}", s, &padding[..pad_len])))
            }
            _ => Ok(Value::Undefined),
        }
    }))));
}

/// Install repeat and pad methods
fn install_repeat_pad_methods(proto: &Rc<RefCell<Object>>) {
    install_repeat_method(proto);
    install_pad_start_method(proto);
    install_pad_end_method(proto);
}

/// Install replace, match, search methods
fn install_replace_match_methods(proto: &Rc<RefCell<Object>>) {
    let proto_clone = Rc::clone(proto);

    proto_clone.borrow_mut().set("replace", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        match crate::builtins::get_native_this() {
            Some(Value::String(s)) => {
                let search = args.first().map(to_js_string).unwrap_or_default();
                let replace = args.get(1).map(to_js_string).unwrap_or_default();
                let new_s = if let Some(pos) = s.find(&search) {
                    format!("{}{}{}", &s[..pos], replace, &s[pos + search.len()..])
                } else {
                    s.clone()
                };
                Ok(Value::String(new_s))
            }
            _ => Ok(Value::Undefined),
        }
    }))));

    proto_clone.borrow_mut().set("match", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        match crate::builtins::get_native_this() {
            Some(Value::String(s)) => {
                let pattern = args.first().map(to_js_string).unwrap_or_default();
                Ok(Value::Boolean(s.contains(&pattern)))
            }
            _ => Ok(Value::Undefined),
        }
    }))));

    proto_clone.borrow_mut().set("search", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        match crate::builtins::get_native_this() {
            Some(Value::String(s)) => {
                let pattern = args.first().map(to_js_string).unwrap_or_default();
                Ok(Value::Number(s.find(&pattern).map(|i| i as f64).unwrap_or(-1.0)))
            }
            _ => Ok(Value::Undefined),
        }
    }))));
}

/// Install toString and valueOf methods
fn install_to_string_methods(proto: &Rc<RefCell<Object>>) {
    let proto_clone = Rc::clone(proto);

    proto_clone.borrow_mut().set("toString", Value::NativeFunction(Rc::new(NativeFunction::new(move |_| {
        match crate::builtins::get_native_this() {
            Some(Value::String(s)) => Ok(Value::String(s.clone())),
            Some(v) => Ok(Value::String(to_js_string(&v))),
            _ => Ok(Value::Undefined),
        }
    }))));

    proto_clone.borrow_mut().set("valueOf", Value::NativeFunction(Rc::new(NativeFunction::new(move |_| {
        match crate::builtins::get_native_this() {
            Some(Value::String(s)) => Ok(Value::String(s.clone())),
            Some(v) => Ok(Value::String(to_js_string(&v))),
            _ => Ok(Value::Undefined),
        }
    }))));
}

/// Install all String.prototype methods (shared, not per-access)
fn install_string_methods(proto: &Rc<RefCell<Object>>) {
    install_basic_methods(proto);
    install_search_methods(proto);
    install_case_methods(proto);
    install_trim_methods(proto);
    install_slice_methods(proto);
    install_split_concat_methods(proto);
    install_repeat_pad_methods(proto);
    install_replace_match_methods(proto);
    install_to_string_methods(proto);
}
