# Task 426: `ink-date-parse-setters` Example — `Date.parse`, `Date.UTC`, `setTime`, `setFullYear`, `setMonth`, `setDate`, `setHours`

**Priority:** P2-Medium
**Phase:** 33 — HIR & Codegen Edge Cases
**Depends on:** 425

## Problem

`Date` static parsing methods (`parse`, `UTC`) and setter methods (`setTime`, `setFullYear`, `setMonth`, `setDate`, `setHours`) are not explicitly exercised. Task 174 covers Date getters and Task 366 covers locale formatting, but parsing and mutation are missing.

## HIR Coverage

- `Expr::Call` for `Date.parse()`, `Date.UTC()`
- `Expr::Call` for `date.setFullYear()`, `date.setMonth()`, etc.
- `Expr::New` for `Date` constructor

## Compile-Path Codegen

- `quote_codegen_exprs.inc` for expression evaluation
- Runtime API mapping in codegen or bridge globals

## Ink Example

```tsx
// examples/ink-date-parse-setters/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const parsed = Date.parse('2024-06-15T10:30:00Z');
const utc = Date.UTC(2024, 5, 15, 10, 30, 0);

const d = new Date();
d.setTime(utc);
d.setFullYear(2025);
d.setMonth(0);
d.setDate(1);
d.setHours(12);

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Parsed: {parsed}</Text>
      <Text>UTC: {utc}</Text>
      <Text>Result: {d.toISOString()}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-date-parse-setters/`
- [ ] Uses `Date.parse`, `Date.UTC`, `setTime`, `setFullYear`, `setMonth`, `setDate`, `setHours`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
