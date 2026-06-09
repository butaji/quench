# Task 251: `ink-websocket` Example — WebSocket API

**Priority:** P2-Medium
**Phase:** 22 — Web APIs + Event System
**Depends on:** 250

## Problem

The `WebSocket` API provides bidirectional communication over TCP. No existing Ink example exercises it.

## Ink Example

```tsx
// examples/ink-websocket/tui/app.tsx
import React, { useState, useEffect } from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const [status, setStatus] = useState('connecting');

  useEffect(() => {
    // For parity harness, immediately update status since real WS
    // connection is non-deterministic in test environments.
    setStatus('mock-open');
  }, []);

  return (
    <Box flexDirection="column">
      <Text>WebSocket status: {status}</Text>
    </Box>
  );
}
```


## HIR Coverage

- Standard `Expr::Call` + `Expr::Member` for runtime API access

## Compile-Path Codegen

- Runtime API mapping in codegen or bridge globals

## Acceptance Criteria

- [ ] Example exists at `examples/ink-websocket/`
- [ ] Uses `WebSocket` constructor or type references
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust for WebSocket
- [ ] Parity harness passes with 100% match in all 3 environments
