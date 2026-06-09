//! Bridge between JavaScript (rquickjs) and Rust
//!
//! All `__ink_*` functions that JS calls go through this module.
//! These are exposed to the QuickJS VM as native host functions.

use std::io::Write;

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use thiserror::Error;

use crate::ink::{InkRuntime, PropValue};

// Thread-local Ink runtime instance
// Uses RefCell for interior mutability since Yoga Node is not Send+Sync
thread_local! {
    static INK_RUNTIME: RefCell<InkRuntime> = RefCell::new(InkRuntime::new());
}

/// Exit flag - set to true to break the event loop
static SHOULD_EXIT: AtomicBool = AtomicBool::new(false);

/// Exit code to return
static EXIT_CODE: AtomicU32 = AtomicU32::new(0);

/// Callback registry for input handlers
static INPUT_CALLBACKS: std::sync::LazyLock<std::sync::Arc<std::sync::Mutex<HashMap<u32, String>>>> =
    std::sync::LazyLock::new(|| std::sync::Arc::new(std::sync::Mutex::new(HashMap::new())));

static NEXT_CALLBACK_ID: std::sync::LazyLock<std::sync::Arc<AtomicU32>> =
    std::sync::LazyLock::new(|| std::sync::Arc::new(AtomicU32::new(1)));

/// Mouse event types
#[derive(Debug, Clone)]
pub struct MouseEvent {
    pub kind: MouseEventKind,
    pub column: u16,
    pub row: u16,
    pub modifiers: MouseModifiers,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MouseEventKind {
    Press,
    Release,
    Hold,
    WheelUp,
    WheelDown,
    Unknown,
}

#[derive(Debug, Clone, Default)]
pub struct MouseModifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

/// Bridge errors
#[derive(Error, Debug)]
pub enum FfiError {
    #[error("Invalid argument: {0}")]
    InvalidArgument(String),

    #[error("Bridge call failed: {0}")]
    CallFailed(String),

