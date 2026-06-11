//! Bridge: FFI dispatch
//!
//! Routes __ink_call from JS to Rust handlers.

use crate::bridge::node::{__ink_create_node, __ink_create_root, __ink_create_text_node, __ink_render_element, __ink_destroy_root};
use crate::bridge::tree::{__ink_append_child, __ink_calculate_layout, __ink_clear_dirty, __ink_commit, __ink_commit_update, __ink_get_layout, __ink_get_node_children, __ink_get_node_parent, __ink_get_node_prop, __ink_get_node_tag, __ink_get_node_text, __ink_get_root_id, __ink_insert_before, __ink_is_dirty, __ink_measure_element, __ink_remove_child, __ink_set_text};
use crate::bridge::timers::{__ink_clear_timer, __ink_drain_microtasks, __ink_enqueue_microtask, __ink_has_pending_timers, __ink_next_timer_delay, __ink_process_timers, __ink_set_interval, __ink_set_timeout};
use crate::bridge::io::{__ink_exit as ink_exit, __ink_get_exit_code, __ink_get_terminal_size, __ink_reset_exit, __ink_set_exit_requested, __ink_set_terminal_size, __ink_should_exit, __ink_stdout_write, __ink_stderr_write, __ink_measure_text, __ink_stdin_is_raw};



/// Parse JSON args into String vector.
/// Returns Err on malformed JSON so callers can log and handle it.
fn parse_args(args_json: &str) -> std::result::Result<Vec<String>, serde_json::Error> {
    let json_vals: Vec<serde_json::Value> = serde_json::from_str(args_json)?;
    Ok(json_vals.iter().map(json_val_to_string).collect())
}

/// Convert JSON value to string representation
fn json_val_to_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        _ => v.to_string(),
    }
}

/// Parse first arg as f64, returning default if missing or invalid
fn parse_f64_arg(args: &[String], index: usize) -> f64 {
    args.get(index).and_then(|s| s.parse().ok()).unwrap_or(0.0)
}

/// Parse first arg as u32
fn parse_u32_arg(args: &[String], index: usize) -> u32 {
    parse_f64_arg(args, index) as u32
}

/// Parse first arg as i32
fn parse_i32_arg(args: &[String], index: usize) -> i32 {
    parse_f64_arg(args, index) as i32
}

/// Parse first arg as u64
fn parse_u64_arg(args: &[String], index: usize) -> u64 {
    args.get(index).and_then(|s| s.parse().ok()).unwrap_or(0)
}

// ===================================================================
// Node handlers
// ===================================================================

fn handle_create_root(_args: &[String]) -> String { (__ink_create_root() as f64).to_string() }

fn handle_render_element(args: &[String]) -> String {
    let json = args.first().cloned().unwrap_or_default();
    (__ink_render_element(&json) as f64).to_string()
}

fn handle_destroy_root(args: &[String]) -> String {
    let id = parse_u32_arg(args, 0);
    __ink_destroy_root(id);
    String::new()
}

fn handle_create_node(args: &[String]) -> String {
    let tag = args.first().cloned().unwrap_or_default();
    let props = args.get(1).cloned().unwrap_or_default();
    (__ink_create_node(&tag, &props).unwrap_or(0) as f64).to_string()
}

fn handle_create_text_node(args: &[String]) -> String {
    let text = args.first().cloned().unwrap_or_default();
    (__ink_create_text_node(&text) as f64).to_string()
}

// ===================================================================
// Tree handlers
// ===================================================================

fn handle_append_child(args: &[String]) -> String {
    let p = parse_u32_arg(args, 0);
    let c = parse_u32_arg(args, 1);
    (__ink_append_child(p, c).is_ok()).to_string()
}

fn handle_remove_child(args: &[String]) -> String {
    let p = parse_u32_arg(args, 0);
    let c = parse_u32_arg(args, 1);
    (__ink_remove_child(p, c).is_ok()).to_string()
}

fn handle_insert_before(args: &[String]) -> String {
    let p = parse_u32_arg(args, 0);
    let c = parse_u32_arg(args, 1);
    let b = parse_u32_arg(args, 2);
    (__ink_insert_before(p, c, b).is_ok()).to_string()
}

fn handle_commit_update(args: &[String]) -> String {
    let id = parse_u32_arg(args, 0);
    let props = args.get(1).cloned().unwrap_or_default();
    (__ink_commit_update(id, &props).is_ok()).to_string()
}

fn handle_set_text(args: &[String]) -> String {
    let id = parse_u32_arg(args, 0);
    let text = args.get(1).cloned().unwrap_or_default();
    (__ink_set_text(id, &text).is_ok()).to_string()
}

fn handle_commit(_args: &[String]) -> String {
    __ink_commit();
    String::new()
}

fn handle_is_dirty(_args: &[String]) -> String { __ink_is_dirty().to_string() }

fn handle_clear_dirty(_args: &[String]) -> String {
    __ink_clear_dirty();
    String::new()
}

// ===================================================================
// Measure handlers
// ===================================================================

