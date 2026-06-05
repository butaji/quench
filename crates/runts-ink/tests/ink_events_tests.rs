//! Unit tests for runts-ink events.
//!
//! These tests verify that the Ink-compatible event types
//! behave correctly and can be serialized/deserialized.

use runts_ink::{
    FocusId, InputEvent, Key, MouseEvent, PasteEvent, ResizeEvent, WindowSize,
};

#[test]
fn test_key_empty() {
    let k = Key::empty();
    assert_eq!(k.input, "");
    assert!(!k.up_arrow);
    assert!(!k.down_arrow);
    assert!(!k.left_arrow);
    assert!(!k.right_arrow);
    assert!(!k.page_up);
    assert!(!k.page_down);
    assert!(!k.home);
    assert!(!k.end);
    assert!(!k.return_key);
    assert!(!k.escape);
    assert!(!k.backspace);
    assert!(!k.delete);
    assert!(!k.tab);
    assert!(!k.ctrl);
    assert!(!k.shift);
    assert!(!k.meta);
}

#[test]
fn test_key_with_input() {
    let k = Key {
        input: "a".to_string(),
        ..Key::empty()
    };
    assert_eq!(k.input, "a");
}

#[test]
fn test_key_arrow_keys() {
    let mut k = Key::empty();
    k.up_arrow = true;
    assert!(k.up_arrow);
    assert!(!k.down_arrow);
    
    k = Key::empty();
    k.down_arrow = true;
    assert!(k.down_arrow);
    
    k = Key::empty();
    k.left_arrow = true;
    assert!(k.left_arrow);
    
    k = Key::empty();
    k.right_arrow = true;
    assert!(k.right_arrow);
}

#[test]
fn test_key_navigation() {
    let mut k = Key::empty();
    k.page_up = true;
    assert!(k.page_up);
    
    k = Key::empty();
    k.page_down = true;
    assert!(k.page_down);
    
    k = Key::empty();
    k.home = true;
    assert!(k.home);
    
    k = Key::empty();
    k.end = true;
    assert!(k.end);
}

#[test]
fn test_key_special_keys() {
    let mut k = Key::empty();
    k.return_key = true;
    assert!(k.return_key);
    
    k = Key::empty();
    k.escape = true;
    assert!(k.escape);
    
    k = Key::empty();
    k.backspace = true;
    assert!(k.backspace);
    
    k = Key::empty();
    k.delete = true;
    assert!(k.delete);
    
    k = Key::empty();
    k.tab = true;
    assert!(k.tab);
}

#[test]
fn test_key_modifiers() {
    let mut k = Key::empty();
    k.ctrl = true;
    assert!(k.ctrl);
    
    k = Key::empty();
    k.shift = true;
    assert!(k.shift);
    
    k = Key::empty();
    k.meta = true;
    assert!(k.meta);
}

#[test]
fn test_key_serde_roundtrip() {
    let keys = [
        Key::empty(),
        Key { input: "a".to_string(), ..Key::empty() },
        Key { input: "".to_string(), up_arrow: true, ..Key::empty() },
        Key { input: "c".to_string(), ctrl: true, ..Key::empty() },
        Key { 
            input: "A".to_string(), 
            shift: true, 
            up_arrow: true, 
            ..Key::empty() 
        },
    ];
    
    for key in keys {
        let json = serde_json::to_string(&key).unwrap();
        let parsed: Key = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.input, key.input);
        assert_eq!(parsed.up_arrow, key.up_arrow);
        assert_eq!(parsed.down_arrow, key.down_arrow);
        assert_eq!(parsed.left_arrow, key.left_arrow);
        assert_eq!(parsed.right_arrow, key.right_arrow);
        assert_eq!(parsed.page_up, key.page_up);
        assert_eq!(parsed.page_down, key.page_down);
        assert_eq!(parsed.home, key.home);
        assert_eq!(parsed.end, key.end);
        assert_eq!(parsed.return_key, key.return_key);
        assert_eq!(parsed.escape, key.escape);
        assert_eq!(parsed.backspace, key.backspace);
        assert_eq!(parsed.delete, key.delete);
        assert_eq!(parsed.tab, key.tab);
        assert_eq!(parsed.ctrl, key.ctrl);
        assert_eq!(parsed.shift, key.shift);
        assert_eq!(parsed.meta, key.meta);
    }
}