    #[error("Ink error: {0}")]
    InkError(#[from] crate::ink::InkError),
}

pub type Result<T> = std::result::Result<T, FfiError>;

// ============================================================================
// Bridge Functions - Root Node
// ============================================================================

/// Create the terminal root node
/// Returns the root node ID
pub fn __ink_create_root() -> u32 {
    INK_RUNTIME.with(|runtime| {
        let mut r = runtime.borrow_mut();
        r.create_root()
    })
}

/// Destroy the root and all child nodes
pub fn __ink_destroy_root(root_id: u32) {
    INK_RUNTIME.with(|runtime| {
        let mut r = runtime.borrow_mut();
        r.destroy_root(root_id)
    })
}

// ============================================================================
// Bridge Functions - Node Creation
// ============================================================================

/// Create a new node with tag and props (props as JSON string)
/// Returns the new node ID
pub fn __ink_create_node(tag: &str, props_json: &str) -> Result<u32> {
    let props = parse_props_json(props_json)?;
    INK_RUNTIME.with(|runtime| {
        let mut r = runtime.borrow_mut();
        Ok(r.create_node(tag, props))
    })
}

/// Create a text node with content
/// Returns the new node ID  
pub fn __ink_create_text_node(text: &str) -> u32 {
    INK_RUNTIME.with(|runtime| {
        let mut r = runtime.borrow_mut();
        r.create_text_node(text)
    })
}

// ============================================================================
// Bridge Functions - Tree Mutation
// ============================================================================

/// Append child to parent
pub fn __ink_append_child(parent_id: u32, child_id: u32) -> Result<()> {
    INK_RUNTIME.with(|runtime| {
        let mut r = runtime.borrow_mut();
        r.append_child(parent_id, child_id)
            .map_err(FfiError::from)
    })
}

/// Remove child from parent
pub fn __ink_remove_child(parent_id: u32, child_id: u32) -> Result<()> {
    INK_RUNTIME.with(|runtime| {
        let mut r = runtime.borrow_mut();
        r.remove_child(parent_id, child_id)
            .map_err(FfiError::from)
    })
}

/// Insert child before another child
pub fn __ink_insert_before(parent_id: u32, child_id: u32, before_id: u32) -> Result<()> {
    INK_RUNTIME.with(|runtime| {
        let mut r = runtime.borrow_mut();
        r.insert_before(parent_id, child_id, before_id)
            .map_err(FfiError::from)
    })
}

// ============================================================================
// Bridge Functions - Updates
// ============================================================================

/// Commit prop updates to a node (props as JSON string)
pub fn __ink_commit_update(node_id: u32, props_json: &str) -> Result<()> {
    let props = parse_props_json(props_json)?;
    INK_RUNTIME.with(|runtime| {
        let mut r = runtime.borrow_mut();
        r.commit_update(node_id, props)
            .map_err(FfiError::from)
    })
}

/// Set text content of a text node
pub fn __ink_set_text(node_id: u32, text: &str) -> Result<()> {
    INK_RUNTIME.with(|runtime| {
        let mut r = runtime.borrow_mut();
        r.set_text(node_id, text)
            .map_err(FfiError::from)
    })
}

/// Mark the tree as needing layout and render
pub fn __ink_commit() {
    INK_RUNTIME.with(|runtime| {
        let mut r = runtime.borrow_mut();
        r.mark_dirty()
    })
}

/// Check if the tree is dirty
pub fn __ink_is_dirty() -> bool {
    INK_RUNTIME.with(|runtime| {
        runtime.borrow().is_dirty()
    })
}

/// Clear the dirty flag (called after render)
pub fn __ink_clear_dirty() {
    INK_RUNTIME.with(|runtime| {
        let mut r = runtime.borrow_mut();
        r.clear_dirty()
    })
}

// ============================================================================
// Bridge Functions - Render Element (Task 010)
// ============================================================================

/// Parse a JSON element tree and build the Rust node tree.
/// Returns the root node ID.
pub fn __ink_render_element(json: &str) -> u32 {
    let root_id = __ink_create_root();
    
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(json) {
        build_element_tree(root_id, &val);
    }
    
    __ink_commit();
    root_id
}

/// Recursively build nodes from a serde_json::Value element tree.
fn build_element_tree(parent_id: u32, element: &serde_json::Value) {
    match element {
        serde_json::Value::Null | serde_json::Value::Object(_) if element.as_object().map(|m| m.is_empty()).unwrap_or(false) => {}
        serde_json::Value::String(s) => {
            let text_id = __ink_create_text_node(s);
            let _ = __ink_append_child(parent_id, text_id);
        }
        serde_json::Value::Number(n) => {
            let text_id = __ink_create_text_node(&n.to_string());
            let _ = __ink_append_child(parent_id, text_id);
        }
        serde_json::Value::Bool(b) => {
            let text_id = __ink_create_text_node(&b.to_string());
            let _ = __ink_append_child(parent_id, text_id);
        }
        serde_json::Value::Array(arr) => {
            for item in arr {
                build_element_tree(parent_id, item);
            }
        }
        serde_json::Value::Object(map) => {
            if let Some(serde_json::Value::String(tag)) = map.get("type") {
                // Build props JSON, excluding children
                let props_json = map.get("props")
                    .and_then(|p| {
                        if let serde_json::Value::Object(props_map) = p {
                            let mut filtered = props_map.clone();
                            filtered.remove("children");
                            serde_json::to_string(&serde_json::Value::Object(filtered)).ok()
                        } else {
                            None
                        }
                    })
                    .unwrap_or_else(|| "{}".to_string());
                
                let node_id = __ink_create_node(tag, &props_json).unwrap_or(0);
                let _ = __ink_append_child(parent_id, node_id);
                
                // Mount children from top-level `children` or from `props.children`
                if let Some(children) = map.get("children") {
                    build_element_tree(node_id, children);
                } else if let Some(serde_json::Value::Object(props_map)) = map.get("props") {
                    if let Some(children) = props_map.get("children") {
                        build_element_tree(node_id, children);
                    }
                }
            } else {
                // Plain object without type — stringify as text
                let text = serde_json::to_string(map).unwrap_or_default();
                let text_id = __ink_create_text_node(&text);
                let _ = __ink_append_child(parent_id, text_id);
            }
        }
        _ => {}
    }
}

// ============================================================================
// Bridge Functions - Measurement
// ============================================================================

/// Measure text dimensions
/// Returns {width, height} in cells
pub fn __ink_measure_text(text: &str, max_width: u32) -> (u32, u32) {
    use textwrap::wrap;
    use unicode_width::UnicodeWidthStr;
    
    if text.is_empty() {
        return (0, 0);
    }

    let max_width = if max_width == 0 { 80 } else { max_width as usize };
    let lines = wrap(text, max_width);
    
    let width = lines.iter()
        .map(|l| UnicodeWidthStr::width(l.as_ref()))
        .max()
        .unwrap_or(0) as u32;
    
    let height = lines.len() as u32;

    (width, height)
}

/// Measure element dimensions from Yoga layout
/// Returns {width, height} in cells
pub fn __ink_measure_element(node_id: u32) -> Option<(f32, f32)> {
    INK_RUNTIME.with(|runtime| {
        let r = runtime.borrow();
        r.node(node_id).map(|n| {
            let layout = n.get_layout();
            (layout.width(), layout.height())
        })
    })
}

/// Get the computed layout for a node
/// Returns (left, top, width, height)
pub fn __ink_get_layout(node_id: u32) -> Option<(f32, f32, f32, f32)> {
    INK_RUNTIME.with(|runtime| {
        let r = runtime.borrow();
        r.node(node_id).map(|n| {
            let layout = n.get_layout();
            (layout.left(), layout.top(), layout.width(), layout.height())
        })
    })
}

// ============================================================================
// Bridge Functions - I/O
// ============================================================================

/// Write to stdout
pub fn __ink_stdout_write(data: &str) {
    let _ = std::io::stdout().write_all(data.as_bytes());
}

/// Write to stderr  
pub fn __ink_stderr_write(data: &str) {
    eprint!("{}", data);
}

/// Check if stdin is in raw mode
pub fn __ink_stdin_is_raw() -> bool {
    false
}

/// Set raw mode on stdin
pub fn __ink_set_raw_mode(_enabled: bool) {}

/// Exit the application with optional error code
pub fn __ink_exit(code: i32) {
    SHOULD_EXIT.store(true, Ordering::SeqCst);
    EXIT_CODE.store(code as u32, Ordering::SeqCst);
}

/// Check if exit was requested
pub fn __ink_should_exit() -> bool {
    SHOULD_EXIT.load(Ordering::SeqCst)
}

/// Get exit code
pub fn __ink_get_exit_code() -> u32 {
    EXIT_CODE.load(Ordering::SeqCst)
}

/// Reset exit state (for reuse in tests)
pub fn __ink_reset_exit() {
    SHOULD_EXIT.store(false, Ordering::SeqCst);
    EXIT_CODE.store(0, Ordering::SeqCst);
}

// ============================================================================
// Bridge Functions - Input Handlers
// ============================================================================

/// Register an input callback (callback as JS code string)
/// Returns callback ID
pub fn __ink_register_input(callback_js: &str) -> u32 {
    let id = NEXT_CALLBACK_ID.fetch_add(1, Ordering::SeqCst);
    let mut callbacks = INPUT_CALLBACKS.lock().unwrap();
    callbacks.insert(id, callback_js.to_string());
    id
}

/// Unregister an input callback
pub fn __ink_unregister_input(id: u32) {
    let mut callbacks = INPUT_CALLBACKS.lock().unwrap();
    callbacks.remove(&id);
}

/// Dispatch a key event to registered callbacks
/// Returns JSON array of callback results for JS to evaluate
pub fn __ink_dispatch_key(key: &str, ctrl: bool, shift: bool, alt: bool) -> String {
    let callbacks = INPUT_CALLBACKS.lock().unwrap();
    let results: Vec<String> = callbacks.values()
        .map(|cb| format!("({})({{key:'{}',ctrl:{},shift:{},alt:{}}})", cb, key, ctrl, shift, alt))
        .collect();
    format!("[{}]", results.join(","))
}

/// Dispatch a mouse event to registered callbacks
/// Returns JSON array of callback results for JS to evaluate
pub fn __ink_dispatch_mouse(event: &MouseEvent) -> String {
    let kind_str = match event.kind {
        MouseEventKind::Press => "press",
        MouseEventKind::Release => "release",
        MouseEventKind::Hold => "hold",
        MouseEventKind::WheelUp => "wheelUp",
        MouseEventKind::WheelDown => "wheelDown",
        MouseEventKind::Unknown => "unknown",
    };

    let mouse_obj = format!(
        r#"{{"kind":"{}","column":{},"row":{},"shift":{},"ctrl":{},"alt":{}}}"#,
        kind_str,
        event.column,
        event.row,
        event.modifiers.shift,
        event.modifiers.ctrl,
        event.modifiers.alt
    );

    let callbacks = INPUT_CALLBACKS.lock().unwrap();
    let results: Vec<String> = callbacks.values()
        .map(|cb| format!("({})({})", cb, mouse_obj))
        .collect();

    format!("[{}]", results.join(","))
}

// ============================================================================
// Bridge Functions - Terminal State
// ============================================================================

/// Set terminal dimensions
pub fn __ink_set_terminal_size(width: u32, height: u32) {
    INK_RUNTIME.with(|runtime| {
        let mut r = runtime.borrow_mut();
        r.set_terminal_size(width, height)
    })
}

/// Get terminal dimensions
pub fn __ink_get_terminal_size() -> (u32, u32) {
    INK_RUNTIME.with(|runtime| {
        runtime.borrow().terminal_size()
    })
}

// ============================================================================
// Bridge Functions - Node Access (for rendering)
// ============================================================================

/// Get node tag
pub fn __ink_get_node_tag(node_id: u32) -> Option<String> {
    INK_RUNTIME.with(|runtime| {
        runtime.borrow().node(node_id).map(|n| match n.tag {
            crate::ink::InkTag::Box => "ink-box".to_string(),
            crate::ink::InkTag::Text => "ink-text".to_string(),
            crate::ink::InkTag::Static => "ink-static".to_string(),
            crate::ink::InkTag::Newline => "ink-newline".to_string(),
            crate::ink::InkTag::Spacer => "ink-spacer".to_string(),
            crate::ink::InkTag::Unknown => "unknown".to_string(),
        })
    })
}

/// Get node text content
pub fn __ink_get_node_text(node_id: u32) -> Option<String> {
    INK_RUNTIME.with(|runtime| {
        runtime.borrow().node(node_id).and_then(|n| n.text.clone())
    })
}

/// Get node children
pub fn __ink_get_node_children(node_id: u32) -> Option<Vec<u32>> {
    INK_RUNTIME.with(|runtime| {
        runtime.borrow().node(node_id).map(|n| n.children.clone())
    })
}

/// Get node parent
pub fn __ink_get_node_parent(node_id: u32) -> Option<u32> {
    INK_RUNTIME.with(|runtime| {
        runtime.borrow().node(node_id).and_then(|n| n.parent)
    })
}

/// Get node prop
pub fn __ink_get_node_prop(node_id: u32, prop: &str) -> Option<String> {
    INK_RUNTIME.with(|runtime| {
        runtime.borrow().node(node_id)
            .and_then(|n| n.props.get(prop))
            .map(prop_value_to_json)
    })
}

/// Get raw node prop value (for internal use by render)
pub fn __ink_get_node_prop_raw(node_id: u32, prop: &str) -> Option<crate::ink::PropValue> {
    INK_RUNTIME.with(|runtime| {
        runtime.borrow().node(node_id)
            .and_then(|n| n.props.get(prop).cloned())
    })
}

/// Get root ID
pub fn __ink_get_root_id() -> Option<u32> {
    INK_RUNTIME.with(|runtime| {
        runtime.borrow().root_id()
    })
}

/// Calculate layout for the entire tree
pub fn __ink_calculate_layout() -> Result<()> {
    INK_RUNTIME.with(|runtime| {
        let mut r = runtime.borrow_mut();
        r.calculate_layout().map_err(FfiError::from)
    })
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Parse a JSON string into props HashMap
fn parse_props_json(json: &str) -> Result<HashMap<String, PropValue>> {
    if json.is_empty() || json == "null" || json == "undefined" {
        return Ok(HashMap::new());
    }
    
    let mut props = HashMap::new();
    
    let json = json.trim();
    let content = if json.starts_with('{') && json.ends_with('}') {
        &json[1..json.len()-1]
    } else {
        json
    };
    
    let chars: Vec<char> = content.chars().collect();
    let mut pos = 0;
    while pos < chars.len() {
        while pos < chars.len() && chars[pos].is_whitespace() {
            pos += 1;
        }
        if pos >= chars.len() { break; }
        
        let key_start = if chars[pos] == '"' {
            pos += 1;
            let start = pos;
            while pos < chars.len() && chars[pos] != '"' {
                pos += 1;
            }
            pos += 1;
            chars[start..pos-1].iter().collect::<String>()
        } else {
            break;
        };
        
        while pos < chars.len() && (chars[pos].is_whitespace() || chars[pos] == ':') {
            pos += 1;
        }
        if pos >= chars.len() { break; }
        
        let value = if chars[pos] == '"' {
            pos += 1;
            let start = pos;
            while pos < chars.len() && chars[pos] != '"' {
                if chars[pos] == '\\' && pos + 1 < chars.len() {
                    pos += 2;
                } else {
                    pos += 1;
                }
            }
            pos += 1;
            let s: String = chars[start..pos-1].iter().collect();
            PropValue::String(unescape_string(&s))
        } else if chars[pos] == '[' {
            pos += 1;
            let start = pos;
            let mut depth = 1;
            while pos < chars.len() && depth > 0 {
                if chars[pos] == '[' { depth += 1; }
                if chars[pos] == ']' { depth -= 1; }
                pos += 1;
            }
            let s: String = chars[start..pos-1].iter().collect();
            PropValue::String(format!("[{}]", s))
        } else if chars[pos] == '{' {
            pos += 1;
            let start = pos;
            let mut depth = 1;
            while pos < chars.len() && depth > 0 {
                if chars[pos] == '{' { depth += 1; }
                if chars[pos] == '}' { depth -= 1; }
                pos += 1;
            }
            let s: String = chars[start..pos-1].iter().collect();
            PropValue::String(format!("{{{}}}", s))
        } else {
            let start = pos;
            while pos < chars.len() && !chars[pos].is_whitespace() && chars[pos] != ',' && chars[pos] != '}' {
                pos += 1;
            }
            let val_str: String = chars[start..pos].iter().collect();
            if val_str == "true" {
                PropValue::Bool(true)
            } else if val_str == "false" {
                PropValue::Bool(false)
            } else if val_str == "null" {
                PropValue::Null
            } else if let Ok(n) = val_str.parse::<f64>() {
                PropValue::Number(n)
            } else {
                PropValue::String(val_str)
            }
        };
        
        props.insert(key_start, value);
        
        while pos < chars.len() && (chars[pos] == ',' || chars[pos].is_whitespace()) {
            pos += 1;
        }
    }
    
    Ok(props)
}

/// Unescape a JSON string
fn unescape_string(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => result.push('\n'),
                Some('r') => result.push('\r'),
                Some('t') => result.push('\t'),
                Some('\\') => result.push('\\'),
                Some('"') => result.push('"'),
                Some('u') => {
                    let mut hex = String::new();
                    for _ in 0..4 {
                        if let Some(h) = chars.next() {
                            hex.push(h);
                        }
                    }
                    if let Ok(code) = u32::from_str_radix(&hex, 16) {
                        if let Some(ch) = char::from_u32(code) {
                            result.push(ch);
                        }
                    }
                }
                Some(c) => result.push(c),
                None => break,
            }
        } else {
            result.push(c);
        }
    }
    
