# Task 069: Implement renderToString

## Status: PENDING

## Goal
Implement `renderToString()` for synchronous string rendering without terminal I/O. This is critical for testing, documentation generation, and server-side rendering scenarios.

## Ink API Reference

```typescript
import { renderToString } from 'ink';

const output = renderToString(
  <Box padding={1}>
    <Text color="green">Hello World</Text>
  </Box>,
  { columns: 40 }
);
// output: "\x1b[32m Hello World \x1b[39m\n" (with ANSI codes)
```

## Current State

Stub exists in `src/runtime.js`:
```javascript
function renderToString(element, options) {
  // TODO: Implement synchronous rendering without terminal I/O
  return '';
}
```

## Architecture Challenge

Current rendering pipeline:
```
JS reconciler → Rust FFI → Yoga layout → ratatui Terminal → Crossterm stdout
```

`renderToString` needs:
```
JS reconciler → Rust FFI → Yoga layout → ratatui Buffer → String (no Terminal, no Crossterm)
```

## Implementation Plan

### Phase 1: Rust Backend (No Terminal I/O)

Add `__ink_render_to_string` FFI function in Rust:

```rust
// src/bridge/ffi.rs
fn handle_render_to_string(args: &[String]) -> String {
    let root_id = parse_root_id(&args[0]);
    let columns = parse_columns(&args[1]);
    
    // Create a ratatui Buffer directly (no Terminal, no CrosstermBackend)
    let mut buf = Buffer::empty(Rect::new(0, 0, columns, 100));
    
    // Calculate layout
    bridge::__ink_set_terminal_size(columns as u32, 100);
    bridge::__ink_calculate_layout();
    
    // Render tree to buffer
    render_node(root_id, &mut buf, Rect::new(0, 0, columns, 100));
    
    // Convert buffer to string with ANSI codes
    buf_to_ansi_string(&buf)
}
```

### Phase 2: Buffer-to-String Conversion

Convert ratatui `Buffer` to ANSI string:

```rust
fn buf_to_ansi_string(buf: &Buffer) -> String {
    let mut result = String::new();
    let area = buf.area();
    
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            let cell = buf.get(x, y);
            // Append ANSI escape codes for style/color
            result.push_str(&cell_style_to_ansi(cell));
            result.push(cell.symbol().chars().next().unwrap_or(' '));
        }
        result.push('\n');
    }
    
    result
}
```

### Phase 3: JS Hook Integration

```javascript
function renderToString(element, options) {
  options = options || {};
  const columns = options.columns || 80;
  
  // Create temporary root
  const rootId = globalThis.__ink_create_root();
  
  // Mount element
  mountTree(element, rootId);
  globalThis.__ink_commit();
  
  // Render to string via Rust
  const result = globalThis.__ink_render_to_string(rootId, columns);
  
  // Clean up
  globalThis.__ink_destroy_root(rootId);
  
  return result;
}
```

## Acceptance Criteria

- [ ] `renderToString(<Text>hello</Text>)` returns `"hello\n"`
- [ ] `renderToString(<Text color="green">hello</Text>)` returns ANSI green code + "hello" + reset
- [ ] `renderToString(..., { columns: 40 })` respects width constraint
- [ ] `renderToString()` works with nested `<Box>` and `<Text>`
- [ ] Terminal-specific hooks return no-op defaults (useInput, useApp, etc.)
- [ ] Static component output prepended to dynamic output
- [ ] No terminal state mutated (no cursor hide, no raw mode)
- [ ] No side effects on global terminal state

## Files to Modify

- `src/bridge/ffi.rs` — Add `render_to_string` handler
- `src/render.rs` — Add `render_to_buffer()` function (no Terminal)
- `src/runtime.js` — Implement `renderToString()` using new FFI

## Testing

```tsx
// tests/render-to-string.test.tsx
import { renderToString, Box, Text } from 'ink';

const output = renderToString(
  <Box borderStyle="round" padding={1}>
    <Text color="green">Hello</Text>
  </Box>
);

console.assert(output.includes('Hello'));
console.assert(output.includes('\x1b[32m')); // Green ANSI code
```

## References
- Ink renderToString: https://github.com/vadimdemedes/ink/blob/master/src/render-to-string.tsx
- ratatui Buffer: https://docs.rs/ratatui/latest/ratatui/buffer/struct.Buffer.html
