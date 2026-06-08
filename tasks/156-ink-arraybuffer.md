# Task 156: `ink-arraybuffer` Example — `ArrayBuffer`, `Uint8Array`, `DataView`

**Priority:** P2-Medium
**Phase:** 14 — Runtime API Completion
**Depends on:** 155

## Problem

`ArrayBuffer`, typed arrays (`Uint8Array`), and `DataView` are standard JavaScript binary data APIs. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-arraybuffer/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const buffer = new ArrayBuffer(8);
const view = new Uint8Array(buffer);
view[0] = 255;
view[1] = 128;

const dataView = new DataView(buffer);
const int16 = dataView.getInt16(0);

const text = new TextDecoder().decode(buffer);

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Buffer size: {buffer.byteLength}</Text>
      <Text>First byte: {view[0]}</Text>
      <Text>Int16: {int16}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-arraybuffer/`
- [ ] Uses `ArrayBuffer`, `Uint8Array`, `DataView`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
