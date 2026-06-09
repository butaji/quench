# Task 363: `ink-string-match-search` Example — `String.prototype.match`, `search`, `replace`

**Priority:** P1-High
**Phase:** 29 — String Methods Completion
**Depends on:** 362

## Problem

`String.prototype.match`, `search`, and `replace` are core string methods that interact with RegExp. No dedicated Ink example exercises all three.

## Ink Example

```tsx
// examples/ink-string-match-search/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const text = 'The quick brown fox';
  const match = text.match(/quick/);
  const search = text.search(/brown/);
  const replace = text.replace('fox', 'dog');

  return (
    <Box flexDirection="column">
      <Text>Match: {match ? match[0] : 'none'}</Text>
      <Text>Search index: {search}</Text>
      <Text>Replace: {replace}</Text>
    </Box>
  );
}
```


## HIR Coverage

- Standard `Expr::Call` + `Expr::Member` for runtime API access

## Compile-Path Codegen

- Runtime API mapping in codegen or bridge globals

## Acceptance Criteria

- [ ] Example exists at `examples/ink-string-match-search/`
- [ ] Uses `match`, `search`, `replace`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
