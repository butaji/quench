# Task 324: `ink-browser-globals` Example — `window`, `document`, `navigator`, `location`

**Priority:** P2-Medium
**Phase:** 26 — Browser Globals
**Depends on:** 323

## Problem

Browser globals (`window`, `document`, `navigator`, `location`, `history`) may appear in isomorphic or universal code. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-browser-globals/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Location: {window.location?.href ?? 'none'}</Text>
      <Text>UserAgent: {navigator.userAgent}</Text>
      <Text>Language: {navigator.language}</Text>
    </Box>
  );
}
```


## HIR Coverage

- Standard `Expr::Call` + `Expr::Member` for runtime API access

## Compile-Path Codegen

- Runtime API mapping in codegen or bridge globals

## Acceptance Criteria

- [ ] Example exists at `examples/ink-browser-globals/`
- [ ] Uses `window`, `document`, `navigator` detection
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust for browser globals
- [ ] Parity harness passes with 100% match in all 3 environments