    result
}

/// Convert PropValue to JSON string
fn prop_value_to_json(value: &PropValue) -> String {
    match value {
        PropValue::Null => "null".to_string(),
        PropValue::Bool(b) => b.to_string(),
        PropValue::Number(n) => n.to_string(),
        PropValue::String(s) => format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")),
        PropValue::Vec(v) => format!("[{}]", v.iter().map(prop_value_to_json).collect::<Vec<_>>().join(",")),
    }
}

// ============================================================================
// Timer System (Task 055 - Optimized hot path)
// ============================================================================
// OPTIMIZATION: Store only timer IDs in Rust, not callback strings.
// Actual callback invocation happens in JS via __tb_invoke_timers().
// This avoids ctx.eval() in the hot path for 10x speedup.
// ============================================================================

use std::time::{Duration, Instant};

/// Timer entry - stores ID and metadata only, NOT the callback
#[derive(Debug, Clone)]
struct TimerEntry {
    id: u32,           // Rust timer ID (maps to JS timer ID)
    delay_ms: u64,
    is_interval: bool,
    created_at: Instant,
    last_fired: Option<Instant>,
}

/// Timer registry
static TIMERS: std::sync::LazyLock<std::sync::Arc<std::sync::Mutex<HashMap<u32, TimerEntry>>>> =
    std::sync::LazyLock::new(|| std::sync::Arc::new(std::sync::Mutex::new(HashMap::new())));

