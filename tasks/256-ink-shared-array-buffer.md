# Task 256: `ink-shared-array-buffer` Example — SharedArrayBuffer

**Priority:** P3-Low
**Phase:** 22 — Advanced Runtime APIs
**Depends on:** 255

## Problem

`SharedArrayBuffer` enables sharing raw binary data between workers/threads. Task 229 covers `Atomics` but not `SharedArrayBuffer` explicitly.

## Ink Example

```tsx
// examples/ink-shared-array-buffer/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const buffer = new SharedArrayBuffer(16);
  const view = new Int32Array(buffer);
  view[0] = 42;

  return (
    <Box flexDirection="column">
      <Text>Buffer byteLength: {buffer.byteLength}</Text>
      <Text>View[0]: {view[0]}</Text>
    </Box>
  );
}
```


## HIR Coverage

- Standard `Expr::Call` + `Expr::Member` for runtime API access

## Compile-Path Codegen

- Runtime API mapping in codegen or bridge globals

## Acceptance Criteria

- [ ] Example exists at `examples/ink-shared-array-buffer/`
- [ ] Uses `SharedArrayBuffer` constructor
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
