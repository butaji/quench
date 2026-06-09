# Task 433: `ink-headers-methods` Example — `Headers.append`, `delete`, `get`, `has`, `set`, `entries`, `keys`, `values`

**Priority:** P2-Medium
**Phase:** 34 — HIR & Codegen Edge Cases
**Depends on:** 432

## Problem

`Headers` instance methods (`append`, `delete`, `get`, `has`, `set`, `entries`, `keys`, `values`, `forEach`) provide full HTTP header manipulation. Task 218 covers basic `Headers` construction and `.get()`, but the full method surface is missing.

## HIR Coverage

- `Expr::Call` with `Expr::Member` callee for all `Headers.*` methods
- `Expr::New` for `Headers` constructor
- `Expr::Call` for `Object.fromEntries(headers.entries())`

## Compile-Path Codegen

- `quote_codegen_exprs.inc` for expression evaluation
- Runtime API mapping in codegen or bridge globals

## Ink Example

```tsx
// examples/ink-headers-methods/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const headers = new Headers();
headers.set('Content-Type', 'application/json');
headers.append('X-Custom', 'value1');
headers.append('X-Custom', 'value2');

const hasCT = headers.has('Content-Type');
const ct = headers.get('Content-Type');
const custom = headers.get('X-Custom');
headers.delete('X-Custom');
const hasCustom = headers.has('X-Custom');

const entries: [string, string][] = [];
headers.forEach((value, key) => {
  entries.push([key, value]);
});

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Has CT: {hasCT ? 'yes' : 'no'}</Text>
      <Text>CT: {ct}</Text>
      <Text>Custom: {custom}</Text>
      <Text>Has Custom: {hasCustom ? 'yes' : 'no'}</Text>
      <Text>Entries: {entries.map(([k, v]) => `${k}=${v}`).join(', ')}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-headers-methods/`
- [ ] Uses `set`, `append`, `get`, `has`, `delete`, `forEach`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
