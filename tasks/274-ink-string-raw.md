# Task 274: `ink-string-raw` Example — `String.raw`

**Priority:** P2-Medium
**Phase:** 23 — Runtime API Completion
**Depends on:** 273

## Problem

`String.raw` returns the raw string form of a template literal without interpreting escape sequences. No existing Ink example exercises it.

## Ink Example

```tsx
// examples/ink-string-raw/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const raw = String.raw`C:\Users\Alice\file.txt`;
  const normal = `C:\Users\Alice\file.txt`;

  return (
    <Box flexDirection="column">
      <Text>Raw: {raw}</Text>
      <Text>Normal: {normal}</Text>
    </Box>
  );
}
```


## HIR Coverage

- Standard `Expr::Call` + `Expr::Member` for runtime API access

## Compile-Path Codegen

- Runtime API mapping in codegen or bridge globals

## Acceptance Criteria

- [ ] Example exists at `examples/ink-string-raw/`
- [ ] Uses `String.raw` template tag
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
