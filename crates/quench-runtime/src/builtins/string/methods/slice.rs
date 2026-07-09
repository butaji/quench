//! String slice methods (substring, slice)

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{to_number, NativeFunction, Object, Value};

/// Install slice/substring methods
pub fn install_slice_methods(proto: &Rc<RefCell<Object>>) {
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
