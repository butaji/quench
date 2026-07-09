// linter-skip
//! Bridge function registrations for Quench runtime
//!
//! Registers all __ink_* and __tb_* host functions that bridge
//! the Rust runtime to the JavaScript environment.

use std::rc::Rc;
use quench_runtime::{Context, Value, Object, ObjectKind};

/// Helper: Convert Value to JavaScript string representation
fn to_js_string(v: &Value) -> String {
    match v {
        Value::Undefined => "undefined".to_string(),
        Value::Null => "null".to_string(),
        Value::Boolean(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        Value::Object(_) | Value::ObjectId(_) => "[object Object]".to_string(),
        Value::Function(_) | Value::NativeFunction(_) | Value::NativeConstructor(_) => {
            "[Function]".to_string()
        }
        Value::Symbol(s) => format!("Symbol({})", s),
    }
}

/// Helper: Convert Value to f64 number
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

/// Helper: Create a layout object with x, y, width, height
fn make_layout_object(x: f64, y: f64, w: f64, h: f64) -> Value {
    let obj = Object::new(ObjectKind::Ordinary);
    let obj = Rc::new(std::cell::RefCell::new(obj));
    obj.borrow_mut().set("left", Value::Number(x));
    obj.borrow_mut().set("top", Value::Number(y));
    obj.borrow_mut().set("width", Value::Number(w));
    obj.borrow_mut().set("height", Value::Number(h));
    Value::Object(obj)
}

/// Helper: Create a size object with width, height
fn make_size_object(w: f64, h: f64) -> Value {
    let obj = Object::new(ObjectKind::Ordinary);
    let obj = Rc::new(std::cell::RefCell::new(obj));
    obj.borrow_mut().set("width", Value::Number(w));
    obj.borrow_mut().set("height", Value::Number(h));
    Value::Object(obj)
}

/// Helper: Parse comma-separated result into (x, y) or use defaults
fn parse_xy(result: &str, default: f64) -> (f64, f64) {
    let parts: Vec<&str> = result.split(',').collect();
    let x = parts.get(0).and_then(|s| s.parse::<f64>().ok()).unwrap_or(default);
    let y = parts.get(1).and_then(|s| s.parse::<f64>().ok()).unwrap_or(default);
    (x, y)
}

/// Helper: Parse comma-separated layout result into (x, y, w, h)
fn parse_layout(result: &str) -> (f64, f64, f64, f64) {
    let parts: Vec<&str> = result.split(',').collect();
    let x = parts.get(0).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
    let y = parts.get(1).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
    let w = parts.get(2).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
    let h = parts.get(3).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
    (x, y, w, h)
}

/// Register all bridge FFI functions with the JS runtime
pub fn register_bridge_functions(ctx: &mut Context) {
    register_call_functions(ctx);
    register_node_functions(ctx);
    register_manipulation_functions(ctx);
    register_measurement_functions(ctx);
    register_terminal_functions(ctx);
    register_introspection_functions(ctx);
    register_timer_functions(ctx);
    register_event_stubs(ctx);
    register_ink_tags(ctx);
    tracing::debug!("Bridge functions registered");
}

// =============================================================================
// Call functions
// =============================================================================

fn register_call_functions(ctx: &mut Context) {
    // __ink_call - generic bridge call
    ctx.register_native("__ink_call", move |args| {
        let method = args.first().map(|v| to_js_string(v)).unwrap_or_default();
        let args_json = args.get(1).map(|v| to_js_string(v)).unwrap_or_else(|| "[]".to_string());
        let result = crate::bridge::ffi::call_ink_ffi(&method, &args_json);
        Ok(Value::String(result))
    });

    // __ink_call_fast - fast path FFI
    ctx.register_native("__ink_call_fast", move |args| {
        let method_or_id = args.first().cloned().unwrap_or(Value::Undefined);
        let a = args.get(1).map(|v| to_number(v)).unwrap_or(0.0);
        let b = args.get(2).map(|v| to_number(v)).unwrap_or(0.0);
        let c = args.get(3).map(|v| to_number(v)).unwrap_or(0.0);
        let d = args.get(4).map(|v| to_number(v)).unwrap_or(0.0);
        let e = args.get(5).map(|v| to_number(v)).unwrap_or(0.0);

        let result = if let Value::Number(id) = method_or_id {
            crate::bridge::ffi::call_ink_ffi_fast(id as u32, a, b, c, d, e)
        } else {
            let method_name = to_js_string(&method_or_id);
            if let Some(id) = crate::bridge::ffi::get_fast_method_id(&method_name) {
                crate::bridge::ffi::call_ink_ffi_fast(id, a, b, c, d, e)
            } else {
                0.0
            }
        };

        Ok(Value::Number(result))
    });
}

// =============================================================================
// Node creation functions
// =============================================================================

fn register_node_functions(ctx: &mut Context) {
    ctx.register_native("__ink_create_root", move |_args| {
        let result = crate::bridge::ffi::call_ink_ffi("create_root", "[]");
        Ok(Value::Number(result.parse::<f64>().unwrap_or(0.0)))
    });

    ctx.register_native("__ink_destroy_root", move |args| {
        let id = args.first().map(|v| to_number(v) as u32).unwrap_or(0);
        let _ = crate::bridge::ffi::call_ink_ffi("destroy_root", &format!("[{}]", id));
        Ok(Value::Undefined)
    });

    ctx.register_native("__ink_create_node", move |args| {
        let tag = args.first().map(|v| to_js_string(v)).unwrap_or_default();
        let props = args.get(1).map(|v| to_js_string(v)).unwrap_or_default();
        let result =
            crate::bridge::ffi::call_ink_ffi("create_node", &format!("[\"{}\",{}]", tag, props));
        Ok(Value::Number(result.parse::<f64>().unwrap_or(0.0)))
    });

    ctx.register_native("__ink_create_text_node", move |args| {
        let text = args.first().map(|v| to_js_string(v)).unwrap_or_default();
        let result =
            crate::bridge::ffi::call_ink_ffi("create_text_node", &format!("[\"{}\"]", text));
        Ok(Value::Number(result.parse::<f64>().unwrap_or(0.0)))
    });
}

// =============================================================================
// DOM manipulation functions
// =============================================================================

fn register_manipulation_functions(ctx: &mut Context) {
    ctx.register_native("__ink_append_child", move |args| {
        let parent = args.first().map(|v| to_js_string(v)).unwrap_or_default();
        let child = args.get(1).map(|v| to_js_string(v)).unwrap_or_default();
        let result =
            crate::bridge::ffi::call_ink_ffi("append_child", &format!("[{},{}]", parent, child));
        Ok(Value::Boolean(result == "true"))
    });

    ctx.register_native("__ink_remove_child", move |args| {
        let parent = args.first().map(|v| to_js_string(v)).unwrap_or_default();
        let child = args.get(1).map(|v| to_js_string(v)).unwrap_or_default();
        let result =
            crate::bridge::ffi::call_ink_ffi("remove_child", &format!("[{},{}]", parent, child));
        Ok(Value::Boolean(result == "true"))
    });

    ctx.register_native("__ink_insert_before", move |args| {
        let parent = args.first().map(|v| to_js_string(v)).unwrap_or_default();
        let child = args.get(1).map(|v| to_js_string(v)).unwrap_or_default();
        let before = args.get(2).map(|v| to_js_string(v)).unwrap_or_default();
        let result = crate::bridge::ffi::call_ink_ffi(
            "insert_before",
            &format!("[{},{},{}]", parent, child, before),
        );
        Ok(Value::Boolean(result == "true"))
    });

    ctx.register_native("__ink_commit_update", move |args| {
        let id = args.first().map(|v| to_js_string(v)).unwrap_or_default();
        let props = args.get(1).map(|v| to_js_string(v)).unwrap_or_default();
        let result = crate::bridge::ffi::call_ink_ffi(
            "commit_update",
            &format!("[\"{}\",{}]", id, props),
        );
        Ok(Value::Boolean(result == "true"))
    });

    ctx.register_native("__ink_set_text", move |args| {
        let id = args.first().map(|v| to_js_string(v)).unwrap_or_default();
        let text = args.get(1).map(|v| to_js_string(v)).unwrap_or_default();
        let result =
            crate::bridge::ffi::call_ink_ffi("set_text", &format!("[\"{}\",\"{}\"]", id, text));
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
}

// =============================================================================
// Measurement functions
// =============================================================================

fn register_measurement_functions(ctx: &mut Context) {
    ctx.register_native("__ink_measure_text", move |args| {
        let text = args.first().map(|v| to_js_string(v)).unwrap_or_default();
        let width = args.get(1).map(|v| to_number(v) as u32).unwrap_or(80);
        let result =
            crate::bridge::ffi::call_ink_ffi("measure_text", &format!("[\"{}\",{}]", text, width));
        let (w, h) = parse_xy(&result, 0.0);
        Ok(make_size_object(w, h))
    });

    ctx.register_native("__ink_measure_element", move |args| {
        let id = args.first().map(|v| to_js_string(v)).unwrap_or_default();
        let result = crate::bridge::ffi::call_ink_ffi("measure_element", &format!("[{}]", id));

        if result == "null" {
            return Ok(Value::Null);
        }

        let (w, h) = parse_xy(&result, 0.0);
        Ok(make_size_object(w, h))
    });
}

// =============================================================================
// Terminal control functions
// =============================================================================

fn register_terminal_functions(ctx: &mut Context) {
    ctx.register_native("__ink_exit", move |args| {
        let code = args.first().map(|v| to_number(v) as i32).unwrap_or(0);
        let _ = crate::bridge::ffi::call_ink_ffi("exit", &format!("[{}]", code));
        Ok(Value::Undefined)
    });

    ctx.register_native("__ink_should_exit", move |_args| {
        let result = crate::bridge::ffi::call_ink_ffi("should_exit", "[]");
        Ok(Value::Boolean(result == "true"))
    });

    ctx.register_native("__ink_get_exit_code", move |_args| {
        let result = crate::bridge::ffi::call_ink_ffi("get_exit_code", "[]");
        Ok(Value::Number(result.parse::<f64>().unwrap_or(0.0)))
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
        let width = args.first().map(|v| to_number(v) as u32).unwrap_or(80);
        let height = args.get(1).map(|v| to_number(v) as u32).unwrap_or(24);
        let _ = crate::bridge::ffi::call_ink_ffi(
            "set_terminal_size",
            &format!("[{},{}]", width, height),
        );
        Ok(Value::Undefined)
    });

    ctx.register_native("__ink_get_terminal_size", move |_args| {
        let result = crate::bridge::ffi::call_ink_ffi("get_terminal_size", "[]");
        let (w, h) = parse_xy(&result, 80.0);
        Ok(make_size_object(w, h))
    });
}

// =============================================================================
// Node introspection functions
// =============================================================================

fn register_introspection_functions(ctx: &mut Context) {
    ctx.register_native("__ink_get_node_tag", move |args| {
        let id = args.first().map(|v| to_number(v) as u32).unwrap_or(0);
        let result = crate::bridge::ffi::call_ink_ffi("get_node_tag", &format!("[{}]", id));
        if result == "null" {
            Ok(Value::Null)
        } else {
            Ok(Value::String(result.trim_matches('"').to_string()))
        }
    });

    ctx.register_native("__ink_get_node_text", move |args| {
        let id = args.first().map(|v| to_number(v) as u32).unwrap_or(0);
        let result = crate::bridge::ffi::call_ink_ffi("get_node_text", &format!("[{}]", id));
        if result == "null" {
            Ok(Value::Null)
        } else {
            Ok(Value::String(result.trim_matches('"').to_string()))
        }
    });

    ctx.register_native("__ink_get_node_children", move |args| {
        let id = args.first().map(|v| to_number(v) as u32).unwrap_or(0);
        let result = crate::bridge::ffi::call_ink_ffi("get_node_children", &format!("[{}]", id));

        if result == "null" {
            return Ok(Value::Null);
        }

        let nums: Vec<f64> = result
            .trim_matches(|c| c == '[' || c == ']')
            .split(',')
            .filter_map(|s| s.trim().parse::<f64>().ok())
            .collect();

        Ok(Value::Object(Rc::new(std::cell::RefCell::new(Object::new_array(
            nums.len(),
        )))))
    });

    ctx.register_native("__ink_get_node_prop", move |args| {
        let id = args.first().map(|v| to_number(v) as u32).unwrap_or(0);
        let prop = args.get(1).map(|v| to_js_string(v)).unwrap_or_default();
        let result =
            crate::bridge::ffi::call_ink_ffi("get_node_prop", &format!("[{},\"{}\"]", id, prop));
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
            Ok(Value::Number(result.parse::<f64>().unwrap_or(0.0)))
        }
    });

    ctx.register_native("__ink_calculate_layout", move |_args| {
        let result = crate::bridge::ffi::call_ink_ffi("calculate_layout", "[]");
        Ok(Value::Boolean(result == "true"))
    });

    ctx.register_native("__ink_get_layout", move |args| {
        let id = args.first().map(|v| to_js_string(v)).unwrap_or_default();
        let result = crate::bridge::ffi::call_ink_ffi("get_layout", &format!("[{}]", id));

        if result == "null" {
            return Ok(Value::Null);
        }

        let (x, y, w, h) = parse_layout(&result);
        Ok(make_layout_object(x, y, w, h))
    });
}

// =============================================================================
// Timer functions
// =============================================================================

fn register_timer_functions(ctx: &mut Context) {
    ctx.register_native("__ink_set_timeout", move |args| {
        let callback = args.first().map(|v| to_js_string(v)).unwrap_or_default();
        let delay = args.get(1).map(|v| to_number(v) as u64).unwrap_or(0);
        let result = crate::bridge::ffi::call_ink_ffi(
            "set_timeout",
            &format!("[\"{}\",{}]", callback, delay),
        );
        Ok(Value::Number(result.parse::<f64>().unwrap_or(0.0)))
    });

    ctx.register_native("__ink_set_interval", move |args| {
        let callback = args.first().map(|v| to_js_string(v)).unwrap_or_default();
        let interval = args.get(1).map(|v| to_number(v) as u64).unwrap_or(0);
        let result = crate::bridge::ffi::call_ink_ffi(
            "set_interval",
            &format!("[\"{}\",{}]", callback, interval),
        );
        Ok(Value::Number(result.parse::<f64>().unwrap_or(0.0)))
    });

    ctx.register_native("__ink_clear_timer", move |args| {
        let id = args.first().map(|v| to_number(v) as u32).unwrap_or(0);
        let _ = crate::bridge::ffi::call_ink_ffi("clear_timer", &format!("[{}]", id));
        Ok(Value::Undefined)
    });
}

// =============================================================================
// Event dispatch stubs
// =============================================================================

fn register_event_stubs(ctx: &mut Context) {
    ctx.register_native("__tb_dispatch_key", move |_args| Ok(Value::Undefined));
    ctx.register_native("__tb_dispatch_mouse", move |_args| Ok(Value::Undefined));
    ctx.register_native("__tb_dispatch_resize", move |_args| Ok(Value::Undefined));
    ctx.register_native("__tb_invoke_timers", move |_args| Ok(Value::Undefined));
}

// =============================================================================
// Ink component tags
// =============================================================================

fn register_ink_tags(ctx: &mut Context) {
    // Direct global registrations
    ctx.set_global(
        "Box".to_string(),
        Value::String("ink-box".to_string()),
    );
    ctx.set_global(
        "Text".to_string(),
        Value::String("ink-text".to_string()),
    );
    ctx.set_global(
        "Static".to_string(),
        Value::String("ink-static".to_string()),
    );
    ctx.set_global(
        "Newline".to_string(),
        Value::String("ink-newline".to_string()),
    );
    ctx.set_global(
        "Spacer".to_string(),
        Value::String("ink-spacer".to_string()),
    );

    // ink namespace object
    let ink_ns = Object::new(ObjectKind::Ordinary);
    let ink = Rc::new(std::cell::RefCell::new(ink_ns));
    ink.borrow_mut()
        .set("Box", Value::String("ink-box".to_string()));
    ink.borrow_mut()
        .set("Text", Value::String("ink-text".to_string()));
    ink.borrow_mut()
        .set("Static", Value::String("ink-static".to_string()));
    ink.borrow_mut()
        .set("Newline", Value::String("ink-newline".to_string()));
    ink.borrow_mut()
        .set("Spacer", Value::String("ink-spacer".to_string()));
    ctx.set_global("ink".to_string(), Value::Object(ink));
}
