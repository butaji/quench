# Task 417: `ink-crypto-web` Example — `crypto.getRandomValues`, `crypto.subtle`

**Priority:** P2-Medium
**Phase:** 33 — HIR & Codegen Edge Cases
**Depends on:** 416

## Problem

Web Crypto API methods (`crypto.getRandomValues`, `crypto.subtle.digest`) are not explicitly exercised. Task 248 covers `crypto.randomUUID` and Task 316 covers Node.js `crypto`, but the standard Web Crypto API is missing.

## HIR Coverage

- `Expr::Call` with chained member access (`crypto.getRandomValues`, `crypto.subtle.digest`)
- `Expr::New` for `Uint8Array`
- `Expr::Await` for async subtle crypto operations

## Compile-Path Codegen

- `quote_codegen_exprs.inc` for expression evaluation
- Runtime API mapping in codegen or bridge globals

## Ink Example

```tsx
// examples/ink-crypto-web/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const array = new Uint8Array(4);
crypto.getRandomValues(array);
const randomHex = Array.from(array).map((b) => b.toString(16).padStart(2, '0')).join('');

async function digest(): Promise<string> {
  const encoder = new TextEncoder();
  const data = encoder.encode('hello');
  const hash = await crypto.subtle.digest('SHA-256', data);
  return Array.from(new Uint8Array(hash)).map((b) => b.toString(16).padStart(2, '0')).join('');
}

export default function App() {
  const [hash, setHash] = React.useState('computing...');
  React.useEffect(() => {
    digest().then(setHash);
  }, []);

  return (
    <Box flexDirection="column">
      <Text>Random: {randomHex}</Text>
      <Text>Hash: {hash}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-crypto-web/`
- [ ] Uses `crypto.getRandomValues` and `crypto.subtle.digest`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
