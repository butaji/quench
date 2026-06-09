# Task 254: `ink-event-target` Example — EventTarget and CustomEvent

**Priority:** P2-Medium
**Phase:** 22 — Web APIs + Event System
**Depends on:** 253

## Problem

`EventTarget` and `CustomEvent` provide the DOM-style event dispatch mechanism. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-event-target/tui/app.tsx
import React, { useState, useEffect } from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const [message, setMessage] = useState('');

  useEffect(() => {
    const target = new EventTarget();
    target.addEventListener('greet', ((e: CustomEvent) => {
      setMessage(e.detail.message);
    }) as EventListener);

    target.dispatchEvent(new CustomEvent('greet', { detail: { message: 'Hello from EventTarget' } }));
  }, []);

  return (
    <Box flexDirection="column">
      <Text>Message: {message}</Text>
    </Box>
  );
}
```


## HIR Coverage

- Standard `Expr::Call` + `Expr::Member` for runtime API access

## Compile-Path Codegen

- Runtime API mapping in codegen or bridge globals

## Acceptance Criteria

- [ ] Example exists at `examples/ink-event-target/`
- [ ] Uses `EventTarget`, `addEventListener`, `dispatchEvent`, `CustomEvent`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