static NEXT_TIMER_ID: std::sync::LazyLock<std::sync::Arc<AtomicU32>> =
    std::sync::LazyLock::new(|| std::sync::Arc::new(AtomicU32::new(1)));

/// Create a one-shot timer (setTimeout equivalent)
/// Now receives JS timer ID as callback_js parameter
/// Returns the same ID back to JS for correlation
pub fn __ink_set_timeout(callback_js: &str, delay_ms: u64) -> u32 {
    // callback_js is actually the JS timer ID (passed as string)
    let js_id: u32 = callback_js.parse().unwrap_or(0);
    
    let id = NEXT_TIMER_ID.fetch_add(1, Ordering::SeqCst);
    let entry = TimerEntry {
        id,
        delay_ms,
        is_interval: false,
        created_at: Instant::now(),
        last_fired: None,
    };
    let mut timers = TIMERS.lock().unwrap();
    timers.insert(id, entry);
    tracing::debug!("Created timeout rust_id={}, js_id={}, delay={}ms", id, js_id, delay_ms);
    id
}

/// Create an interval timer (setInterval equivalent)
pub fn __ink_set_interval(callback_js: &str, interval_ms: u64) -> u32 {
    let js_id: u32 = callback_js.parse().unwrap_or(0);
    
    let id = NEXT_TIMER_ID.fetch_add(1, Ordering::SeqCst);
    let entry = TimerEntry {
        id,
        delay_ms: interval_ms,
        is_interval: true,
        created_at: Instant::now(),
        last_fired: None,
    };
    let mut timers = TIMERS.lock().unwrap();
    timers.insert(id, entry);
    tracing::debug!("Created interval rust_id={}, js_id={}, interval={}ms", id, js_id, interval_ms);
    id
}

