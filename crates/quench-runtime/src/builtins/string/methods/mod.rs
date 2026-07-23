//! String method implementations organized by category

pub mod at;
pub mod basic;
pub mod case;
pub mod concat;
pub mod pad;
pub mod replace;
pub mod search;
pub mod slice;
pub mod to_string;
pub mod trim;

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{Object, Value};

/// String payload from `this` for String.prototype methods (primitive or boxed).
pub(crate) fn this_js_string() -> Option<String> {
    match crate::builtins::get_native_this()? {
        Value::String(s) => Some(s),
        Value::Object(obj) => match obj.borrow().get("_value") {
            Some(Value::String(s)) => Some(s),
            _ => None,
        },
        _ => None,
    }
}

/// Install all String.prototype methods on the prototype object
pub fn install_string_methods(proto: &Rc<RefCell<Object>>) {
    basic::install_basic_methods(proto);
    search::install_search_methods(proto);
    case::install_case_methods(proto);
    trim::install_trim_methods(proto);
    slice::install_slice_methods(proto);
    concat::install_split_concat_methods(proto);
    pad::install_repeat_pad_methods(proto);
    to_string::install_to_string_methods(proto);
    at::install_at_method(proto);
}
