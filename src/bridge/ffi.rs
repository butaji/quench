//! Bridge: FFI dispatch
//!
//! Routes __ink_call from JS to Rust handlers.
//!
//! Uses a static dispatch table for O(1) lookup by method name.
//! See also: `__ink_call_fast` for the f64-args hot path (task 105).

use crate::bridge::node::{__ink_create_node, __ink_create_root, __ink_create_text_node, __ink_render_element, __ink_destroy_root};
use crate::bridge::tree::{__ink_append_child, __ink_calculate_layout, __ink_clear_dirty, __ink_commit, __ink_commit_update, __ink_get_layout, __ink_get_node_children, __ink_get_node_parent, __ink_get_node_prop, __ink_get_node_tag, __ink_get_node_text, __ink_get_root_id, __ink_insert_before, __ink_is_dirty, __ink_measure_element, __ink_remove_child, __ink_set_text};
use crate::bridge::timers::{__ink_clear_timer, __ink_drain_microtasks, __ink_enqueue_microtask, __ink_has_pending_timers, __ink_next_timer_delay, __ink_process_timers, __ink_set_interval, __ink_set_timeout};
use crate::bridge::io::{__ink_exit as ink_exit, __ink_get_exit_code, __ink_get_terminal_size, __ink_reset_exit, __ink_set_exit_requested, __ink_set_terminal_size, __ink_should_exit, __ink_stdout_write, __ink_stderr_write, __ink_measure_text, __ink_stdin_is_raw};

use std::cell::RefCell;

thread_local! {
    /// Reusable buffer for hot-path FFI returns to avoid per-call allocations.
    static BUF: RefCell<String> = RefCell::new(String::with_capacity(64));
}

/// Clear the reusable buffer, run a closure that writes into it, then clone the result.
fn with_buf(f: impl FnOnce(&mut String)) -> String {
    BUF.with(|buf| {
        let mut b = buf.borrow_mut();
        b.clear();
        f(&mut b);
        b.clone()
    })
}

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

fn handle_is_dirty(_args: &[String]) -> String {
    with_buf(|b| b.push_str(if __ink_is_dirty() { "true" } else { "false" }))
}

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
    with_buf(|b| {
        let _ = std::fmt::Write::write_fmt(b, format_args!("{},{}", w, h));
    })
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
        Some(children) => with_buf(|b| {
            b.push('[');
            for (i, &c) in children.iter().enumerate() {
                if i > 0 {
                    b.push(',');
                }
                let _ = std::fmt::Write::write_fmt(b, format_args!("{}", c));
            }
            b.push(']');
        }),
        None => "null".to_string(),
    }
}

fn handle_get_node_parent(args: &[String]) -> String {
    let id = parse_u32_arg(args, 0);
    match __ink_get_node_parent(id) {
        Some(parent_id) => with_buf(|b| {
            let _ = std::fmt::Write::write_fmt(b, format_args!("{}", parent_id));
        }),
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
        Some(id) => with_buf(|b| {
            let _ = std::fmt::Write::write_fmt(b, format_args!("{}", id));
        }),
        None => "null".to_string(),
    }
}

fn handle_calculate_layout(_args: &[String]) -> String { (__ink_calculate_layout().is_ok()).to_string() }

