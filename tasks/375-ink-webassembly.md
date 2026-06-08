# Task 375: `ink-webassembly` Example — `WebAssembly` API

**Priority:** P3-Low
**Phase:** 29 — Web Platform APIs
**Depends on:** 374

## Problem

`WebAssembly` provides near-native performance execution in browsers and Node.js. No existing Ink example exercises it.

## Ink Example

```tsx
// examples/ink-webassembly/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const wasm = new Uint8Array([
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00
  ]);
  const valid = WebAssembly.validate(wasm);

  return (
    <Box flexDirection="column">
      <Text>Valid: {String(valid)}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-webassembly/`
- [ ] Uses `WebAssembly` global detection
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