fn handle_measure_text(args: &[String]) -> String {
    let text = args.first().cloned().unwrap_or_default();
    let width = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(80);
    let (w, h) = __ink_measure_text(&text, width);
    format!("{},{}", w, h)
}

fn handle_measure_element(args: &[String]) -> String {
    let id = parse_u32_arg(args, 0);
    match __ink_measure_element(id) {
        Some((w, h)) => format!("{},{}", w, h),
        None => "null".to_string(),
    }
}

// ===================================================================
// I/O handlers
// ===================================================================

fn handle_exit(args: &[String]) -> String {
    let code = parse_i32_arg(args, 0);
    ink_exit(code);
    String::new()
}

fn handle_should_exit(_args: &[String]) -> String { __ink_should_exit().to_string() }

fn handle_get_exit_code(_args: &[String]) -> String { (__ink_get_exit_code() as f64).to_string() }

fn handle_reset_exit(_args: &[String]) -> String {
    __ink_reset_exit();
    String::new()
}

fn handle_set_exit_requested(_args: &[String]) -> String {
    __ink_set_exit_requested();
    String::new()
}

fn handle_set_terminal_size(args: &[String]) -> String {
    let w = args.first().and_then(|s| s.parse().ok()).unwrap_or(80);
    let h = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(24);
    __ink_set_terminal_size(w, h);
    String::new()
}

fn handle_get_terminal_size(_args: &[String]) -> String {
    let (w, h) = __ink_get_terminal_size();
    format!("{},{}", w, h)
}

fn handle_get_node_tag(args: &[String]) -> String {
    let id = parse_u32_arg(args, 0);
    __ink_get_node_tag(id).unwrap_or_else(|| "null".to_string())
}

fn handle_get_node_text(args: &[String]) -> String {
    let id = parse_u32_arg(args, 0);
    __ink_get_node_text(id).unwrap_or_else(|| "null".to_string())
}

fn handle_get_node_children(args: &[String]) -> String {
    let id = parse_u32_arg(args, 0);
    match __ink_get_node_children(id) {
        Some(children) => {
            let s: Vec<String> = children.iter().map(|&c| c.to_string()).collect();
            format!("[{}]", s.join(","))
        }
        None => "null".to_string(),
    }
}

fn handle_get_node_parent(args: &[String]) -> String {
    let id = parse_u32_arg(args, 0);
    match __ink_get_node_parent(id) {
        Some(parent_id) => parent_id.to_string(),
        None => "null".to_string(),
    }
}

fn handle_get_node_prop(args: &[String]) -> String {
    let id = parse_u32_arg(args, 0);
    let prop = args.get(1).cloned().unwrap_or_default();
    __ink_get_node_prop(id, &prop).unwrap_or_else(|| "null".to_string())
}

fn handle_get_root_id(_args: &[String]) -> String {
    match __ink_get_root_id() {
        Some(id) => id.to_string(),
        None => "null".to_string(),
    }
}

fn handle_calculate_layout(_args: &[String]) -> String { (__ink_calculate_layout().is_ok()).to_string() }

fn handle_get_layout(args: &[String]) -> String {
    let id = parse_u32_arg(args, 0);
    match __ink_get_layout(id) {
        Some((x, y, w, h)) => format!("{},{},{},{}", x, y, w, h),
        None => "null".to_string(),
    }
}

fn handle_stdout_write(args: &[String]) -> String {
    let data = args.first().cloned().unwrap_or_default();
    __ink_stdout_write(&data);
    String::new()
}

fn handle_stderr_write(args: &[String]) -> String {
    let data = args.first().cloned().unwrap_or_default();
    __ink_stderr_write(&data);
    String::new()
}

fn handle_stdin_is_raw(_args: &[String]) -> String { __ink_stdin_is_raw().to_string() }

fn handle_set_raw_mode(args: &[String]) -> String {
    let enabled = args.first().cloned().unwrap_or_default() == "true";
    if enabled {
        let _ = crossterm::terminal::enable_raw_mode();
    } else {
        let _ = crossterm::terminal::disable_raw_mode();
    }
    String::new()
}

// ===================================================================
// Timer handlers
// ===================================================================

fn handle_set_timeout(args: &[String]) -> String {
    let callback = args.first().cloned().unwrap_or_default();
    let delay = parse_u64_arg(args, 1);
    (__ink_set_timeout(&callback, delay) as f64).to_string()
}

fn handle_set_interval(args: &[String]) -> String {
    let callback = args.first().cloned().unwrap_or_default();
    let interval = parse_u64_arg(args, 1);
    (__ink_set_interval(&callback, interval) as f64).to_string()
}

fn handle_clear_timer(args: &[String]) -> String {
    let id = parse_u32_arg(args, 0);
    (__ink_clear_timer(id)).to_string()
}

fn handle_process_timers(_args: &[String]) -> String { __ink_process_timers() }

fn handle_has_pending_timers(_args: &[String]) -> String { __ink_has_pending_timers().to_string() }

