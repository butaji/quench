# Task 272: `ink-intl-list` Example — `Intl.ListFormat`, `Intl.RelativeTimeFormat`, `Intl.PluralRules`, `Intl.Collator`

**Priority:** P3-Low
**Phase:** 23 — Runtime API Completion
**Depends on:** 271

## Problem

Additional `Intl` formatters (`ListFormat`, `RelativeTimeFormat`, `PluralRules`, `Collator`) are not covered by existing tasks 227–228.

## Ink Example

```tsx
// examples/ink-intl-list/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const items = ['apple', 'banana', 'cherry'];
  const list = new Intl.ListFormat('en', { style: 'long', type: 'conjunction' }).format(items);
  const relative = new Intl.RelativeTimeFormat('en').format(-1, 'day');
  const plural = new Intl.PluralRules('en').select(2);
  const collator = new Intl.Collator('en').compare('a', 'b');

  return (
    <Box flexDirection="column">
      <Text>List: {list}</Text>
      <Text>Relative: {relative}</Text>
      <Text>Plural: {plural}</Text>
      <Text>Collator: {collator}</Text>
    </Box>
  );
}
```


## HIR Coverage

- Standard `Expr::Call` + `Expr::Member` for runtime API access

## Compile-Path Codegen

- Runtime API mapping in codegen or bridge globals

## Acceptance Criteria

- [ ] Example exists at `examples/ink-intl-list/`
- [ ] Uses `Intl.ListFormat`, `RelativeTimeFormat`, `PluralRules`, `Collator`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust for Intl constructors
- [ ] Parity harness passes with 100% match in all 3 environments
