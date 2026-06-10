# Task 093: Fix keycode_to_ink_name String Allocation

## Status: 🟢 **POLISH — NOT STARTED**

## Goal
Return `&'static str` from `keycode_to_ink_name` instead of allocating `String` on every keystroke.

## Problem

`src/render/keycode.rs::keycode_to_ink_name()` allocates a new `String` for every key event:

```rust
pub fn keycode_to_ink_name(key: &crossterm::event::KeyEvent) -> String {
    match key.code {
        KeyCode::Char(' ') => " ".to_string(),      // Allocates
        KeyCode::Enter => "return".to_string(),      // Allocates
        KeyCode::Up => "upArrow".to_string(),        // Allocates
        KeyCode::F(n) => format!("f{}", n),          // Allocates
        _ => format!("{:?}", key.code).to_lowercase(), // Allocates
    }
}
```

Every keystroke allocates 5-30 bytes. With rapid typing or gaming input, this is hundreds of allocations/second for a function that could return a static string slice.

## Fix

```rust
pub fn keycode_to_ink_name(key: &crossterm::event::KeyEvent) -> &'static str {
    match key.code {
        KeyCode::Char(' ') => " ",
        KeyCode::Char(c) => {
            // Need static storage for Char — use a lookup or leak small string
            // For 95 printable chars, a static array works:
            static CHAR_NAMES: [&str; 128] = [
                // ... map each ASCII char to its string representation
            ];
            CHAR_NAMES.get(c as usize).copied().unwrap_or("unknown")
        }
        KeyCode::Enter => "return",
        KeyCode::Esc => "escape",
        KeyCode::Backspace => "backspace",
        KeyCode::Delete => "delete",
        KeyCode::Tab | KeyCode::BackTab => "tab",
        KeyCode::Up => "upArrow",
        KeyCode::Down => "downArrow",
        KeyCode::Left => "leftArrow",
        KeyCode::Right => "rightArrow",
        KeyCode::Home => "home",
        KeyCode::End => "end",
        KeyCode::PageUp => "pageUp",
        KeyCode::PageDown => "pageDown",
        KeyCode::Insert => "insert",
        KeyCode::F(1) => "f1",
        KeyCode::F(2) => "f2",
        // ... etc, or use a match for F keys
        _ => "unknown",
    }
}
```

For `F(n)` keys and `Char(c)`, we need either:
- A small static lookup table
- `format!` for truly dynamic cases, with a note that it's rare
- Accept that `Char` variants need allocation and optimize the common cases

**Simpler approach:** Just return `&'static str` for the 99% case (special keys), and allocate only for `Char(c)` where `c` is dynamic.

## Acceptance Criteria
- [ ] Common keys (arrows, enter, escape, F1-F12) return `&'static str` with zero allocation
- [ ] `handle_key_event` updated to work with `&str` instead of `String`
- [ ] `cargo test` passes
- [ ] `cargo clippy` clean

## Files to Modify
- `src/render/keycode.rs` — Return `&'static str`
- `src/event_loop.rs` — `handle_key_event` signature update

## References
- Rust string types: https://doc.rust-lang.org/std/string/