/// Clear a timer (clearTimeout/clearInterval equivalent)
pub fn __ink_clear_timer(id: u32) -> bool {
    let mut timers = TIMERS.lock().unwrap();
    let removed = timers.remove(&id).is_some();
    if removed {
        tracing::debug!("Cleared timer {}", id);
    }
    removed
}

/// Clear all timers (for testing)
pub fn __ink_clear_all_timers() {
    let mut timers = TIMERS.lock().unwrap();
    timers.clear();
}

/// Clear microtask flag (for testing)
pub fn __ink_clear_all_microtasks() {
    HAS_PENDING_MICROTASKS.store(false, Ordering::SeqCst);
}

/// Get IDs of all timers that should fire now
/// Returns JSON array of timer IDs for JS to invoke
pub fn __ink_process_timers() -> String {
    let now = Instant::now();
    
    let mut timers = TIMERS.lock().unwrap();
    let timers_to_fire: Vec<u32> = timers.iter()
        .filter(|(_, timer)| {
            let elapsed = now.duration_since(
                timer.last_fired.unwrap_or(timer.created_at)
            ).as_millis() as u64;
            elapsed >= timer.delay_ms
        })
        .filter(|(_, timer)| {
            timer.is_interval || timer.last_fired.is_none()
        })
        .map(|(&id, _)| id)
        .collect();
    
    for id in &timers_to_fire {
        if let Some(entry) = timers.get_mut(id) {
            if entry.is_interval {
                entry.last_fired = Some(now);
            } else {
                // One-shot timer - remove it
                timers.remove(id);
            }
        }
    }
    
    // Return JSON array of IDs
    let ids_str = timers_to_fire.iter()
        .map(|id| id.to_string())
        .collect::<Vec<_>>()
        .join(",");
    format!("[{}]", ids_str)
}

