# Task 430: `ink-string-replace-function` Example — `String.prototype.replace` with Function Replacer

**Priority:** P1-High
**Phase:** 34 — HIR & Codegen Edge Cases
**Depends on:** 429

## Problem

`String.prototype.replace` with a function replacer (`str.replace(/re/g, (match) => match.toUpperCase())`) exercises callback-in-method-call patterns with capture groups. Tasks 363 and 195 cover string search/replace but not function replacers.

## HIR Coverage

- `Expr::Call` with `Expr::Member` callee for `str.replace()`
- `Expr::Arrow` or `Expr::Function` as replacer callback
- `Expr::New` for `RegExp` with global flag
- Template literal expressions using replacement results

## Compile-Path Codegen

- `quote_codegen_exprs.inc` for method call expressions
- Runtime API mapping for `String.prototype.replace` with callback

## Ink Example

```tsx
// examples/ink-string-replace-function/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const text = 'The quick brown fox jumps over the lazy dog';

const upper = text.replace(/\b\w/g, (match) => match.toUpperCase());
const numbered = text.replace(/\b(\w+)\b/g, (match, word, offset) => {
  return `[${offset}:${word}]`;
});
const vowels = text.replace(/[aeiou]/g, (match) => match.toUpperCase());

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Upper: {upper}</Text>
      <Text>Numbered: {numbered}</Text>
      <Text>Vowels: {vowels}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-string-replace-function/`
- [ ] Uses `replace` with function replacer and capture groups
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
