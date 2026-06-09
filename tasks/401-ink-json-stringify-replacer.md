# Task 401: `ink-json-stringify-replacer` Example — JSON.stringify with Replacer Function

**Priority:** P2-Medium
**Phase:** 32 — Core Language + React + Runtime Edge Cases
**Depends on:** 400

## Problem

`JSON.stringify` with a replacer function allows selective serialization. No existing Ink example explicitly exercises this pattern.

## HIR Coverage

- `Expr::Call` for `JSON.stringify` with function argument.
- Arrow function as callback argument.

## Compile-Path Codegen

- `quote_codegen_exprs.inc` must emit compilable Rust for `JSON.stringify` with replacer callback.
- The replacer function must be preserved in generated code.

## Ink Example

```tsx
// examples/ink-json-stringify-replacer/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const data = { name: 'Alice', password: 'secret', age: 30 };

const filtered = JSON.stringify(data, (key, value) => {
  if (key === 'password') return undefined;
  return value;
});

export default function App() {
  return (
    <Box>
      <Text>{filtered}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-json-stringify-replacer/`
- [ ] Uses `JSON.stringify` with replacer function
- [ ] HIR callback argument produces compilable Rust
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