fn handle_get_layout(args: &[String]) -> String {
    let id = parse_u32_arg(args, 0);
    match __ink_get_layout(id) {
        Some((x, y, w, h)) => with_buf(|b| {
            let _ = std::fmt::Write::write_fmt(b, format_args!("{},{},{},{}", x, y, w, h));
        }),
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

fn handle_has_pending_timers(_args: &[String]) -> String {
    with_buf(|b| b.push_str(if __ink_has_pending_timers() { "true" } else { "false" }))
}

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
// Fast dispatch by method ID (Task 105)
// ===================================================================

/// Hot-path FFI methods that support the fast f64-args path.
/// Method IDs are assigned at compile time for O(1) dispatch.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FastMethodId {
    // Tree hot path (called every frame)
    IsDirty = 1,
    ClearDirty = 2,
    Commit = 3,
    CalculateLayout = 4,
    GetLayout = 5,
    GetRootId = 6,
    // I/O hot path
    ShouldExit = 7,
    GetTerminalSize = 8,
    StdinIsRaw = 9,
}

/// Fast handler signature: takes 5 f64 args, returns f64.
type FastHandler = fn(f64, f64, f64, f64, f64) -> f64;

/// Fast dispatch table indexed by method ID.
static FAST_HANDLERS: once_cell::sync::Lazy<Vec<(FastMethodId, FastHandler)>> =
    once_cell::sync::Lazy::new(|| {
        vec![
            (FastMethodId::IsDirty, fast_is_dirty),
            (FastMethodId::ClearDirty, fast_clear_dirty),
            (FastMethodId::Commit, fast_commit),
            (FastMethodId::CalculateLayout, fast_calculate_layout),
            (FastMethodId::GetLayout, fast_get_layout),
            (FastMethodId::GetRootId, fast_get_root_id),
            (FastMethodId::ShouldExit, fast_should_exit),
            (FastMethodId::GetTerminalSize, fast_get_terminal_size),
            (FastMethodId::StdinIsRaw, fast_stdin_is_raw),
        ]
    });

// Fast handlers - minimal overhead for hot path
fn fast_is_dirty(_a: f64, _b: f64, _c: f64, _d: f64, _e: f64) -> f64 {
    if __ink_is_dirty() { 1.0 } else { 0.0 }
}

fn fast_clear_dirty(_a: f64, _b: f64, _c: f64, _d: f64, _e: f64) -> f64 {
    __ink_clear_dirty();
    0.0
}

fn fast_commit(_a: f64, _b: f64, _c: f64, _d: f64, _e: f64) -> f64 {
    __ink_commit();
    0.0
}

fn fast_calculate_layout(_a: f64, _b: f64, _c: f64, _d: f64, _e: f64) -> f64 {
    if __ink_calculate_layout().is_ok() { 1.0 } else { 0.0 }
}

fn fast_get_layout(a: f64, _b: f64, _c: f64, _d: f64, _e: f64) -> f64 {
    let id = a as u32;
    match __ink_get_layout(id) {
        Some((_x, _y, _w, _h)) => {
            // Pack into single f64 using IEEE 754 format
            // This is a simplified approach; full impl would need proper encoding
            // For now, return the layout as a side effect and return 0
            // JS will use __ink_call for layout queries
            0.0
        }
        None => 0.0,
    }
}

fn fast_get_root_id(_a: f64, _b: f64, _c: f64, _d: f64, _e: f64) -> f64 {
    __ink_get_root_id().unwrap_or(0) as f64
}

fn fast_should_exit(_a: f64, _b: f64, _c: f64, _d: f64, _e: f64) -> f64 {
    if __ink_should_exit() { 1.0 } else { 0.0 }
}

fn fast_get_terminal_size(_a: f64, _b: f64, _c: f64, _d: f64, _e: f64) -> f64 {
    // Return encoded terminal size
    let (w, h) = __ink_get_terminal_size();
    (w as f64) * 1000.0 + (h as f64)
}

fn fast_stdin_is_raw(_a: f64, _b: f64, _c: f64, _d: f64, _e: f64) -> f64 {
    if __ink_stdin_is_raw() { 1.0 } else { 0.0 }
}

/// Fast dispatch by method ID. Returns f64.
/// Falls back to 0.0 for unknown method IDs.
pub fn call_ink_ffi_fast(method_id: u32, a: f64, b: f64, c: f64, d: f64, e: f64) -> f64 {
    let method = match method_id {
        1 => FastMethodId::IsDirty,
        2 => FastMethodId::ClearDirty,
        3 => FastMethodId::Commit,
        4 => FastMethodId::CalculateLayout,
        5 => FastMethodId::GetLayout,
        6 => FastMethodId::GetRootId,
        7 => FastMethodId::ShouldExit,
        8 => FastMethodId::GetTerminalSize,
        9 => FastMethodId::StdinIsRaw,
        _ => return 0.0,
    };

    // Find the handler and call it
    for (id, handler) in FAST_HANDLERS.iter() {
        if *id == method {
            return handler(a, b, c, d, e);
        }
    }

    0.0
}

/// Get method ID by name. Returns None if method is not in the fast path.
pub fn get_fast_method_id(name: &str) -> Option<u32> {
    match name {
        "is_dirty" => Some(1),
        "clear_dirty" => Some(2),
        "commit" => Some(3),
        "calculate_layout" => Some(4),
        "get_layout" => Some(5),
        "get_root_id" => Some(6),
        "should_exit" => Some(7),
        "get_terminal_size" => Some(8),
        "stdin_is_raw" => Some(9),
        _ => None,
    }
}

// ===================================================================
// JSON dispatch routing table — O(1) lookup using FxHashMap
// ===================================================================

use std::collections::hash_map::DefaultHasher;
use core::hash::BuildHasherDefault;

type FxBuildHasher = BuildHasherDefault<DefaultHasher>;
type FxHashMap<K, V> = std::collections::HashMap<K, V, FxBuildHasher>;

type HandlerFn = fn(&[String]) -> String;

/// Static handler table — built once at compile time.
fn get_handlers() -> FxHashMap<&'static str, HandlerFn> {
    let mut map = FxHashMap::with_hasher(BuildHasherDefault::default());
    // Node
    map.insert("create_root", handle_create_root as HandlerFn);
    map.insert("render_element", handle_render_element as HandlerFn);
    map.insert("destroy_root", handle_destroy_root as HandlerFn);
    map.insert("create_node", handle_create_node as HandlerFn);
    map.insert("create_text_node", handle_create_text_node as HandlerFn);
    // Tree
    map.insert("append_child", handle_append_child as HandlerFn);
    map.insert("remove_child", handle_remove_child as HandlerFn);
    map.insert("insert_before", handle_insert_before as HandlerFn);
    map.insert("commit_update", handle_commit_update as HandlerFn);
    map.insert("set_text", handle_set_text as HandlerFn);
    map.insert("commit", handle_commit as HandlerFn);
    map.insert("is_dirty", handle_is_dirty as HandlerFn);
    map.insert("clear_dirty", handle_clear_dirty as HandlerFn);
    map.insert("calculate_layout", handle_calculate_layout as HandlerFn);
    map.insert("get_layout", handle_get_layout as HandlerFn);
    // Node queries
    map.insert("get_node_tag", handle_get_node_tag as HandlerFn);
    map.insert("get_node_text", handle_get_node_text as HandlerFn);
    map.insert("get_node_children", handle_get_node_children as HandlerFn);
    map.insert("get_node_parent", handle_get_node_parent as HandlerFn);
    map.insert("get_node_prop", handle_get_node_prop as HandlerFn);
    map.insert("get_root_id", handle_get_root_id as HandlerFn);
    // I/O
    map.insert("measure_text", handle_measure_text as HandlerFn);
    map.insert("measure_element", handle_measure_element as HandlerFn);
    map.insert("exit", handle_exit as HandlerFn);
    map.insert("should_exit", handle_should_exit as HandlerFn);
    map.insert("get_exit_code", handle_get_exit_code as HandlerFn);
    map.insert("reset_exit", handle_reset_exit as HandlerFn);
    map.insert("set_exit_requested", handle_set_exit_requested as HandlerFn);
    map.insert("set_terminal_size", handle_set_terminal_size as HandlerFn);
    map.insert("get_terminal_size", handle_get_terminal_size as HandlerFn);
    map.insert("stdout_write", handle_stdout_write as HandlerFn);
    map.insert("stderr_write", handle_stderr_write as HandlerFn);
    map.insert("stdin_is_raw", handle_stdin_is_raw as HandlerFn);
    map.insert("set_raw_mode", handle_set_raw_mode as HandlerFn);
    // Timers
    map.insert("set_timeout", handle_set_timeout as HandlerFn);
    map.insert("set_interval", handle_set_interval as HandlerFn);
    map.insert("clear_timer", handle_clear_timer as HandlerFn);
    map.insert("process_timers", handle_process_timers as HandlerFn);
    map.insert("has_pending_timers", handle_has_pending_timers as HandlerFn);
    map.insert("next_timer_delay", handle_next_timer_delay as HandlerFn);
    map.insert("enqueue_microtask", handle_enqueue_microtask as HandlerFn);
    map.insert("drain_microtasks", handle_drain_microtasks as HandlerFn);
    map
}

/// Cached handler table — populated once on first use.
static HANDLERS: once_cell::sync::Lazy<FxHashMap<&'static str, HandlerFn>> =
    once_cell::sync::Lazy::new(get_handlers);

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

/// Inner dispatch — O(1) lookup through the handler HashMap.
fn call_ink_ffi_inner(method: &str, args_json: &str) -> String {
    let args = parse_args(args_json).unwrap_or_else(|_| {
        tracing::warn!("Malformed JSON args in __ink_call('{}', ...)", method);
        Vec::new()
    });
    let args_slice: &[String] = &args;

    if let Some(handler) = HANDLERS.get(method) {
        return handler(args_slice);
    }

    String::new()
}
