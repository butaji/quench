# Task 431: `ink-intl-duration` Example — `Intl.DurationFormat`

**Priority:** P2-Medium
**Phase:** 34 — HIR & Codegen Edge Cases
**Depends on:** 430

## Problem

`Intl.DurationFormat` (ECMAScript 402 proposal, available in some modern runtimes) formats durations in a locale-aware way. Tasks 227–228 and 272 cover other Intl formatters, and Task 421 covers Segmenter/DisplayNames/Locale, but DurationFormat is missing.

## HIR Coverage

- `Expr::New` for `Intl.DurationFormat`
- `Expr::Call` for `.format()` method
- `Expr::Object` for duration input `{ years, months, days, hours, minutes, seconds }`

## Compile-Path Codegen

- `quote_codegen_exprs.inc` for expression evaluation
- Runtime API mapping in codegen or bridge globals

## Ink Example

```tsx
// examples/ink-intl-duration/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const df = new Intl.DurationFormat('en', { style: 'long' });
const shortDf = new Intl.DurationFormat('de', { style: 'short' });

const duration = { hours: 1, minutes: 30, seconds: 45 };
const formatted = df.format(duration);
const shortFormatted = shortDf.format(duration);

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Long: {formatted}</Text>
      <Text>Short: {shortFormatted}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-intl-duration/`
- [ ] Uses `Intl.DurationFormat` with `.format()`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
