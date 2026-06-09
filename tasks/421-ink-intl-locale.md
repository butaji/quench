# Task 421: `ink-intl-locale` Example — `Intl.Segmenter`, `Intl.DisplayNames`, `Intl.Locale`

**Priority:** P2-Medium
**Phase:** 33 — HIR & Codegen Edge Cases
**Depends on:** 420

## Problem

Additional `Intl` constructors (`Segmenter`, `DisplayNames`, `Locale`) are not explicitly exercised. Tasks 118, 227, 228, and 272 cover `DateTimeFormat`, `NumberFormat`, `ListFormat`, `RelativeTimeFormat`, `PluralRules`, and `Collator`, but these three Intl APIs are missing.

## HIR Coverage

- `Expr::New` for `Intl.Segmenter`, `Intl.DisplayNames`, `Intl.Locale`
- `Expr::Call` for `segment()`, `of()`, `maximize()` methods

## Compile-Path Codegen

- `quote_codegen_exprs.inc` for expression evaluation
- Runtime API mapping in codegen or bridge globals

## Ink Example

```tsx
// examples/ink-intl-locale/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const segmenter = new Intl.Segmenter('en', { granularity: 'word' });
const segments = Array.from(segmenter.segment('Hello world'));
const words = segments.map((s) => s.segment);

const displayNames = new Intl.DisplayNames(['en'], { type: 'language' });
const langName = displayNames.of('ja');

const locale = new Intl.Locale('en-US');
const maximized = locale.maximize().toString();

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Words: {words.join(', ')}</Text>
      <Text>Lang: {langName}</Text>
      <Text>Locale: {maximized}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-intl-locale/`
- [ ] Uses `Intl.Segmenter`, `Intl.DisplayNames`, `Intl.Locale`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
