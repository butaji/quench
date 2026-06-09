# Task 314: `ink-urlpattern` Example — `URLPattern`

**Priority:** P3-Low
**Phase:** 25 — Web APIs
**Depends on:** 313

## Problem

`URLPattern` provides pattern matching for URLs. No existing Ink example exercises it.

## Ink Example

```tsx
// examples/ink-urlpattern/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const pattern = new URLPattern({ pathname: '/users/:id' });
  const match = pattern.exec('https://example.com/users/42');

  return (
    <Box flexDirection="column">
      <Text>Pathname: {match?.pathname.groups.id ?? 'none'}</Text>
      <Text>Test: {String(pattern.test('https://example.com/users/42'))}</Text>
    </Box>
  );
}
```


## HIR Coverage

- Standard `Expr::Call` + `Expr::Member` for runtime API access

## Compile-Path Codegen

- Runtime API mapping in codegen or bridge globals

## Acceptance Criteria

- [ ] Example exists at `examples/ink-urlpattern/`
- [ ] Uses `URLPattern` constructor and `exec`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
