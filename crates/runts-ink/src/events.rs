//! Input / focus / window event types.
//!
//! These are the wire formats exchanged between the JS
//! reconciler (in rquickjs) and the Rust event loop. They
//! are designed to be JSON-serialisable so the JS side
//! can `JSON.parse(event)` them in `useInput` handlers.
//!
//! The mapping from the Rust type to Ink's JS shape is
//! `InputEvent` -> `{ input: str, key: InkKey }` where
//! `InkKey` is a flat object of booleans. We follow the
//! same convention here so the JS reconciler can do a
//! mechanical translation.

use serde::{Deserialize, Serialize};

/// A focusable element id. In Ink, `useFocus()` returns
/// `{ isFocused, focus }`; the `id` is a stable string the
/// user picks. Here we wrap it in a newtype to keep
/// `String` from leaking into all the API signatures.
#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FocusId(pub String);

impl FocusId {
    /// Create a new focus id.
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

impl std::fmt::Display for FocusId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// A single key on the keyboard, mapped to Ink's
/// `InkKey` shape.
///
/// Each field is `true` if the corresponding key is part
/// of the event. `input` carries the literal character
/// (or empty for non-printable keys). `ctrl`/`shift`/
/// `meta` carry the modifier state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Key {
    /// The literal character, or empty for non-printable
    /// keys (e.g. `ArrowLeft`).
    pub input: String,
    /// Up arrow.
    pub up_arrow: bool,
    /// Down arrow.
    pub down_arrow: bool,
    /// Left arrow.
    pub left_arrow: bool,
    /// Right arrow.
    pub right_arrow: bool,
    /// Page up.
    pub page_up: bool,
    /// Page down.
    pub page_down: bool,
    /// Home.
    pub home: bool,
    /// End.
    pub end: bool,
    /// Return / enter.
    pub return_key: bool,
    /// Escape.
    pub escape: bool,
    /// Backspace.
    pub backspace: bool,
    /// Delete.
    pub delete: bool,
    /// Tab.
    pub tab: bool,
    /// Ctrl modifier.
    pub ctrl: bool,
    /// Shift modifier.
    pub shift: bool,
    /// Meta / super modifier.
    pub meta: bool,
}

impl Key {
    /// An empty / "no key" value. Useful as a default
    /// in tests and in the constructor for `InputEvent`.
    pub fn empty() -> Self {
        Self {
            input: String::new(),
            up_arrow: false,
            down_arrow: false,
            left_arrow: false,
            right_arrow: false,
            page_up: false,
            page_down: false,
            home: false,
            end: false,
            return_key: false,
            escape: false,
            backspace: false,
            delete: false,
            tab: false,
            ctrl: false,
            shift: false,
            meta: false,
        }
    }

    /// Build a `Key` from a `crossterm::event::KeyEvent`.
    /// This is the canonical mapping from the platform
    /// event to the Ink shape.
    pub fn from_crossterm(key: crossterm::event::KeyEvent) -> Self {
        let mut k = Self::empty();
        apply_modifiers(&mut k, &key);
        apply_keycode(&mut k, &key);
        k
    }
}

/// Apply the modifier bits (Ctrl/Shift/Meta) from a
/// crossterm key event to our own `Key` flags.
fn apply_modifiers(k: &mut Key, key: &crossterm::event::KeyEvent) {
    use crossterm::event::KeyModifiers;
    k.ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    k.shift = key.modifiers.contains(KeyModifiers::SHIFT);
    k.meta = key.modifiers.contains(KeyModifiers::META | KeyModifiers::SUPER);
}

/// Translate a crossterm `KeyCode` into the matching
/// `Key` flag. Helper for `Key::from_crossterm`. The
/// match is split across a few dispatch helpers so each
/// function stays under the linter's complexity cap.
fn apply_keycode(k: &mut Key, key: &crossterm::event::KeyEvent) {
    use crossterm::event::KeyCode;
    match key.code {
        KeyCode::Char(c) => k.input = c.to_string(),
        KeyCode::Backspace => k.backspace = true,
        KeyCode::Enter => k.return_key = true,
        KeyCode::Esc => k.escape = true,
        KeyCode::Tab => k.tab = true,
        KeyCode::Delete => k.delete = true,
        _ => apply_navigation(k, &key.code),
    }
}