#[test]
fn test_input_event() {
    let ev = InputEvent {
        input: "x".to_string(),
        key: Key {
            input: "x".to_string(),
            ctrl: true,
            ..Key::empty()
        },
    };
    
    assert_eq!(ev.input, "x");
    assert!(ev.key.ctrl);
}

#[test]
fn test_input_event_serde_roundtrip() {
    let ev = InputEvent {
        input: "hello".to_string(),
        key: Key {
            input: "h".to_string(),
            shift: true,
            ..Key::empty()
        },
    };
    
    let json = serde_json::to_string(&ev).unwrap();
    let parsed: InputEvent = serde_json::from_str(&json).unwrap();
    
    assert_eq!(parsed.input, ev.input);
    assert_eq!(parsed.key.input, ev.key.input);
    assert_eq!(parsed.key.shift, ev.key.shift);
}

#[test]
fn test_mouse_event() {
    let ev = MouseEvent {
        x: 10,
        y: 20,
        button: 0,
        release: false,
    };
    
    assert_eq!(ev.x, 10);
    assert_eq!(ev.y, 20);
    assert_eq!(ev.button, 0);
    assert!(!ev.release);
}

#[test]
fn test_mouse_event_serde() {
    let ev = MouseEvent {
        x: 5,
        y: 15,
        button: 1,
        release: true,
    };
    
    let json = serde_json::to_string(&ev).unwrap();
    let parsed: MouseEvent = serde_json::from_str(&json).unwrap();
    
    assert_eq!(parsed.x, ev.x);
    assert_eq!(parsed.y, ev.y);
    assert_eq!(parsed.button, ev.button);
    assert_eq!(parsed.release, ev.release);
}

#[test]
fn test_paste_event() {
    let ev = PasteEvent {
        text: "hello world".to_string(),
    };
    
    assert_eq!(ev.text, "hello world");
}

#[test]
fn test_paste_event_serde() {
    let ev = PasteEvent {
        text: "pasted content".to_string(),
    };
    
    let json = serde_json::to_string(&ev).unwrap();
    let parsed: PasteEvent = serde_json::from_str(&json).unwrap();
    
    assert_eq!(parsed.text, ev.text);
}

#[test]
fn test_resize_event() {
    let ev = ResizeEvent {
        width: 80,
        height: 24,
    };
    
    assert_eq!(ev.width, 80);
    assert_eq!(ev.height, 24);
}

#[test]
fn test_resize_event_serde() {
    let ev = ResizeEvent {
        width: 120,
        height: 40,
    };
    
    let json = serde_json::to_string(&ev).unwrap();
    let parsed: ResizeEvent = serde_json::from_str(&json).unwrap();
    
    assert_eq!(parsed.width, ev.width);
    assert_eq!(parsed.height, ev.height);
}

#[test]
fn test_window_size() {
    let ws = WindowSize {
        columns: 80,
        rows: 24,
    };
    
    assert_eq!(ws.columns, 80);
    assert_eq!(ws.rows, 24);
}

#[test]
fn test_window_size_serde() {
    let ws = WindowSize {
        columns: 100,
        rows: 50,
    };
    
    let json = serde_json::to_string(&ws).unwrap();
    let parsed: WindowSize = serde_json::from_str(&json).unwrap();
    
    assert_eq!(parsed.columns, ws.columns);
    assert_eq!(parsed.rows, ws.rows);
}

#[test]
fn test_focus_id_new() {
    let id = FocusId::new("my-focus");
    assert_eq!(id.0, "my-focus");
}

#[test]
fn test_focus_id_display() {
    let id = FocusId::new("test-id");
    let display = format!("{}", id);
    assert_eq!(display, "test-id");
}

#[test]
fn test_focus_id_serde() {
    let id = FocusId::new("unique-id");
    
    let json = serde_json::to_string(&id).unwrap();
    let parsed: FocusId = serde_json::from_str(&json).unwrap();
    
    assert_eq!(parsed.0, id.0);
}

#[test]
fn test_focus_id_equality() {
    let id1 = FocusId::new("same");
    let id2 = FocusId::new("same");
    let id3 = FocusId::new("different");
    
    assert_eq!(id1, id2);
    assert_ne!(id1, id3);
}

#[test]
fn test_focus_id_hash() {
    use std::collections::HashSet;
    
    let mut set = HashSet::new();
    set.insert(FocusId::new("id1"));
    set.insert(FocusId::new("id2"));
    set.insert(FocusId::new("id1")); // Duplicate
    
    assert_eq!(set.len(), 2);
}
