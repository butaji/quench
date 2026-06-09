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

/// Callback registry for input handlers (using strings for simplicity)
static INPUT_CALLBACKS: std::sync::LazyLock<std::sync::Arc<std::sync::Mutex<HashMap<u32, String>>>> =
    std::sync::LazyLock::new(|| std::sync::Arc::new(std::sync::Mutex::new(HashMap::new())));

static NEXT_CALLBACK_ID: std::sync::LazyLock<std::sync::Arc<AtomicU32>> =
    std::sync::LazyLock::new(|| std::sync::Arc::new(AtomicU32::new(1)));

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
    // TODO: Query actual terminal state via crossterm
    false
}

/// Set raw mode on stdin
pub fn __ink_set_raw_mode(_enabled: bool) {
    // TODO: Toggle crossterm raw mode
}

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

/// Get node prop
pub fn __ink_get_node_prop(node_id: u32, prop: &str) -> Option<String> {
    INK_RUNTIME.with(|runtime| {
        runtime.borrow().node(node_id)
            .and_then(|n| n.props.get(prop))
            .map(prop_value_to_json)
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
    
    // Simple JSON parser for props - handles basic cases
    // For production, use serde_json
    let mut props = HashMap::new();
    
    // Remove surrounding braces if present
    let json = json.trim();
    let content = if json.starts_with('{') && json.ends_with('}') {
        &json[1..json.len()-1]
    } else {
        json
    };
    
    // Parse key-value pairs
    let chars: Vec<char> = content.chars().collect();
    let mut pos = 0;
    while pos < chars.len() {
        // Skip whitespace
        while pos < chars.len() && chars[pos].is_whitespace() {
            pos += 1;
        }
        if pos >= chars.len() { break; }
        
        // Parse key
        let key_start = if chars[pos] == '"' {
            pos += 1;
            let start = pos;
            while pos < chars.len() && chars[pos] != '"' {
                pos += 1;
            }
            pos += 1; // skip closing quote
            chars[start..pos-1].iter().collect::<String>()
        } else {
            break;
        };
        
        // Skip whitespace and colon
        while pos < chars.len() && (chars[pos].is_whitespace() || chars[pos] == ':') {
            pos += 1;
        }
        if pos >= chars.len() { break; }
        
        // Parse value
        let value = if chars[pos] == '"' {
            pos += 1;
            let start = pos;
            while pos < chars.len() && chars[pos] != '"' {
                if chars[pos] == '\\' && pos + 1 < chars.len() {
                    pos += 2; // skip escaped char
                } else {
                    pos += 1;
                }
            }
            pos += 1; // skip closing quote
            let s: String = chars[start..pos-1].iter().collect();
            PropValue::String(unescape_string(&s))
        } else if chars[pos] == '[' {
            // Array
            pos += 1;
            let start = pos;
            let mut depth = 1;
            while pos < chars.len() && depth > 0 {
                if chars[pos] == '[' { depth += 1; }
                if chars[pos] == ']' { depth -= 1; }
                pos += 1;
            }
            // Recursively parse array items
            let s: String = chars[start..pos-1].iter().collect();
            PropValue::String(format!("[{}]", s))
        } else if chars[pos] == '{' {
            // Object
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
            // Number or boolean or null
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
        
        // Skip comma
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
                    // Unicode escape
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
// Test utilities
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_and_destroy_root() {
        __ink_reset_exit();
        let root_id = __ink_create_root();
        assert!(root_id > 0);
        assert_eq!(__ink_get_root_id(), Some(root_id));
        
        __ink_destroy_root(root_id);
        assert_eq!(__ink_get_root_id(), None);
    }

    #[test]
    fn test_create_nodes() {
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
        let root_id = __ink_create_root();
        let box_id = __ink_create_node("ink-box", "{}").unwrap();
        let text_id = __ink_create_text_node("test");
        
        __ink_append_child(root_id, box_id).unwrap();
        __ink_append_child(box_id, text_id).unwrap();
        
        assert_eq!(__ink_get_node_children(root_id), Some(vec![box_id]));
        assert_eq!(__ink_get_node_children(box_id), Some(vec![text_id]));
        
        __ink_destroy_root(root_id);
    }

    #[test]
    fn test_measure_text() {
        assert_eq!(__ink_measure_text("", 80), (0, 0));
        assert_eq!(__ink_measure_text("hello", 80), (5, 1));
        assert_eq!(__ink_measure_text("hello", 3), (3, 2)); // Wrapped to 3-char lines
        assert_eq!(__ink_measure_text("日本語", 80), (6, 1)); // Wide chars (3 * 2 cells)
    }

    #[test]
    fn test_exit() {
        __ink_reset_exit();
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
        let props = parse_props_json(r#"{"flexDirection":"column","padding":2}"#).unwrap();
        assert_eq!(props.len(), 2);
        assert_eq!(props.get("flexDirection"), Some(&PropValue::String("column".to_string())));
        assert_eq!(props.get("padding"), Some(&PropValue::Number(2.0)));
    }

    #[test]
    fn test_escape_string() {
        assert_eq!(unescape_string(r#"hello\nworld"#), "hello\nworld");
        assert_eq!(unescape_string(r#""quoted""#), "\"quoted\"");
    }
}
