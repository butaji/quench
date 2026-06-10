# Task 088: Fix Double JSON Parse on Every create_node

## Status: 🟠 **SIGNIFICANT PERFORMANCE — NOT STARTED**

## Goal
Eliminate redundant JSON parsing in the `__ink_create_node` path.

## Problem

Every `__ink_create_node` call parses JSON **twice**:

```rust
// 1. bridge/ffi.rs::call_ink_ffi — serde_json parse
let args = parse_args(args_json);  // serde_json::from_str

// 2. handle_create_node → __ink_create_node → parse_props_json
let props = parse_props_json(props);  // custom char parser
```

For a 2000-node tree: **4,000 JSON parses** on startup. Both parsers run on the same `props` data.

## Why This Happens

The FFI bridge passes all arguments as a JSON array string:
```js
__ink_call('create_node', JSON.stringify([tag, props]))
```

The Rust side parses the outer array with serde_json, then the inner props object with the custom parser.

## Fix Approaches

### Option A: Pass serde_json::Value Through

Change `__ink_call` to accept and return `serde_json::Value` instead of strings:

```rust
pub fn call_ink_ffi(method: &str, args: &serde_json::Value) -> serde_json::Value {
    // args is already parsed — no double parse
}
```

### Option B: Pass Props as HashMap Directly

Instead of stringifying props in JS, pass them as a pre-serialized flat object that Rust can deserialize once:

```js
// Instead of: __ink_call('create_node', JSON.stringify([tag, props]))
// Use: __ink_call('create_node', {tag, ...props})
```

### Option C: Cache Parsed Props

Store parsed props in a cache keyed by the raw JSON string. If the same props string appears again (common with repeated components), reuse the parsed HashMap.

## Acceptance Criteria
- [ ] `create_node` path does only one JSON parse (not two)
- [ ] Startup benchmark: 2000-node tree creation time improved
- [ ] All tests pass
- [ ] `cargo clippy` clean

## Files to Modify
- `src/bridge/ffi.rs` — Change parse_args to avoid double parse
- `src/bridge/node.rs` — Accept pre-parsed props
- `src/runtime.js` — May need adjustment to call format

## References
- Task 073 (Replace Custom JSON Parser)
