//! Bridge: Node creation
//!
//! Functions for creating root, element, and text nodes.

#[cfg(test)]
use serial_test::serial;

use crate::ink::INK_RUNTIME;
use crate::bridge::props::parse_props_json;

// Re-export props functions for backwards compatibility
pub use crate::bridge::props::prop_value_to_json;
#[cfg(test)]
pub use crate::bridge::props::unescape_string;

/// Reset all bridge state for testing
pub fn reset_bridge_state() {
    crate::bridge::io::__ink_reset_exit();
    crate::bridge::timers::__ink_clear_all_timers();
    crate::bridge::timers::__ink_clear_all_microtasks();
    crate::bridge::timers::__ink_reset_timer_id();
}

/// Create the terminal root node
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

/// Create a new node with tag and props (props as JSON string)
pub fn __ink_create_node(tag: &str, props_json: &str) -> crate::bridge::Result<u32> {
    let props = parse_props_json(props_json)?;
    INK_RUNTIME.with(|runtime| {
        let mut r = runtime.borrow_mut();
        Ok(r.create_node(tag, props))
    })
}

/// Create a text node with content
pub fn __ink_create_text_node(text: &str) -> u32 {
    INK_RUNTIME.with(|runtime| {
        let mut r = runtime.borrow_mut();
        r.create_text_node(text)
    })
}

/// Parse a JSON element tree and build the Rust node tree
pub fn __ink_render_element(json: &str) -> u32 {
    let root_id = __ink_create_root();

    match serde_json::from_str::<serde_json::Value>(json) {
        Ok(val) => {
            build_element_tree(root_id, &val);
        }
        Err(e) => {
            tracing::error!("Malformed JSON in __ink_render_element: {}", e);
        }
    }

    crate::bridge::__ink_commit();
    root_id
}

/// Recursively build nodes from serde_json::Value
fn build_element_tree(parent_id: u32, element: &serde_json::Value) {
    match element {
        serde_json::Value::Null | serde_json::Value::Object(_) if is_empty_object(element) => {},
        serde_json::Value::String(s) => append_text_node(parent_id, s),
        serde_json::Value::Number(n) => append_text_node(parent_id, &n.to_string()),
        serde_json::Value::Bool(b) => append_text_node(parent_id, &b.to_string()),
        serde_json::Value::Array(arr) => build_array_tree(parent_id, arr),
        serde_json::Value::Object(map) => build_object_node(parent_id, map),
        _ => {}
    }
}

/// Check if a JSON value is an empty object
fn is_empty_object(val: &serde_json::Value) -> bool {
    val.as_object().map(|m| m.is_empty()).unwrap_or(false)
}

/// Append a text node to parent
fn append_text_node(parent_id: u32, text: &str) {
    let text_id = __ink_create_text_node(text);
    let _ = crate::bridge::__ink_append_child(parent_id, text_id);
}

/// Build element tree from array
fn build_array_tree(parent_id: u32, arr: &[serde_json::Value]) {
    for item in arr {
        build_element_tree(parent_id, item);
    }
}

/// Build a node from an object map
fn build_object_node(parent_id: u32, map: &serde_json::Map<String, serde_json::Value>) {
    if let Some(serde_json::Value::String(tag)) = map.get("type") {
        let props_json = extract_node_props(map);
        let node_id = __ink_create_node(tag, &props_json).unwrap_or(0);
        let _ = crate::bridge::__ink_append_child(parent_id, node_id);

        if let Some(children) = get_children(map) {
            build_element_tree(node_id, children);
        }
    } else {
        let text = serde_json::to_string(map).unwrap_or_default();
        append_text_node(parent_id, &text);
    }
}

/// Extract props JSON from node object
fn extract_node_props(map: &serde_json::Map<String, serde_json::Value>) -> String {
    map.get("props")
        .and_then(|p| {
            if let serde_json::Value::Object(props_map) = p {
                let mut filtered = props_map.clone();
                filtered.remove("children");
                serde_json::to_string(&serde_json::Value::Object(filtered)).ok()
            } else {
                None
            }
        })
        .unwrap_or_else(|| "{}".to_string())
}

/// Get children from node object (either top-level or in props)
fn get_children(map: &serde_json::Map<String, serde_json::Value>) -> Option<&serde_json::Value> {
    map.get("children").or_else(|| {
        map.get("props")
            .and_then(|p| p.get("children"))
    })
}

