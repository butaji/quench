# Task 255: `ink-finalization-registry` Example — FinalizationRegistry

**Priority:** P2-Medium
**Phase:** 22 — Advanced Runtime APIs
**Depends on:** 254

## Problem

`FinalizationRegistry` registers cleanup callbacks for garbage-collected objects. Task 112 covers `WeakRef` but not `FinalizationRegistry`.

## Ink Example

```tsx
// examples/ink-finalization-registry/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const registry = new FinalizationRegistry<string>((heldValue) => {
    // Cleanup callback would run when object is GC'd.
    // Not deterministic in tests; this example exercises the API surface.
  });

  const obj = { name: 'temp' };
  registry.register(obj, 'cleanup-value');

  return (
    <Box flexDirection="column">
      <Text>Registered object</Text>
    </Box>
  );
}
```


## HIR Coverage

- Standard `Expr::Call` + `Expr::Member` for runtime API access

## Compile-Path Codegen

- Runtime API mapping in codegen or bridge globals

## Acceptance Criteria

- [ ] Example exists at `examples/ink-finalization-registry/`
- [ ] Uses `FinalizationRegistry` constructor and `register()`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