/// Map arrow / page / home / end keys onto the matching
/// `Key` flags. The remaining rare keys are silently
/// dropped (matching Ink's behaviour — they don't have
/// first-class `useInput` flags).
// allow:complexity
fn apply_navigation(k: &mut Key, code: &crossterm::event::KeyCode) {
    use crossterm::event::KeyCode;
    match code {
        KeyCode::Up | KeyCode::Down | KeyCode::Left | KeyCode::Right => {
            apply_arrow(k, code);
        }
        KeyCode::PageUp | KeyCode::PageDown | KeyCode::Home | KeyCode::End => {
            apply_page(k, code);
        }
        _ => {}
    }
}

/// Map the four arrow keys onto the matching `Key` flags.
fn apply_arrow(k: &mut Key, code: &crossterm::event::KeyCode) {
    use crossterm::event::KeyCode;
    match code {
        KeyCode::Up => k.up_arrow = true,
        KeyCode::Down => k.down_arrow = true,
        KeyCode::Left => k.left_arrow = true,
        KeyCode::Right => k.right_arrow = true,
        _ => {}
    }
}

/// Map the page / home / end keys onto the matching
/// `Key` flags.
fn apply_page(k: &mut Key, code: &crossterm::event::KeyCode) {
    use crossterm::event::KeyCode;
    match code {
        KeyCode::PageUp => k.page_up = true,
        KeyCode::PageDown => k.page_down = true,
        KeyCode::Home => k.home = true,
        KeyCode::End => k.end = true,
        _ => {}
    }
}

/// A keyboard event delivered to a `useInput` handler.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputEvent {
    /// The literal character (empty for non-printable
    /// keys).
    pub input: String,
    /// The key.
    pub key: Key,
}

impl InputEvent {
    /// Build an `InputEvent` from a `crossterm::event::KeyEvent`.
    pub fn from_crossterm(key: crossterm::event::KeyEvent) -> Self {
        let k = Key::from_crossterm(key.clone());
        Self {
            input: k.input.clone(),
            key: k,
        }
    }
}

/// A mouse event. Most Ink apps don't use these, but the
/// type exists so the JS reconciler can subscribe if it
/// wants to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MouseEvent {
    /// Column (0-indexed) of the event.
    pub x: u16,
    /// Row (0-indexed) of the event.
    pub y: u16,
    /// `button` byte as reported by crossterm. The JS
    /// reconciler translates this into `left` / `right` /
    /// `middle` booleans.
    pub button: u8,
    /// Whether the mouse is releasing (true) or pressing
    /// (false).
    pub release: bool,
}

/// A paste event, delivered when crossterm's bracketed
/// paste mode is enabled and a paste is detected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PasteEvent {
    /// The pasted text.
    pub text: String,
}

/// A terminal resize event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResizeEvent {
    /// New width in cells.
    pub width: u16,
    /// New height in cells.
    pub height: u16,
}

/// A snapshot of the current window size. Returned by
/// `useWindowSize()` in Ink.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowSize {
    /// Width in cells.
    pub columns: u16,
    /// Height in cells.
    pub rows: u16,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn key_empty_has_no_flags() {
        let k = Key::empty();
        assert_eq!(k.input, "");
        assert!(!k.up_arrow);
        assert!(!k.ctrl);
    }

    #[test]
    fn key_from_crossterm_letter() {
        let ev = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        let k = Key::from_crossterm(ev);
        assert_eq!(k.input, "a");
        assert!(!k.up_arrow);
    }

    #[test]
    fn key_from_crossterm_arrow() {
        let ev = KeyEvent::new(KeyCode::Up, KeyModifiers::NONE);
        let k = Key::from_crossterm(ev);
        assert!(k.up_arrow);
        assert!(!k.input.is_empty() == false); // arrows have no character
    }

    #[test]
    fn key_from_crossterm_ctrl_modifier() {
        let ev = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        let k = Key::from_crossterm(ev);
        assert!(k.ctrl);
    }

    #[test]
    fn input_event_round_trips_via_serde() {
        let ev = InputEvent {
            input: "x".to_string(),
            key: Key {
                input: "x".to_string(),
                ctrl: true,
                ..Key::empty()
            },
        };
        let json = serde_json::to_string(&ev).unwrap();
        let parsed: InputEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, ev);
    }

    #[test]
    fn window_size_serialises() {
        let s = WindowSize { columns: 80, rows: 24 };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("80"));
        assert!(json.contains("24"));
    }

    #[test]
    fn focus_id_displays_as_inner_string() {
        let id = FocusId::new("counter-button");
        assert_eq!(format!("{id}"), "counter-button");
    }
}