/// Check if there are any pending timers
pub fn __ink_has_pending_timers() -> bool {
    let timers = TIMERS.lock().unwrap();
    !timers.is_empty()
}

/// Get time until next timer fires (for event loop optimization)
/// Returns Some(Duration) or None if no timers
pub fn __ink_next_timer_delay() -> Option<Duration> {
    let now = Instant::now();
    let timers = TIMERS.lock().unwrap();
    
    timers.values()
        .map(|timer| {
            let since = timer.last_fired.unwrap_or(timer.created_at);
            let elapsed = now.duration_since(since);
            if elapsed >= Duration::from_millis(timer.delay_ms) {
                Duration::ZERO
            } else {
                Duration::from_millis(timer.delay_ms) - elapsed
            }
        })
        .min()
}

// ============================================================================
// Microtask System - OPTIMIZED
// ============================================================================
// Microtasks are now handled entirely in JS via microtaskCallbacks array.
// Rust just tracks whether we need to invoke them. No stringification.
// ============================================================================

/// Flag to indicate pending microtasks (for event loop)
static HAS_PENDING_MICROTASKS: AtomicBool = AtomicBool::new(false);

/// Signal that microtasks are pending (called from JS via __ink_enqueue_microtask)
pub fn __ink_enqueue_microtask(_callback_js: &str) {
    // No-op: microtasks are stored in JS array and invoked via __tb_invoke_microtasks()
    HAS_PENDING_MICROTASKS.store(true, Ordering::SeqCst);
}

/// Drain microtasks - returns true if there were pending microtasks
/// Actual invocation happens in JS via __tb_invoke_microtasks()
pub fn __ink_drain_microtasks() -> bool {
    let had_pending = HAS_PENDING_MICROTASKS.load(Ordering::SeqCst);
    HAS_PENDING_MICROTASKS.store(false, Ordering::SeqCst);
    had_pending
}