fn handle_next_timer_delay(_args: &[String]) -> String {
    match __ink_next_timer_delay() {
        Some(d) => d.as_millis().to_string(),
        None => "-1".to_string(),
    }
}

fn handle_enqueue_microtask(args: &[String]) -> String {
    let callback = args.first().cloned().unwrap_or_default();
    __ink_enqueue_microtask(&callback);
    String::new()
}

fn handle_drain_microtasks(_args: &[String]) -> String { __ink_drain_microtasks().to_string() }

// ===================================================================
// Dispatch routing tables
// ===================================================================

#[allow(clippy::type_complexity)]
const NODE_METHODS: &[(&str, fn(&[String]) -> String)] = &[
    ("create_root", handle_create_root),
    ("render_element", handle_render_element),
    ("destroy_root", handle_destroy_root),
    ("create_node", handle_create_node),
    ("create_text_node", handle_create_text_node),
];

#[allow(clippy::type_complexity)]
const TREE_METHODS: &[(&str, fn(&[String]) -> String)] = &[
    ("append_child", handle_append_child),
    ("remove_child", handle_remove_child),
    ("insert_before", handle_insert_before),
    ("commit_update", handle_commit_update),
    ("set_text", handle_set_text),
    ("commit", handle_commit),
    ("is_dirty", handle_is_dirty),
    ("clear_dirty", handle_clear_dirty),
    ("calculate_layout", handle_calculate_layout),
    ("get_layout", handle_get_layout),
];

#[allow(clippy::type_complexity)]
const NODE_QUERY_METHODS: &[(&str, fn(&[String]) -> String)] = &[
    ("get_node_tag", handle_get_node_tag),
    ("get_node_text", handle_get_node_text),
    ("get_node_children", handle_get_node_children),
    ("get_node_parent", handle_get_node_parent),
    ("get_node_prop", handle_get_node_prop),
    ("get_root_id", handle_get_root_id),
];

#[allow(clippy::type_complexity)]
const IO_METHODS: &[(&str, fn(&[String]) -> String)] = &[
    ("measure_text", handle_measure_text),
    ("measure_element", handle_measure_element),
    ("exit", handle_exit),
    ("should_exit", handle_should_exit),
    ("get_exit_code", handle_get_exit_code),
    ("reset_exit", handle_reset_exit),
    ("set_exit_requested", handle_set_exit_requested),
    ("set_terminal_size", handle_set_terminal_size),
    ("get_terminal_size", handle_get_terminal_size),
    ("stdout_write", handle_stdout_write),
    ("stderr_write", handle_stderr_write),
    ("stdin_is_raw", handle_stdin_is_raw),
    ("set_raw_mode", handle_set_raw_mode),
];

#[allow(clippy::type_complexity)]
const TIMER_METHODS: &[(&str, fn(&[String]) -> String)] = &[
    ("set_timeout", handle_set_timeout),
    ("set_interval", handle_set_interval),
    ("clear_timer", handle_clear_timer),
    ("process_timers", handle_process_timers),
    ("has_pending_timers", handle_has_pending_timers),
    ("next_timer_delay", handle_next_timer_delay),
    ("enqueue_microtask", handle_enqueue_microtask),
    ("drain_microtasks", handle_drain_microtasks),
];

/// Lookup handler from method table
#[allow(clippy::type_complexity)]
fn lookup_handler<'a>(method: &str, table: &[(&str, fn(&'a [String]) -> String)]) -> Option<fn(&'a [String]) -> String> {
    table.iter().find(|(name, _)| *name == method).map(|(_, handler)| *handler)
}

/// Main dispatch function — catches panics so they don't propagate into QuickJS C code.
/// Panics in any FFI handler would otherwise cause UB by unwinding through
/// foreign (C) stack frames.
pub fn call_ink_ffi(method: &str, args_json: &str) -> String {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        call_ink_ffi_inner(method, args_json)
    })) {
        Ok(result) => result,
        Err(_) => {
            tracing::error!("Panic in FFI handler '{}'", method);
            // Return a value that JS can detect as an error
            "__INK_PANIC__".to_string()
        }
    }
}

/// Inner dispatch — never called directly; always wrapped in catch_unwind above.
fn call_ink_ffi_inner(method: &str, args_json: &str) -> String {
    let args = parse_args(args_json).unwrap_or_else(|_| {
        tracing::warn!("Malformed JSON args in __ink_call('{}', ...)", method);
        Vec::new()
    });
    let args_slice: &[String] = &args;

    // Try each dispatch table in order
    if let Some(handler) = lookup_handler(method, NODE_METHODS) {
        return handler(args_slice);
    }
    if let Some(handler) = lookup_handler(method, TREE_METHODS) {
        return handler(args_slice);
    }
    if let Some(handler) = lookup_handler(method, NODE_QUERY_METHODS) {
        return handler(args_slice);
    }
    if let Some(handler) = lookup_handler(method, IO_METHODS) {
        return handler(args_slice);
    }
    if let Some(handler) = lookup_handler(method, TIMER_METHODS) {
        return handler(args_slice);
    }

    String::new()
}
