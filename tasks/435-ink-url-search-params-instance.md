# Task 435: `ink-url-search-params-instance` Example — `URL.searchParams` Instance Property

**Priority:** P2-Medium
**Phase:** 34 — HIR & Codegen Edge Cases
**Depends on:** 434

## Problem

The `URL.searchParams` instance property returns a live `URLSearchParams` object bound to the URL. Task 218 covers `URL` construction and Task 246 covers standalone `URLSearchParams`, but the `url.searchParams` instance property with live mutation is not explicitly exercised.

## HIR Coverage

- `Expr::Member` for `url.searchParams`
- `Expr::Call` for `searchParams.set()`, `searchParams.get()`, `searchParams.toString()`
- `Expr::Member` for `url.href` after mutation

## Compile-Path Codegen

- `quote_codegen_exprs.inc` for expression evaluation
- Runtime API mapping in codegen or bridge globals

## Ink Example

```tsx
// examples/ink-url-search-params-instance/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const url = new URL('https://example.com/?name=Alice');
url.searchParams.set('age', '30');
url.searchParams.append('city', 'NYC');
url.searchParams.delete('name');

const hasAge = url.searchParams.has('age');
const age = url.searchParams.get('age');
const query = url.searchParams.toString();
const href = url.href;

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Has Age: {hasAge ? 'yes' : 'no'}</Text>
      <Text>Age: {age}</Text>
      <Text>Query: {query}</Text>
      <Text>Href: {href}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-url-search-params-instance/`
- [ ] Uses `URL.searchParams` instance property with live mutation
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
