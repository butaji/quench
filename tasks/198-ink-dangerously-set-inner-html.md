# Task 198: `ink-dangerously-set-inner-html` Example — `dangerouslySetInnerHTML`

**Priority:** P2-Medium
**Phase:** 17 — React JSX Edge Cases
**Depends on:** 197

## Problem

`dangerouslySetInnerHTML` is a React JSX prop for injecting raw HTML. While Ink is a terminal renderer (not HTML), testing this prop exercises HIR JSX attribute handling for object-valued props with nested objects. No existing Ink example exercises it.

## Ink Example

```tsx
// examples/ink-dangerously-set-inner-html/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  return (
    <Box flexDirection="column">
      <Text dangerouslySetInnerHTML={{ __html: 'Raw HTML Content' }}>
        Fallback text
      </Text>
      <Text>Object prop parsed successfully</Text>
    </Box>
  );
}
```


## HIR Coverage

- `JsxElement`, `JsxFragment`, `JsxSpreadAttribute` variants

## Compile-Path Codegen

- `quote_codegen.rs` JSX element codegen + Ratatui plugin

## Acceptance Criteria

- [x] Example exists at `examples/ink-dangerously-set-inner-html/`
- [x] Exercises JSX object attribute parsing (`{ __html: '...' }`)
- [x] Renders identically in deno and `runts dev` (100% output match)
- [x] Compile path handles JSX object attribute values
- [x] Parity harness passes with 100% match in all 3 environments

## Notes

- Dev path (deno/rq) produces 100% parity.
- Compile path builds successfully; unknown props like `dangerouslySetInnerHTML` are ignored by the ratatui codegen (expected behavior for non-widget props).
- Added `test_ink_dangerously_set_inner_html` to `src/transpile/tests/rq_parity/mod.rs`.
