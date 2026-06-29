//! Host functions - bridge between JS and Rust
//!
//! These functions expose the existing bridge FFI to the JS interpreter.

use std::rc::Rc;
use std::cell::RefCell;

use crate::js_runtime::value::{Value, Object, ObjectKind, NativeFunction, to_js_string, to_number};

/// Register Ink host functions and globals
pub fn register_host_functions(ctx: &mut crate::js_runtime::Context) {
    // Register FFI wrapper functions
    register_ffi_wrappers(ctx);
    
    // Register Ink component tags
    register_ink_tags(ctx);
}

/// Register the FFI wrapper functions that call into Rust bridge
fn register_ffi_wrappers(ctx: &mut crate::js_runtime::Context) {
    // __ink_call - generic bridge call
    let _ctx_ref = Rc::new(RefCell::new(())); // Placeholder for context
    ctx.set_global("__ink_call".to_string(), Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let method = args.first().map(|v| to_js_string(v)).unwrap_or_default();
        let args_json = args.get(1).map(|v| to_js_string(v)).unwrap_or_else(|| "[]".to_string());
        
        let result = crate::bridge::ffi::call_ink_ffi(&method, &args_json);
        Ok(Value::String(result))
    }))));
    
    // __ink_call_fast - fast path FFI
    ctx.set_global("__ink_call_fast".to_string(), Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let method_or_id = args.first().cloned().unwrap_or(Value::Undefined);
        let a = args.get(1).map(|v| to_number(v)).unwrap_or(0.0);
        let b = args.get(2).map(|v| to_number(v)).unwrap_or(0.0);
        let c = args.get(3).map(|v| to_number(v)).unwrap_or(0.0);
        let d = args.get(4).map(|v| to_number(v)).unwrap_or(0.0);
        let e = args.get(5).map(|v| to_number(v)).unwrap_or(0.0);
        
        let result = if let Value::Number(id) = method_or_id {
            // Fast path: method ID
            crate::bridge::ffi::call_ink_ffi_fast(id as u32, a, b, c, d, e)
        } else {
            // Slow path: method name
            let method_name = to_js_string(&method_or_id);
            if let Some(id) = crate::bridge::ffi::get_fast_method_id(&method_name) {
                crate::bridge::ffi::call_ink_ffi_fast(id, a, b, c, d, e)
            } else {
                0.0
            }
        };
        
        Ok(Value::Number(result))
    }))));
    
    // Setup wrapper functions that mirror runtime.js expectations
    setup_bridge_wrappers(ctx);
}

