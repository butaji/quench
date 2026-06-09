# Task 307: `ink-fetch-api` Example — `fetch`, `Response`, `Request`

**Priority:** P1-High
**Phase:** 25 — Web APIs
**Depends on:** 306

## Problem

The `fetch` API with `Response` and `Request` constructors is a core web standard. Task 052 covers async fetch but not the full API surface.

## Ink Example

```tsx
// examples/ink-fetch-api/tui/app.tsx
import React, { useState, useEffect } from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const [status, setStatus] = useState('loading');

  useEffect(() => {
    const req = new Request('https://example.com');
    setStatus(`Method: ${req.method}, URL: ${req.url}`);
  }, []);

  return (
    <Box flexDirection="column">
      <Text>Request: {status}</Text>
    </Box>
  );
}
```


## HIR Coverage

- Standard `Expr::Call` + `Expr::Member` for runtime API access

## Compile-Path Codegen

- Runtime API mapping in codegen or bridge globals

## Acceptance Criteria

- [ ] Example exists at `examples/ink-fetch-api/`
- [ ] Uses `Request` constructor
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
