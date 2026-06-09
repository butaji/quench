# Task 218: `ink-web-api` Example — Web APIs (`URL`, `AbortController`, `TextEncoder`, `Blob`, `FormData`, `Headers`)

**Priority:** P2-Medium
**Phase:** 19 — Runtime API Deep Coverage
**Depends on:** 217

## Problem

Common Web APIs (`URL`, `AbortController`, `TextEncoder`/`TextDecoder`, `Blob`, `FormData`, `Headers`) are available in modern JavaScript runtimes but not covered by any existing task.

## Ink Example

```tsx
// examples/ink-web-api/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const url = new URL('https://example.com/path?name=value');
  const encoder = new TextEncoder();
  const encoded = encoder.encode('hello');
  const headers = new Headers({ 'Content-Type': 'application/json' });

  return (
    <Box flexDirection="column">
      <Text>Hostname: {url.hostname}</Text>
      <Text>Pathname: {url.pathname}</Text>
      <Text>Search: {url.search}</Text>
      <Text>Encoded length: {encoded.length}</Text>
      <Text>Content-Type: {headers.get('Content-Type')}</Text>
    </Box>
  );
}
```


## HIR Coverage

- Standard `Expr::Call` + `Expr::Member` for runtime API access

## Compile-Path Codegen

- Runtime API mapping in codegen or bridge globals

## Acceptance Criteria

- [ ] Example exists at `examples/ink-web-api/`
- [ ] Uses `URL`, `TextEncoder`, `Headers`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust for Web API constructors
- [ ] Parity harness passes with 100% match in all 3 environments