/// Setup the bridge wrapper functions expected by runtime.js
fn setup_bridge_wrappers(ctx: &mut crate::js_runtime::Context) {
    // __ink_create_root
    ctx.set_global("__ink_create_root".to_string(), Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
        let result = crate::bridge::ffi::call_ink_ffi("create_root", "[]");
        Ok(Value::Number(result.parse::<f64>().unwrap_or(0.0)))
    }))));
    
    // __ink_destroy_root
    ctx.set_global("__ink_destroy_root".to_string(), Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let id = args.first().map(|v| to_number(v) as u32).unwrap_or(0);
        let _ = crate::bridge::ffi::call_ink_ffi("destroy_root", &format!("[{}]", id));
        Ok(Value::Undefined)
    }))));
    
    // __ink_create_node
    ctx.set_global("__ink_create_node".to_string(), Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let tag = args.first().map(|v| to_js_string(v)).unwrap_or_default();
        let props = args.get(1).map(|v| to_js_string(v)).unwrap_or_default();
        // Props is already JSON, don't wrap it in quotes
        let result = crate::bridge::ffi::call_ink_ffi("create_node", &format!("[\"{}\",{}]", tag, props));
        Ok(Value::Number(result.parse::<f64>().unwrap_or(0.0)))
    }))));
    
    // __ink_create_text_node
    ctx.set_global("__ink_create_text_node".to_string(), Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let text = args.first().map(|v| to_js_string(v)).unwrap_or_default();
        let result = crate::bridge::ffi::call_ink_ffi("create_text_node", &format!("[\"{}\"]", text));
        Ok(Value::Number(result.parse::<f64>().unwrap_or(0.0)))
    }))));
    
    // __ink_append_child
    ctx.set_global("__ink_append_child".to_string(), Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let parent = args.first().map(|v| to_js_string(v)).unwrap_or_default();
        let child = args.get(1).map(|v| to_js_string(v)).unwrap_or_default();
        let result = crate::bridge::ffi::call_ink_ffi("append_child", &format!("[{},{}]", parent, child));
        Ok(Value::Boolean(result == "true"))
    }))));
    
    // __ink_remove_child
    ctx.set_global("__ink_remove_child".to_string(), Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let parent = args.first().map(|v| to_js_string(v)).unwrap_or_default();
        let child = args.get(1).map(|v| to_js_string(v)).unwrap_or_default();
        let result = crate::bridge::ffi::call_ink_ffi("remove_child", &format!("[{},{}]", parent, child));
        Ok(Value::Boolean(result == "true"))
    }))));
    
    // __ink_insert_before
    ctx.set_global("__ink_insert_before".to_string(), Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let parent = args.first().map(|v| to_js_string(v)).unwrap_or_default();
        let child = args.get(1).map(|v| to_js_string(v)).unwrap_or_default();
        let before = args.get(2).map(|v| to_js_string(v)).unwrap_or_default();
        let result = crate::bridge::ffi::call_ink_ffi("insert_before", &format!("[{},{},{}]", parent, child, before));
        Ok(Value::Boolean(result == "true"))
    }))));
    
    // __ink_commit_update
    ctx.set_global("__ink_commit_update".to_string(), Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let id = args.first().map(|v| to_js_string(v)).unwrap_or_default();
        let props = args.get(1).map(|v| to_js_string(v)).unwrap_or_default();
        let result = crate::bridge::ffi::call_ink_ffi("commit_update", &format!("[\"{}\",{}]", id, props));
        Ok(Value::Boolean(result == "true"))
    }))));
    
    // __ink_set_text
    ctx.set_global("__ink_set_text".to_string(), Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let id = args.first().map(|v| to_js_string(v)).unwrap_or_default();
        let text = args.get(1).map(|v| to_js_string(v)).unwrap_or_default();
        let result = crate::bridge::ffi::call_ink_ffi("set_text", &format!("[\"{}\",\"{}\"]", id, text));
        Ok(Value::Boolean(result == "true"))
    }))));
    
    // __ink_commit
    ctx.set_global("__ink_commit".to_string(), Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
        let _ = crate::bridge::ffi::call_ink_ffi("commit", "[]");
        Ok(Value::Undefined)
    }))));
    
    // __ink_is_dirty
    ctx.set_global("__ink_is_dirty".to_string(), Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
        let result = crate::bridge::ffi::call_ink_ffi("is_dirty", "[]");
        Ok(Value::Boolean(result == "true"))
    }))));
    
    // __ink_clear_dirty
    ctx.set_global("__ink_clear_dirty".to_string(), Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
        let _ = crate::bridge::ffi::call_ink_ffi("clear_dirty", "[]");
        Ok(Value::Undefined)
    }))));
    
    // __ink_measure_text
    ctx.set_global("__ink_measure_text".to_string(), Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let text = args.first().map(|v| to_js_string(v)).unwrap_or_default();
        let width = args.get(1).map(|v| to_number(v) as u32).unwrap_or(80);
        let result = crate::bridge::ffi::call_ink_ffi("measure_text", &format!("[\"{}\",{}]", text, width));
        
        // Parse "width,height" response
        let parts: Vec<&str> = result.split(',').collect();
        let w = parts.get(0).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
        let h = parts.get(1).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
        
        let obj = Object::new(ObjectKind::Ordinary);
        let obj = Rc::new(RefCell::new(obj));
        obj.borrow_mut().set("width", Value::Number(w));
        obj.borrow_mut().set("height", Value::Number(h));
        Ok(Value::Object(obj))
    }))));
    
    // __ink_measure_element
    ctx.set_global("__ink_measure_element".to_string(), Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let id = args.first().map(|v| to_js_string(v)).unwrap_or_default();
        let result = crate::bridge::ffi::call_ink_ffi("measure_element", &format!("[{}]", id));
        
        if result == "null" {
            return Ok(Value::Null);
        }
        
        let parts: Vec<&str> = result.split(',').collect();
        let w = parts.get(0).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
        let h = parts.get(1).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
        
        let obj = Object::new(ObjectKind::Ordinary);
        let obj = Rc::new(RefCell::new(obj));
        obj.borrow_mut().set("width", Value::Number(w));
        obj.borrow_mut().set("height", Value::Number(h));
        Ok(Value::Object(obj))
    }))));
    
    // __ink_exit
    ctx.set_global("__ink_exit".to_string(), Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let code = args.first().map(|v| to_number(v) as i32).unwrap_or(0);
        let _ = crate::bridge::ffi::call_ink_ffi("exit", &format!("[{}]", code));
        Ok(Value::Undefined)
    }))));
    
    // __ink_should_exit
    ctx.set_global("__ink_should_exit".to_string(), Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
        let result = crate::bridge::ffi::call_ink_ffi("should_exit", "[]");
        Ok(Value::Boolean(result == "true"))
    }))));
    
    // __ink_get_exit_code
    ctx.set_global("__ink_get_exit_code".to_string(), Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
        let result = crate::bridge::ffi::call_ink_ffi("get_exit_code", "[]");
        Ok(Value::Number(result.parse::<f64>().unwrap_or(0.0)))
    }))));
    
    // __ink_reset_exit
    ctx.set_global("__ink_reset_exit".to_string(), Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
        let _ = crate::bridge::ffi::call_ink_ffi("reset_exit", "[]");
        Ok(Value::Undefined)
    }))));
    
    // __ink_set_exit_requested
    ctx.set_global("__ink_set_exit_requested".to_string(), Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
        let _ = crate::bridge::ffi::call_ink_ffi("set_exit_requested", "[]");
        Ok(Value::Undefined)
    }))));
    
    // __ink_set_terminal_size
    ctx.set_global("__ink_set_terminal_size".to_string(), Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let width = args.first().map(|v| to_number(v) as u32).unwrap_or(80);
        let height = args.get(1).map(|v| to_number(v) as u32).unwrap_or(24);
        let _ = crate::bridge::ffi::call_ink_ffi("set_terminal_size", &format!("[{},{}]", width, height));
        Ok(Value::Undefined)
    }))));
    
    // __ink_get_terminal_size
    ctx.set_global("__ink_get_terminal_size".to_string(), Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
        let result = crate::bridge::ffi::call_ink_ffi("get_terminal_size", "[]");
        let parts: Vec<&str> = result.split(',').collect();
        let w = parts.get(0).and_then(|s| s.parse::<f64>().ok()).unwrap_or(80.0);
        let h = parts.get(1).and_then(|s| s.parse::<f64>().ok()).unwrap_or(24.0);
        
        let obj = Object::new(ObjectKind::Ordinary);
        let obj = Rc::new(RefCell::new(obj));
        obj.borrow_mut().set("width", Value::Number(w));
        obj.borrow_mut().set("height", Value::Number(h));
        Ok(Value::Object(obj))
    }))));
    
    // __ink_get_node_tag
    ctx.set_global("__ink_get_node_tag".to_string(), Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let id = args.first().map(|v| to_number(v) as u32).unwrap_or(0);
        let result = crate::bridge::ffi::call_ink_ffi("get_node_tag", &format!("[{}]", id));
        if result == "null" {
            Ok(Value::Null)
        } else {
            Ok(Value::String(result.trim_matches('"').to_string()))
        }
    }))));
    
    // __ink_get_node_text
    ctx.set_global("__ink_get_node_text".to_string(), Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let id = args.first().map(|v| to_number(v) as u32).unwrap_or(0);
        let result = crate::bridge::ffi::call_ink_ffi("get_node_text", &format!("[{}]", id));
        if result == "null" {
            Ok(Value::Null)
        } else {
            Ok(Value::String(result.trim_matches('"').to_string()))
        }
    }))));
    
    // __ink_get_node_children
    ctx.set_global("__ink_get_node_children".to_string(), Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let id = args.first().map(|v| to_number(v) as u32).unwrap_or(0);
        let result = crate::bridge::ffi::call_ink_ffi("get_node_children", &format!("[{}]", id));
        
        if result == "null" {
            return Ok(Value::Null);
        }
        
        // Parse array response like "[1,2,3]"
        let nums: Vec<f64> = result.trim_matches(|c| c == '[' || c == ']')
            .split(',')
            .filter_map(|s| s.trim().parse::<f64>().ok())
            .collect();
        
        Ok(Value::Object(Rc::new(RefCell::new(Object::new_array(nums.len())))))
    }))));
    
    // __ink_get_node_prop
    ctx.set_global("__ink_get_node_prop".to_string(), Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let id = args.first().map(|v| to_number(v) as u32).unwrap_or(0);
        let prop = args.get(1).map(|v| to_js_string(v)).unwrap_or_default();
        let result = crate::bridge::ffi::call_ink_ffi("get_node_prop", &format!("[{},\"{}\"]", id, prop));
        if result == "null" {
            Ok(Value::Null)
        } else {
            Ok(Value::String(result.trim_matches('"').to_string()))
        }
    }))));
    
    // __ink_get_root_id
    ctx.set_global("__ink_get_root_id".to_string(), Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
        let result = crate::bridge::ffi::call_ink_ffi("get_root_id", "[]");
        if result == "null" {
            Ok(Value::Null)
        } else {
            Ok(Value::Number(result.parse::<f64>().unwrap_or(0.0)))
        }
    }))));
    
    // __ink_calculate_layout
    ctx.set_global("__ink_calculate_layout".to_string(), Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
        let result = crate::bridge::ffi::call_ink_ffi("calculate_layout", "[]");
        Ok(Value::Boolean(result == "true"))
    }))));
    
    // __ink_get_layout
    ctx.set_global("__ink_get_layout".to_string(), Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let id = args.first().map(|v| to_js_string(v)).unwrap_or_default();
        let result = crate::bridge::ffi::call_ink_ffi("get_layout", &format!("[{}]", id));
        
        if result == "null" {
            return Ok(Value::Null);
        }
        
        let parts: Vec<&str> = result.split(',').collect();
        let x = parts.get(0).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
        let y = parts.get(1).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
        let w = parts.get(2).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
        let h = parts.get(3).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
        
        let obj = Object::new(ObjectKind::Ordinary);
        let obj = Rc::new(RefCell::new(obj));
        obj.borrow_mut().set("left", Value::Number(x));
        obj.borrow_mut().set("top", Value::Number(y));
        obj.borrow_mut().set("width", Value::Number(w));
        obj.borrow_mut().set("height", Value::Number(h));
        Ok(Value::Object(obj))
    }))));
    
    // Timer functions
    // __ink_set_timeout
    ctx.set_global("__ink_set_timeout".to_string(), Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let callback = args.first().map(|v| to_js_string(v)).unwrap_or_default();
        let delay = args.get(1).map(|v| to_number(v) as u64).unwrap_or(0);
        let result = crate::bridge::ffi::call_ink_ffi("set_timeout", &format!("[\"{}\",{}]", callback, delay));
        Ok(Value::Number(result.parse::<f64>().unwrap_or(0.0)))
    }))));
    
    // __ink_set_interval
    ctx.set_global("__ink_set_interval".to_string(), Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let callback = args.first().map(|v| to_js_string(v)).unwrap_or_default();
        let interval = args.get(1).map(|v| to_number(v) as u64).unwrap_or(0);
        let result = crate::bridge::ffi::call_ink_ffi("set_interval", &format!("[\"{}\",{}]", callback, interval));
        Ok(Value::Number(result.parse::<f64>().unwrap_or(0.0)))
    }))));
    
    // __ink_clear_timer
    ctx.set_global("__ink_clear_timer".to_string(), Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let id = args.first().map(|v| to_number(v) as u32).unwrap_or(0);
        let _ = crate::bridge::ffi::call_ink_ffi("clear_timer", &format!("[{}]", id));
        Ok(Value::Undefined)
    }))));
}