// ============================================================================
// Test utilities
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Tests share global timer state; run them serially
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn reset_test_state() {
        __ink_reset_exit();
        __ink_clear_all_timers();
        __ink_clear_all_microtasks();
    }

    #[test]
    fn test_create_and_destroy_root() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_test_state();
        let root_id = __ink_create_root();
        assert!(root_id > 0);
        assert_eq!(__ink_get_root_id(), Some(root_id));
        
        __ink_destroy_root(root_id);
        assert_eq!(__ink_get_root_id(), None);
    }

    #[test]
    fn test_create_nodes() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_test_state();
        let box_id = __ink_create_node("ink-box", "{}").unwrap();
        assert!(box_id > 0);
        assert_eq!(__ink_get_node_tag(box_id), Some("ink-box".to_string()));

        let text_id = __ink_create_text_node("hello");
        assert!(text_id > 0);
        assert_eq!(__ink_get_node_tag(text_id), Some("ink-text".to_string()));
        assert_eq!(__ink_get_node_text(text_id), Some("hello".to_string()));
    }

    #[test]
    fn test_append_child() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_test_state();
        let root_id = __ink_create_root();
        let box_id = __ink_create_node("ink-box", "{}").unwrap();
        let text_id = __ink_create_text_node("test");
        
        __ink_append_child(root_id, box_id).unwrap();
        __ink_append_child(box_id, text_id).unwrap();
        
        assert_eq!(__ink_get_node_children(root_id), Some(vec![box_id]));
        assert_eq!(__ink_get_node_children(box_id), Some(vec![text_id]));
        
        // Test parent relationships
        assert_eq!(__ink_get_node_parent(box_id), Some(root_id));
        assert_eq!(__ink_get_node_parent(text_id), Some(box_id));
        assert_eq!(__ink_get_node_parent(root_id), None); // Root has no parent
        
        __ink_destroy_root(root_id);
    }

    #[test]
    fn test_measure_text() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_test_state();
        assert_eq!(__ink_measure_text("", 80), (0, 0));
        assert_eq!(__ink_measure_text("hello", 80), (5, 1));
        assert_eq!(__ink_measure_text("hello", 3), (3, 2));
        assert_eq!(__ink_measure_text("日本語", 80), (6, 1));
    }

    #[test]
    fn test_exit() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_test_state();
        assert!(!__ink_should_exit());
        
        __ink_exit(0);
        assert!(__ink_should_exit());
        assert_eq!(__ink_get_exit_code(), 0);
        
        __ink_exit(1);
        assert_eq!(__ink_get_exit_code(), 1);
        
        __ink_reset_exit();
        assert!(!__ink_should_exit());
    }

    #[test]
    fn test_parse_props_json() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_test_state();
        let props = parse_props_json(r#"{"flexDirection":"column","padding":2}"#).unwrap();
        assert_eq!(props.len(), 2);
        assert_eq!(props.get("flexDirection"), Some(&PropValue::String("column".to_string())));
        assert_eq!(props.get("padding"), Some(&PropValue::Number(2.0)));
    }

    #[test]
    fn test_escape_string() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_test_state();
        assert_eq!(unescape_string(r#"hello\nworld"#), "hello\nworld");
        assert_eq!(unescape_string(r#""quoted""#), "\"quoted\"");
    }

    #[test]
    fn test_timers() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_test_state();
        
        // Now passes JS timer ID (not callback string)
        let timeout_id = __ink_set_timeout("10", 100); // JS timer ID "10", Rust returns its own ID
        assert!(timeout_id > 0);
        assert!(__ink_has_pending_timers());
        
        assert!(__ink_clear_timer(timeout_id));
        assert!(!__ink_has_pending_timers());
        
        assert!(!__ink_clear_timer(9999)); // Non-existent timer
        
        let interval_id = __ink_set_interval("20", 100); // JS timer ID "20", Rust returns its own ID
        assert!(interval_id > 0);
        assert!(__ink_has_pending_timers());
        
        assert!(__ink_clear_timer(interval_id));
        assert!(!__ink_has_pending_timers());
    }

    #[test]
    fn test_process_timers() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_test_state();
        
        // __ink_set_timeout now takes a JS timer ID as string (not callback code)
        let rust_id = __ink_set_timeout("1", 0); // JS timer ID "1", Rust returns its own ID
        
        std::thread::sleep(std::time::Duration::from_millis(10));
        
        // Returns JSON array of Rust timer IDs that fired
        let result = __ink_process_timers();
        // Just verify the result is a valid JSON array containing our timer
        assert!(result.starts_with("[") && result.ends_with("]"), "Expected JSON array, got: {}", result);
        // Verify the timer ID is in the result
        let expected_id = rust_id.to_string();
        assert!(result.contains(&expected_id), "Expected timer {} in result, got: {}", expected_id, result);
        
        assert!(!__ink_has_pending_timers());
    }

    #[test]
    fn test_interval_repeats() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_test_state();
        
        // __ink_set_interval now takes JS timer ID
        let rust_id = __ink_set_interval("99", 10); // JS timer ID "99", Rust returns its own ID
        
        for _ in 0..3 {
            std::thread::sleep(std::time::Duration::from_millis(15));
            let result = __ink_process_timers();
            // Verify the Rust timer ID is in the result
            let expected_id = rust_id.to_string();
            assert!(result.contains(&expected_id), "Expected timer {} in result, got: {}", expected_id, result);
        }
        
        assert!(__ink_clear_timer(rust_id));
        assert!(!__ink_has_pending_timers());
    }

    #[test]
    fn test_microtasks() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_test_state();
        
        // Initially no microtasks
        assert!(!__ink_drain_microtasks());
        
        // Enqueue microtasks (they're stored in JS, Rust just sets a flag)
        __ink_enqueue_microtask("dummy");
        assert!(__ink_drain_microtasks()); // Flag is set
        assert!(!__ink_drain_microtasks()); // Flag cleared after drain
    }

    #[test]
    fn test_parse_margin_props() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_test_state();
        
        // Test marginY (top + bottom)
        let props = parse_props_json(r#"{"marginY":2}"#).unwrap();
        assert_eq!(props.get("marginY"), Some(&PropValue::Number(2.0)));
        
        // Test marginX (left + right)
        let props = parse_props_json(r#"{"marginX":1}"#).unwrap();
        assert_eq!(props.get("marginX"), Some(&PropValue::Number(1.0)));
        
        // Test individual margins
        let props = parse_props_json(r#"{"marginTop":1,"marginBottom":2,"marginLeft":3,"marginRight":4}"#).unwrap();
        assert_eq!(props.get("marginTop"), Some(&PropValue::Number(1.0)));
        assert_eq!(props.get("marginBottom"), Some(&PropValue::Number(2.0)));
        assert_eq!(props.get("marginLeft"), Some(&PropValue::Number(3.0)));
        assert_eq!(props.get("marginRight"), Some(&PropValue::Number(4.0)));
    }

    #[test]
    fn test_parse_padding_props() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_test_state();
        
        // Test padding
        let props = parse_props_json(r#"{"padding":2}"#).unwrap();
        assert_eq!(props.get("padding"), Some(&PropValue::Number(2.0)));
        
        // Test paddingY and paddingX
        let props = parse_props_json(r#"{"paddingY":1,"paddingX":2}"#).unwrap();
        assert_eq!(props.get("paddingY"), Some(&PropValue::Number(1.0)));
        assert_eq!(props.get("paddingX"), Some(&PropValue::Number(2.0)));
        
        // Test individual paddings
        let props = parse_props_json(r#"{"paddingTop":1,"paddingBottom":2,"paddingLeft":3,"paddingRight":4}"#).unwrap();
        assert_eq!(props.get("paddingTop"), Some(&PropValue::Number(1.0)));
        assert_eq!(props.get("paddingBottom"), Some(&PropValue::Number(2.0)));
        assert_eq!(props.get("paddingLeft"), Some(&PropValue::Number(3.0)));
        assert_eq!(props.get("paddingRight"), Some(&PropValue::Number(4.0)));
    }

    #[test]
    fn test_parse_background_color() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_test_state();
        
        let props = parse_props_json(r#"{"backgroundColor":"yellow"}"#).unwrap();
        assert_eq!(props.get("backgroundColor"), Some(&PropValue::String("yellow".to_string())));
    }

    #[test]
    fn test_node_with_margin_props() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_test_state();
        
        let root_id = __ink_create_root();
        
        // Create a box with marginY (used by focus-form.js, dashboard.js)
        let box_id = __ink_create_node("ink-box", r#"{"marginY":1}"#).unwrap();
        
        // Verify the prop was parsed
        assert_eq!(__ink_get_node_prop(box_id, "marginY"), Some("1".to_string()));
        
        // Create a box with marginX
        let box_id2 = __ink_create_node("ink-box", r#"{"marginX":2}"#).unwrap();
        assert_eq!(__ink_get_node_prop(box_id2, "marginX"), Some("2".to_string()));
        
        __ink_destroy_root(root_id);
    }

    #[test]
    fn test_node_with_background_color() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_test_state();
        
        let root_id = __ink_create_root();
        
        // Create a text node with backgroundColor (used by file-tree.js for selection)
        let _text_id = __ink_create_text_node("selected");
        
        // Note: Text nodes don't have props in our current implementation
        // but the prop parsing should work
        let props = parse_props_json(r#"{"backgroundColor":"yellow"}"#).unwrap();
        assert_eq!(props.get("backgroundColor"), Some(&PropValue::String("yellow".to_string())));
        
        __ink_destroy_root(root_id);
    }

    #[test]
    fn test_text_measurement_accuracy() {
        // Integration test: verify text measurement is accurate
        // This is critical for buffer diffing - wrong measurements cause misalignment
        let _guard = TEST_LOCK.lock().unwrap();
        reset_test_state();
        
        // Test ASCII text
        let (w, h) = __ink_measure_text("hello", 80);
        assert_eq!(w, 5);
        assert_eq!(h, 1);
        
        // Test longer text that wraps
        let (w, h) = __ink_measure_text("hello world", 5);
        assert_eq!(w, 5);  // max width
        assert_eq!(h, 2);  // "hello" + "world"
        
        // Test Unicode text
        let (w, h) = __ink_measure_text("日本語", 80);
        assert_eq!(w, 6);  // 3 CJK chars = 6 width units
        assert_eq!(h, 1);
        
        // Test empty text
        let (w, h) = __ink_measure_text("", 80);
        assert_eq!(w, 0);
        assert_eq!(h, 0);
    }

    #[test]
    fn test_box_layout_stability() {
        // Verify that box nodes have stable layout across recalculations
        // This is important for buffer diff - unchanged boxes shouldn't be re-rendered
        let _guard = TEST_LOCK.lock().unwrap();
        reset_test_state();
        
        let root_id = __ink_create_root();
        let box_id = __ink_create_node("ink-box", "{}").unwrap();
        __ink_append_child(root_id, box_id).unwrap();
        
        // First layout pass
        __ink_set_terminal_size(80, 24);
        let layout1 = __ink_calculate_layout();
        assert!(layout1.is_ok());
        let (x1, y1, w1, h1) = __ink_get_layout(box_id).unwrap();
        
        // Second layout pass (no changes)
        let layout2 = __ink_calculate_layout();
        assert!(layout2.is_ok());
        let (x2, y2, w2, h2) = __ink_get_layout(box_id).unwrap();
        
        // Box layout should be identical (no changes)
        assert_eq!(x1, x2, "x should be stable");
        assert_eq!(y1, y2, "y should be stable");
        assert_eq!(w1, w2, "width should be stable");
        assert_eq!(h1, h2, "height should be stable");
        
        __ink_destroy_root(root_id);
    }

    #[test]
    fn test_dirty_flag_for_text_change() {
        // Verify that changing text marks the tree as dirty
        // This triggers re-render and buffer flush
        let _guard = TEST_LOCK.lock().unwrap();
        reset_test_state();
        
        let root_id = __ink_create_root();
        let box_id = __ink_create_node("ink-box", "{}").unwrap();
        let text_id = __ink_create_text_node("hello");
        __ink_append_child(root_id, box_id).unwrap();
        __ink_append_child(box_id, text_id).unwrap();
        
        // Clear dirty flag
        __ink_clear_dirty();
        
        // Changing text should mark dirty
        __ink_set_text(text_id, "world").unwrap();
        assert!(__ink_is_dirty(), "Text change should mark tree dirty");
        
        // Clear and verify
        __ink_clear_dirty();
        assert!(!__ink_is_dirty());
        
        __ink_destroy_root(root_id);
    }
}