// ===================================================================
// Tests
// ===================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ink::PropValue;

    fn setup() {
        crate::ink::reset_runtime();
        reset_bridge_state();
    }

    #[test]
    #[serial]
    fn test_create_and_destroy_root() {
        setup();
        let root_id = __ink_create_root();
        assert!(root_id > 0);
        assert_eq!(crate::bridge::__ink_get_root_id(), Some(root_id));
        __ink_destroy_root(root_id);
        assert_eq!(crate::bridge::__ink_get_root_id(), None);
    }

    #[test]
    #[serial]
    fn test_create_nodes() {
        setup();
        let box_id = __ink_create_node("ink-box", "{}").unwrap();
        assert!(box_id > 0);
        assert_eq!(crate::bridge::__ink_get_node_tag(box_id), Some("ink-box".to_string()));

        let text_id = __ink_create_text_node("hello");
        assert!(text_id > 0);
        assert_eq!(crate::bridge::__ink_get_node_tag(text_id), Some("ink-text".to_string()));
        assert_eq!(crate::bridge::__ink_get_node_text(text_id), Some("hello".to_string()));
    }

    #[test]
    #[serial]
    fn test_append_child() {
        setup();
        let root_id = __ink_create_root();
        let box_id = __ink_create_node("ink-box", "{}").unwrap();
        let text_id = __ink_create_text_node("test");

        crate::bridge::__ink_append_child(root_id, box_id).unwrap();
        crate::bridge::__ink_append_child(box_id, text_id).unwrap();

        assert_eq!(crate::bridge::__ink_get_node_children(root_id), Some(vec![box_id]));
        assert_eq!(crate::bridge::__ink_get_node_children(box_id), Some(vec![text_id]));
        assert_eq!(crate::bridge::__ink_get_node_parent(box_id), Some(root_id));
        assert_eq!(crate::bridge::__ink_get_node_parent(text_id), Some(box_id));
        assert_eq!(crate::bridge::__ink_get_node_parent(root_id), None);

        __ink_destroy_root(root_id);
    }

    #[test]
    #[serial]
    fn test_measure_text() {
        setup();
        assert_eq!(crate::bridge::__ink_measure_text("", 80), (0, 0));
        assert_eq!(crate::bridge::__ink_measure_text("hello", 80), (5, 1));
        assert_eq!(crate::bridge::__ink_measure_text("hello", 3), (3, 2));
        assert_eq!(crate::bridge::__ink_measure_text("日本語", 80), (6, 1));
    }

    #[test]
    #[serial]
    fn test_exit() {
        setup();
        assert!(!crate::bridge::__ink_should_exit());

        crate::bridge::__ink_exit(0);
        assert!(crate::bridge::__ink_should_exit());
        assert_eq!(crate::bridge::__ink_get_exit_code(), 0);

        crate::bridge::__ink_exit(1);
        assert_eq!(crate::bridge::__ink_get_exit_code(), 1);

        crate::bridge::__ink_reset_exit();
        assert!(!crate::bridge::__ink_should_exit());
    }

    #[test]
    #[serial]
    fn test_parse_props_json() {
        setup();
        let props = parse_props_json(r#"{"flexDirection":"column","padding":2}"#).unwrap();
        assert_eq!(props.len(), 2);
        assert_eq!(props.get("flexDirection"), Some(&PropValue::String("column".to_string())));
        assert_eq!(props.get("padding"), Some(&PropValue::Number(2.0)));
    }

    #[test]
    #[serial]
    fn test_escape_string() {
        setup();
        assert_eq!(unescape_string(r#"hello\nworld"#), "hello\nworld");
        assert_eq!(unescape_string(r#""quoted""#), "\"quoted\"");
    }

    #[test]
    #[serial]
    fn test_timers() {
        setup();

        let timeout_id = crate::bridge::__ink_set_timeout("10", 100);
        assert!(timeout_id > 0);
        assert!(crate::bridge::__ink_has_pending_timers());

        assert!(crate::bridge::__ink_clear_timer(timeout_id));
        assert!(!crate::bridge::__ink_has_pending_timers());
        assert!(!crate::bridge::__ink_clear_timer(9999));

        let interval_id = crate::bridge::__ink_set_interval("20", 100);
        assert!(interval_id > 0);
        assert!(crate::bridge::__ink_has_pending_timers());
        assert!(crate::bridge::__ink_clear_timer(interval_id));
    }

    #[test]
    #[serial]
    fn test_process_timers() {
        setup();

        let rust_id = crate::bridge::__ink_set_timeout("1", 0);
        std::thread::sleep(std::time::Duration::from_millis(10));

        let result = crate::bridge::__ink_process_timers();
        assert!(result.starts_with("[") && result.ends_with("]"));
        assert!(result.contains(&rust_id.to_string()));

        assert!(!crate::bridge::__ink_has_pending_timers());
    }

    #[test]
    #[serial]
    fn test_interval_repeats() {
        setup();

        let rust_id = crate::bridge::__ink_set_interval("99", 10);

        for _ in 0..3 {
            std::thread::sleep(std::time::Duration::from_millis(15));
            let result = crate::bridge::__ink_process_timers();
            assert!(result.contains(&rust_id.to_string()));
        }

        assert!(crate::bridge::__ink_clear_timer(rust_id));
    }

    #[test]
    #[serial]
    fn test_microtasks() {
        setup();

        assert!(!crate::bridge::__ink_drain_microtasks());
        crate::bridge::__ink_enqueue_microtask("dummy");
        assert!(crate::bridge::__ink_drain_microtasks());
        assert!(!crate::bridge::__ink_drain_microtasks());
    }

    #[test]
    #[serial]
    fn test_parse_margin_props() {
        setup();

        let props = parse_props_json(r#"{"marginY":2}"#).unwrap();
        assert_eq!(props.get("marginY"), Some(&PropValue::Number(2.0)));

        let props = parse_props_json(r#"{"marginX":1}"#).unwrap();
        assert_eq!(props.get("marginX"), Some(&PropValue::Number(1.0)));

        let props = parse_props_json(r#"{"marginTop":1,"marginBottom":2,"marginLeft":3,"marginRight":4}"#).unwrap();
        assert_eq!(props.get("marginTop"), Some(&PropValue::Number(1.0)));
        assert_eq!(props.get("marginBottom"), Some(&PropValue::Number(2.0)));
        assert_eq!(props.get("marginLeft"), Some(&PropValue::Number(3.0)));
        assert_eq!(props.get("marginRight"), Some(&PropValue::Number(4.0)));
    }

    #[test]
    #[serial]
    fn test_parse_padding_props() {
        setup();

        let props = parse_props_json(r#"{"padding":2}"#).unwrap();
        assert_eq!(props.get("padding"), Some(&PropValue::Number(2.0)));

        let props = parse_props_json(r#"{"paddingY":1,"paddingX":2}"#).unwrap();
        assert_eq!(props.get("paddingY"), Some(&PropValue::Number(1.0)));
        assert_eq!(props.get("paddingX"), Some(&PropValue::Number(2.0)));
    }

    #[test]
    #[serial]
    fn test_parse_background_color() {
        setup();

        let props = parse_props_json(r#"{"backgroundColor":"yellow"}"#).unwrap();
        assert_eq!(props.get("backgroundColor"), Some(&PropValue::String("yellow".to_string())));
    }

    #[test]
    #[serial]
    fn test_text_measurement_accuracy() {
        setup();

        let (w, h) = crate::bridge::__ink_measure_text("hello", 80);
        assert_eq!(w, 5);
        assert_eq!(h, 1);

        let (w, h) = crate::bridge::__ink_measure_text("hello world", 5);
        assert_eq!(w, 5);
        assert_eq!(h, 2);

        let (w, h) = crate::bridge::__ink_measure_text("日本語", 80);
        assert_eq!(w, 6);
        assert_eq!(h, 1);

        let (w, h) = crate::bridge::__ink_measure_text("", 80);
        assert_eq!(w, 0);
        assert_eq!(h, 0);
    }

    #[test]
    fn test_dirty_flag_for_text_change() {
        setup();

        let root_id = __ink_create_root();
        let box_id = __ink_create_node("ink-box", "{}").unwrap();
        let text_id = __ink_create_text_node("hello");
        crate::bridge::__ink_append_child(root_id, box_id).unwrap();
        crate::bridge::__ink_append_child(box_id, text_id).unwrap();

        crate::bridge::__ink_clear_dirty();
        crate::bridge::__ink_set_text(text_id, "world").unwrap();
        assert!(crate::bridge::__ink_is_dirty());

        crate::bridge::__ink_clear_dirty();
        assert!(!crate::bridge::__ink_is_dirty());

        __ink_destroy_root(root_id);
    }

    #[test]
    #[serial]
    fn test_render_element_malformed_json() {
        setup();
        // Malformed JSON should NOT panic and should return a root node
        let root_id = __ink_render_element("{ not valid json");
        assert!(root_id > 0, "Should return a root node even with bad JSON");
        // Root should have no children
        assert_eq!(crate::bridge::__ink_get_node_children(root_id), Some(vec![]));
        __ink_destroy_root(root_id);
    }
}
