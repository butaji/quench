# Task 427: `ink-proxy-revoke` Example — `Proxy.revocable`

**Priority:** P2-Medium
**Phase:** 34 — HIR & Codegen Edge Cases
**Depends on:** 426

## Problem

`Proxy.revocable` creates a revocable Proxy that can be permanently disabled. No existing Ink example exercises `Proxy.revocable`.

## HIR Coverage

- `Expr::Call` with `Expr::Member` callee (`Proxy.revocable()`)
- `Expr::Object` for handler argument
- `Expr::Member` for `.proxy` and `.revoke` access on returned object

## Compile-Path Codegen

- `quote_codegen_exprs.inc` for expression evaluation
- Runtime API mapping in codegen or bridge globals

## Ink Example

```tsx
// examples/ink-proxy-revoke/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const target = { value: 42 };
const { proxy, revoke } = Proxy.revocable(target, {
  get(t, prop) {
    return t[prop as keyof typeof t];
  },
});

const before = proxy.value;
revoke();
let after: string;
try {
  after = String(proxy.value);
} catch {
  after = 'revoked';
}

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Before: {before}</Text>
      <Text>After: {after}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-proxy-revoke/`
- [ ] Uses `Proxy.revocable`, `.proxy`, `.revoke`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
