# Task 073: Replace Custom JSON Parser with serde_json

## Status: 🔴 **CRITICAL BUG — NOT STARTED**

## Goal
Delete the 180-line hand-rolled JSON parser in `src/bridge/props.rs` and use `serde_json` (already a dependency).

## Problem

`parse_props_json()` in `src/bridge/props.rs` is a bespoke character-by-character parser that:
- Only handles flat objects (nested objects/arrays are stringified, not parsed)
- Has no spec compliance guarantees
- Is 180 lines of code to maintain and debug
- `serde_json` is **already a dependency** of the project

The parser also implements its own string unescaping (`unescape_string`), unicode escape handling (`handle_unicode_escape`), and primitive type detection — all of which `serde_json` does correctly and faster.

## Current Implementation

```rust
// src/bridge/props.rs — ~180 lines
fn parse_props_json(json: &str) -> Result<HashMap<String, PropValue>> { ... }
fn parse_props_from_chars(chars: &[char]) -> Result<...> { ... }
fn parse_string_value(chars: &[char], pos: &mut usize) -> PropValue { ... }
// ... plus skip_whitespace, parse_key, parse_array_value, parse_object_value,
//     parse_primitive_value, unescape_string, handle_unicode_escape
```

## Fix

Add `Serialize` and `Deserialize` derives to `PropValue`, then replace the entire module body:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PropValue {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Vec(Vec<PropValue>),
}

pub fn parse_props_json(json: &str) -> Result<HashMap<String, PropValue>> {
    if json.trim().is_empty() || json == "null" || json == "undefined" {
        return Ok(HashMap::new());
    }
    serde_json::from_str(json)
        .map_err(|e| FfiError::InvalidArgument(format!("JSON parse error: {e}")))
}
```

The `#[serde(untagged)]` attribute allows `serde_json` to infer the variant from the JSON type.

## Risks & Mitigations

| Risk | Mitigation |
|------|------------|
| `PropValue::Vec` deserialization | `#[serde(untagged)]` handles `Vec<PropValue>` correctly for nested arrays |
| `PropValue::Number` for integers | serde_json deserializes JSON numbers as `f64` by default — this matches current behavior |
| Backward compat with callers | `parse_props_json` signature stays identical; no caller changes needed |

## Acceptance Criteria
- [ ] `PropValue` derives `Serialize` and `Deserialize`
- [ ] All functions in `src/bridge/props.rs` except `parse_props_json` and `prop_value_to_json` are deleted
- [ ] All existing tests pass without modification
- [ ] Add test for nested JSON object: `{"nested":{"a":1}}` → `PropValue::String("{\"a\":1}")` or parsed object
- [ ] Add test for complex escape sequences: `{"text":"line1\\nline2\\u00e9"}`
- [ ] `cargo test` passes
- [ ] Performance is equal or better (serde_json is significantly faster)

## Files to Modify
- `src/ink/node.rs` — Add `Serialize, Deserialize` to `PropValue` derive
- `src/bridge/props.rs` — Replace custom parser with serde_json

## References
- serde_json docs: https://docs.rs/serde_json/latest/serde_json/
- Task 003 (Bridge: Create Nodes — original props parsing task)