/// Register Ink component tag globals
fn register_ink_tags(ctx: &mut crate::js_runtime::Context) {
    // Component tags
    ctx.set_global("Box".to_string(), Value::String("ink-box".to_string()));
    ctx.set_global("Text".to_string(), Value::String("ink-text".to_string()));
    ctx.set_global("Static".to_string(), Value::String("ink-static".to_string()));
    ctx.set_global("Newline".to_string(), Value::String("ink-newline".to_string()));
    ctx.set_global("Spacer".to_string(), Value::String("ink-spacer".to_string()));
    
    // ink namespace
    let ink_ns = Object::new(ObjectKind::Ordinary);
    let ink = Rc::new(RefCell::new(ink_ns));
    ink.borrow_mut().set("Box", Value::String("ink-box".to_string()));
    ink.borrow_mut().set("Text", Value::String("ink-text".to_string()));
    ink.borrow_mut().set("Static", Value::String("ink-static".to_string()));
    ink.borrow_mut().set("Newline", Value::String("ink-newline".to_string()));
    ink.borrow_mut().set("Spacer", Value::String("ink-spacer".to_string()));
    
    ctx.set_global("ink".to_string(), Value::Object(ink));
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_host_registration() {
        let mut ctx = crate::js_runtime::Context::new().unwrap();
        crate::js_runtime::host::register_host_functions(&mut ctx);
        
        // Check component tags
        assert_eq!(ctx.get_global("Box"), Some(crate::js_runtime::value::Value::String("ink-box".to_string())));
        assert!(ctx.get_global("ink").is_some());
    }
}
