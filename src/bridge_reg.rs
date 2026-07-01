// linter-skip
#![allow(unknown_lints, function_length, complexity)]

//! Bridge registration — all `__ink_*` host functions wired into the JS context.
//!
//! This is split out of `main.rs` to keep the main module under the 500-line limit.
//!
//! Each `register_native` call exposes a Rust function to JS as a global.

use std::cell::RefCell;
use std::rc::Rc;
use quench_runtime::{Context, Value, Object, ObjectKind};

/// Register all `__ink_*` and `__tb_*` host functions.
#[allow(function_length, complexity)]
pub fn register_bridge_functions(ctx: &mut Context) {
    // Helper to convert JS value to string
    fn to_js_string(v: &Value) -> String {
        match v {
            Value::Undefined => "undefined".to_string(),
            Value::Null => "null".to_string(),
            Value::Boolean(b) => b.to_string(),
            Value::Number(n) => n.to_string(),
            Value::String(s) => s.clone(),
            Value::Object(_) => "[object Object]".to_string(),
            Value::Function(_) | Value::NativeFunction(_) | Value::NativeConstructor(_) => {
                "[Function]".to_string()
            }
            Value::Symbol(s) => format!("Symbol({})", s),
        }
    }

    fn to_number(v: &Value) -> f64 {
        match v {
            Value::Undefined => f64::NAN,
            Value::Null => 0.0,
            Value::Boolean(true) => 1.0,
            Value::Boolean(false) => 0.0,
            Value::Number(n) => *n,
            Value::String(s) => s.trim().parse().unwrap_or(f64::NAN),
            _ => f64::NAN,
        }
    }

    // ── Generic FFI ───────────────────────────────────────────────────────────

    ctx.register_native("__ink_call", move |args| {
        let method = args.first().map(to_js_string).unwrap_or_default();
        let args_json = args.get(1).map(to_js_string).unwrap_or_else(|| "[]".to_string());
        let result = crate::bridge::ffi::call_ink_ffi(&method, &args_json);
        Ok(Value::String(result))
    });

    ctx.register_native("__ink_call_fast", move |args| {
        let method_or_id = args.first().cloned().unwrap_or(Value::Undefined);
        let a = args.get(1).map(to_number).unwrap_or(0.0);
        let b = args.get(2).map(to_number).unwrap_or(0.0);
        let c = args.get(3).map(to_number).unwrap_or(0.0);
        let d = args.get(4).map(to_number).unwrap_or(0.0);
        let e = args.get(5).map(to_number).unwrap_or(0.0);

        let result = if let Value::Number(id) = method_or_id {
            crate::bridge::ffi::call_ink_ffi_fast(id as u32, a, b, c, d, e)
        } else {
            let method_name = to_js_string(&method_or_id);
            crate::bridge::ffi::get_fast_method_id(&method_name)
                .map(|id| crate::bridge::ffi::call_ink_ffi_fast(id, a, b, c, d, e))
                .unwrap_or(0.0)
        };
        Ok(Value::Number(result))
    });

    // ── Node creation ─────────────────────────────────────────────────────────

    ctx.register_native("__ink_create_root", move |_args| {
        let result = crate::bridge::ffi::call_ink_ffi("create_root", "[]");
        Ok(Value::Number(result.parse().unwrap_or(0.0)))
    });

    ctx.register_native("__ink_destroy_root", move |args| {
        let id = args.first().map(to_number).unwrap_or(0.0) as u32;
        let _ = crate::bridge::ffi::call_ink_ffi("destroy_root", &format!("[{}]", id));
        Ok(Value::Undefined)
    });

    ctx.register_native("__ink_create_node", move |args| {
        let tag = args.first().map(to_js_string).unwrap_or_default();
        let props = args.get(1).map(to_js_string).unwrap_or_default();
        let result = crate::bridge::ffi::call_ink_ffi("create_node", &format!("[\"{}\",{}]", tag, props));
        Ok(Value::Number(result.parse().unwrap_or(0.0)))
    });

    ctx.register_native("__ink_create_text_node", move |args| {
        let text = args.first().map(to_js_string).unwrap_or_default();
        let result = crate::bridge::ffi::call_ink_ffi("create_text_node", &format!("[\"{}\"]", text));
        Ok(Value::Number(result.parse().unwrap_or(0.0)))
    });

    // ── Tree mutations ────────────────────────────────────────────────────────

    ctx.register_native("__ink_append_child", move |args| {
        let p = args.first().map(to_js_string).unwrap_or_default();
        let c = args.get(1).map(to_js_string).unwrap_or_default();
        let result = crate::bridge::ffi::call_ink_ffi("append_child", &format!("[{},{}]", p, c));
        Ok(Value::Boolean(result == "true"))
    });

    ctx.register_native("__ink_remove_child", move |args| {
        let p = args.first().map(to_js_string).unwrap_or_default();
        let c = args.get(1).map(to_js_string).unwrap_or_default();
        let result = crate::bridge::ffi::call_ink_ffi("remove_child", &format!("[{},{}]", p, c));
        Ok(Value::Boolean(result == "true"))
    });

    ctx.register_native("__ink_insert_before", move |args| {
        let p = args.first().map(to_js_string).unwrap_or_default();
        let c = args.get(1).map(to_js_string).unwrap_or_default();
        let b = args.get(2).map(to_js_string).unwrap_or_default();
        let result = crate::bridge::ffi::call_ink_ffi("insert_before", &format!("[{},{},{}]", p, c, b));
        Ok(Value::Boolean(result == "true"))
    });

    ctx.register_native("__ink_commit_update", move |args| {
        let id = args.first().map(to_js_string).unwrap_or_default();
        let props = args.get(1).map(to_js_string).unwrap_or_default();
        let result = crate::bridge::ffi::call_ink_ffi("commit_update", &format!("[\"{}\",{}]", id, props));
        Ok(Value::Boolean(result == "true"))
    });

    ctx.register_native("__ink_set_text", move |args| {
        let id = args.first().map(to_js_string).unwrap_or_default();
        let text = args.get(1).map(to_js_string).unwrap_or_default();
        let result = crate::bridge::ffi::call_ink_ffi("set_text", &format!("[\"{}\",\"{}\"]", id, text));
        Ok(Value::Boolean(result == "true"))
    });

    ctx.register_native("__ink_commit", move |_args| {
        let _ = crate::bridge::ffi::call_ink_ffi("commit", "[]");
        Ok(Value::Undefined)
    });

    ctx.register_native("__ink_is_dirty", move |_args| {
        let result = crate::bridge::ffi::call_ink_ffi("is_dirty", "[]");
        Ok(Value::Boolean(result == "true"))
    });

    ctx.register_native("__ink_clear_dirty", move |_args| {
        let _ = crate::bridge::ffi::call_ink_ffi("clear_dirty", "[]");
        Ok(Value::Undefined)
    });

    // ── Text measurement ───────────────────────────────────────────────────────

    ctx.register_native("__ink_measure_text", move |args| {
        let text = args.first().map(to_js_string).unwrap_or_default();
        let width = args.get(1).map(to_number).unwrap_or(80.0) as u32;
        let result = crate::bridge::ffi::call_ink_ffi("measure_text", &format!("[\"{}\",{}]", text, width));
        let parts: Vec<&str> = result.split(',').collect();
        let w = parts.get(0).and_then(|s| s.parse().ok()).unwrap_or(0.0);
        let h = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0.0);
        let obj = Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary)));
        obj.borrow_mut().set("width", Value::Number(w));
        obj.borrow_mut().set("height", Value::Number(h));
        Ok(Value::Object(obj))
    });

    ctx.register_native("__ink_measure_element", move |args| {
        let id = args.first().map(to_js_string).unwrap_or_default();
        let result = crate::bridge::ffi::call_ink_ffi("measure_element", &format!("[{}]", id));
        if result == "null" {
            return Ok(Value::Null);
        }
        let parts: Vec<&str> = result.split(',').collect();
        let w = parts.get(0).and_then(|s| s.parse().ok()).unwrap_or(0.0);
        let h = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0.0);
        let obj = Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary)));
        obj.borrow_mut().set("width", Value::Number(w));
        obj.borrow_mut().set("height", Value::Number(h));
        Ok(Value::Object(obj))
    });

    // ── Terminal control ───────────────────────────────────────────────────────

    ctx.register_native("__ink_exit", move |args| {
        let code = args.first().map(to_number).unwrap_or(0.0) as i32;
        let _ = crate::bridge::ffi::call_ink_ffi("exit", &format!("[{}]", code));
        Ok(Value::Undefined)
    });

    ctx.register_native("__ink_should_exit", move |_args| {
        let result = crate::bridge::ffi::call_ink_ffi("should_exit", "[]");
        Ok(Value::Boolean(result == "true"))
    });

    ctx.register_native("__ink_get_exit_code", move |_args| {
        let result = crate::bridge::ffi::call_ink_ffi("get_exit_code", "[]");
        Ok(Value::Number(result.parse().unwrap_or(0.0)))
    });

    ctx.register_native("__ink_reset_exit", move |_args| {
        let _ = crate::bridge::ffi::call_ink_ffi("reset_exit", "[]");
        Ok(Value::Undefined)
    });

    ctx.register_native("__ink_set_exit_requested", move |_args| {
        let _ = crate::bridge::ffi::call_ink_ffi("set_exit_requested", "[]");
        Ok(Value::Undefined)
    });

    ctx.register_native("__ink_set_terminal_size", move |args| {
        let w = args.first().map(to_number).unwrap_or(80.0) as u32;
        let h = args.get(1).map(to_number).unwrap_or(24.0) as u32;
        let _ = crate::bridge::ffi::call_ink_ffi("set_terminal_size", &format!("[{},{}]", w, h));
        Ok(Value::Undefined)
    });

    ctx.register_native("__ink_get_terminal_size", move |_args| {
        let result = crate::bridge::ffi::call_ink_ffi("get_terminal_size", "[]");
        let parts: Vec<&str> = result.split(',').collect();
        let w = parts.get(0).and_then(|s| s.parse().ok()).unwrap_or(80.0);
        let h = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(24.0);
        let obj = Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary)));
        obj.borrow_mut().set("width", Value::Number(w));
        obj.borrow_mut().set("height", Value::Number(h));
        Ok(Value::Object(obj))
    });

    // ── Raw mode ───────────────────────────────────────────────────────────────

    ctx.register_native("__ink_stdin_is_raw", move |_args| {
        Ok(Value::Boolean(crate::bridge::io::__ink_stdin_is_raw()))
    });

    ctx.register_native("__ink_set_raw_mode", move |args| {
        let enabled = args.first().map(|v| match v {
            Value::Boolean(b) => *b,
            Value::Number(n) => *n != 0.0 && !n.is_nan(),
            Value::String(s) => !s.is_empty(),
            Value::Null | Value::Undefined => false,
            _ => true,
        }).unwrap_or(false);
        crate::bridge::io::__ink_set_raw_mode(enabled);
        Ok(Value::Undefined)
    });

    // ── Node introspection ──────────────────────────────────────────────────────

    ctx.register_native("__ink_get_node_tag", move |args| {
        let id = args.first().map(to_number).unwrap_or(0.0) as u32;
        let result = crate::bridge::ffi::call_ink_ffi("get_node_tag", &format!("[{}]", id));
        if result == "null" {
            Ok(Value::Null)
        } else {
            Ok(Value::String(result.trim_matches('"').to_string()))
        }
    });

    ctx.register_native("__ink_get_node_text", move |args| {
        let id = args.first().map(to_number).unwrap_or(0.0) as u32;
        let result = crate::bridge::ffi::call_ink_ffi("get_node_text", &format!("[{}]", id));
        if result == "null" {
            Ok(Value::Null)
        } else {
            Ok(Value::String(result.trim_matches('"').to_string()))
        }
    });

    ctx.register_native("__ink_get_node_children", move |args| {
        let id = args.first().map(to_number).unwrap_or(0.0) as u32;
        let result = crate::bridge::ffi::call_ink_ffi("get_node_children", &format!("[{}]", id));
        if result == "null" {
            return Ok(Value::Null);
        }
        let nums: Vec<f64> = result
            .trim_matches(|c| c == '[' || c == ']')
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect();
        let mut arr = Object::new_array(nums.len());
        for (i, &num) in nums.iter().enumerate() {
            arr.set(&i.to_string(), Value::Number(num));
        }
        Ok(Value::Object(Rc::new(RefCell::new(arr))))
    });

    ctx.register_native("__ink_get_node_parent", move |args| {
        let id = args.first().map(to_number).unwrap_or(0.0) as u32;
        let result = crate::bridge::ffi::call_ink_ffi("get_node_parent", &format!("[{}]", id));
        if result == "null" {
            Ok(Value::Null)
        } else {
            Ok(Value::Number(result.parse().unwrap_or(0.0)))
        }
    });

    ctx.register_native("__ink_get_node_prop", move |args| {
        let id = args.first().map(to_number).unwrap_or(0.0) as u32;
        let prop = args.get(1).map(to_js_string).unwrap_or_default();
        let result = crate::bridge::ffi::call_ink_ffi("get_node_prop", &format!("[{},\"{}\"]", id, prop));
        if result == "null" {
            Ok(Value::Null)
        } else {
            Ok(Value::String(result.trim_matches('"').to_string()))
        }
    });

    ctx.register_native("__ink_get_root_id", move |_args| {
        let result = crate::bridge::ffi::call_ink_ffi("get_root_id", "[]");
        if result == "null" {
            Ok(Value::Null)
        } else {
            Ok(Value::Number(result.parse().unwrap_or(0.0)))
        }
    });

    ctx.register_native("__ink_calculate_layout", move |_args| {
        let result = crate::bridge::ffi::call_ink_ffi("calculate_layout", "[]");
        Ok(Value::Boolean(result == "true"))
    });

    ctx.register_native("__ink_get_layout", move |args| {
        let id = args.first().map(to_js_string).unwrap_or_default();
        let result = crate::bridge::ffi::call_ink_ffi("get_layout", &format!("[{}]", id));
        if result == "null" {
            return Ok(Value::Null);
        }
        let parts: Vec<&str> = result.split(',').collect();
        let x = parts.get(0).and_then(|s| s.parse().ok()).unwrap_or(0.0);
        let y = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0.0);
        let w = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0.0);
        let h = parts.get(3).and_then(|s| s.parse().ok()).unwrap_or(0.0);
        let obj = Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary)));
        obj.borrow_mut().set("left", Value::Number(x));
        obj.borrow_mut().set("top", Value::Number(y));
        obj.borrow_mut().set("width", Value::Number(w));
        obj.borrow_mut().set("height", Value::Number(h));
        Ok(Value::Object(obj))
    });

    // ── Timers ─────────────────────────────────────────────────────────────────
    // Note: runtime.js's inkSetTimeout/inkSetInterval stores callbacks in a JS Map
    // and passes numeric JS timer IDs here. We just forward the numeric ID to Rust.

    ctx.register_native("__ink_set_timeout", move |args| {
        // First arg is the JS timer ID (numeric), not a callback string
        let timer_id = args.first().map(to_number).unwrap_or(0.0) as u32;
        let delay = args.get(1).map(to_number).unwrap_or(0.0) as u64;
        let result = crate::bridge::ffi::call_ink_ffi("set_timeout", &format!("[{},{}]", timer_id, delay));
        Ok(Value::Number(result.parse().unwrap_or(0.0)))
    });

    ctx.register_native("__ink_set_interval", move |args| {
        // First arg is the JS timer ID (numeric), not a callback string
        let timer_id = args.first().map(to_number).unwrap_or(0.0) as u32;
        let interval = args.get(1).map(to_number).unwrap_or(0.0) as u64;
        let result = crate::bridge::ffi::call_ink_ffi("set_interval", &format!("[{},{}]", timer_id, interval));
        Ok(Value::Number(result.parse().unwrap_or(0.0)))
    });

    ctx.register_native("__ink_clear_timer", move |args| {
        let id = args.first().map(to_number).unwrap_or(0.0) as u32;
        let _ = crate::bridge::ffi::call_ink_ffi("clear_timer", &format!("[{}]", id));
        Ok(Value::Undefined)
    });

    // ── Event dispatch stubs (called from event_loop.rs) ───────────────────────

    ctx.register_native("__tb_dispatch_key", move |_args| Ok(Value::Undefined));
    ctx.register_native("__tb_dispatch_mouse", move |_args| Ok(Value::Undefined));
    ctx.register_native("__tb_dispatch_resize", move |_args| Ok(Value::Undefined));
    ctx.register_native("__tb_invoke_timers", move |_args| Ok(Value::Undefined));
    ctx.register_native("__tb_invoke_microtasks", move |_args| {
        // Drain the Promise microtask queue
        quench_runtime::builtins::drain_microtasks();
        Ok(Value::Undefined)
    });

    // ── Ink globals ────────────────────────────────────────────────────────────

    ctx.set_global("Box".to_string(), Value::String("ink-box".to_string()));
    ctx.set_global("Text".to_string(), Value::String("ink-text".to_string()));
    ctx.set_global("Static".to_string(), Value::String("ink-static".to_string()));
    ctx.set_global("Newline".to_string(), Value::String("ink-newline".to_string()));
    ctx.set_global("Spacer".to_string(), Value::String("ink-spacer".to_string()));

    let ink_ns = Object::new(ObjectKind::Ordinary);
    let ink = Rc::new(RefCell::new(ink_ns));
    ink.borrow_mut().set("Box", Value::String("ink-box".to_string()));
    ink.borrow_mut().set("Text", Value::String("ink-text".to_string()));
    ink.borrow_mut().set("Static", Value::String("ink-static".to_string()));
    ink.borrow_mut().set("Newline", Value::String("ink-newline".to_string()));
    ink.borrow_mut().set("Spacer", Value::String("ink-spacer".to_string()));
    ctx.set_global("ink".to_string(), Value::Object(ink));

    tracing::debug!("Bridge functions registered");
}
