# Task 409: `ink-string-fundamental` Example — `indexOf`, `lastIndexOf`, `slice`, `split`, `substring`, `toLowerCase`, `toUpperCase`, `trim`, `matchAll`

**Priority:** P1-High
**Phase:** 33 — HIR & Codegen Edge Cases
**Depends on:** 408

## Problem

Fundamental `String.prototype` methods (`indexOf`, `lastIndexOf`, `slice`, `split`, `substring`, `toLowerCase`, `toUpperCase`, `trim`, `matchAll`) are essential string manipulation operations. While Tasks 113, 147, 171, 195, and 363 cover some string methods, the core search/slice/transform methods are not explicitly exercised by a dedicated Ink example.

## HIR Coverage

- `Expr::Call` with `Expr::Member` callee for String prototype methods
- `Expr::New` for `RegExp` used with `matchAll`
- Template literal expressions using processed string results

## Compile-Path Codegen

- `quote_codegen_exprs.inc` for method call expressions
- Runtime API mapping for `String.prototype` methods in codegen or bridge globals

## Ink Example

```tsx
// examples/ink-string-fundamental/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const text = '  Hello, TypeScript World!  ';
const idx = text.indexOf('Type');
const lastIdx = text.lastIndexOf(' ');
const sliced = text.slice(2, 7);
const split = text.trim().split(' ');
const sub = text.substring(9, 19);
const lower = text.toLowerCase();
const upper = text.toUpperCase();
const trimmed = text.trim();

const matches: string[] = [];
for (const m of text.matchAll(/[A-Z][a-z]+/g)) {
  matches.push(m[0]);
}

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Index: {idx}</Text>
      <Text>LastIndex: {lastIdx}</Text>
      <Text>Sliced: {sliced}</Text>
      <Text>Split: {split.join('|')}</Text>
      <Text>Substring: {sub}</Text>
      <Text>Lower: {lower}</Text>
      <Text>Upper: {upper}</Text>
      <Text>Trimmed: {trimmed}</Text>
      <Text>Matches: {matches.join(', ')}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-string-fundamental/`
- [ ] Uses `indexOf`, `lastIndexOf`, `slice`, `split`, `substring`, `toLowerCase`, `toUpperCase`, `trim`, `matchAll`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
