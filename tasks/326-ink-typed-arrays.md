# Task 326: `ink-typed-arrays` Example — Typed Arrays and `DataView`

**Priority:** P1-High
**Phase:** 26 — Binary Data APIs
**Depends on:** 325

## Problem

Typed arrays (`Int8Array`, `Uint8Array`, `Int16Array`, `Uint16Array`, `Int32Array`, `Uint32Array`, `Float32Array`, `Float64Array`, `BigInt64Array`, `BigUint64Array`) and `DataView` provide binary data manipulation. No dedicated example exercises all typed array variants.

## Ink Example

```tsx
// examples/ink-typed-arrays/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const u8 = new Uint8Array([1, 2, 3]);
  const i32 = new Int32Array([10, 20, 30]);
  const f64 = new Float64Array([1.5, 2.5]);
  const view = new DataView(new ArrayBuffer(4));
  view.setInt32(0, 42);

  return (
    <Box flexDirection="column">
      <Text>Uint8: {u8.join(', ')}</Text>
      <Text>Int32: {i32.join(', ')}</Text>
      <Text>Float64: {f64.join(', ')}</Text>
      <Text>DataView: {view.getInt32(0)}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-typed-arrays/`
- [ ] Uses multiple typed array constructors and `DataView`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
