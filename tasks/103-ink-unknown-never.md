# Task 103: `ink-unknown-never` Example — `unknown`, `never`, User-Defined Type Guards

**Priority:** P1-High
**Phase:** 11 — Type System Deep Coverage
**Depends on:** 078

## Problem

`unknown` (safe alternative to `any`), `never` (uninhabitable type), and user-defined type guards (`x is Type`) are advanced but essential TypeScript patterns. No existing Ink example exercises all three.

## Ink Example

```tsx
// examples/ink-unknown-never/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

function isString(x: unknown): x is string {
  return typeof x === 'string';
}

function isNumber(x: unknown): x is number {
  return typeof x === 'number';
}

function formatValue(x: unknown): string {
  if (isString(x)) {
    return `str: ${x.toUpperCase()}`;
  }
  if (isNumber(x)) {
    return `num: ${x.toFixed(2)}`;
  }
  return 'unknown';
}

type Status = 'idle' | 'loading' | 'success';
function assertNever(x: never): never {
  throw new Error(`Unexpected: ${x}`);
}

function getStatusLabel(s: Status): string {
  switch (s) {
    case 'idle': return 'Idle';
    case 'loading': return 'Loading...';
    case 'success': return 'Done!';
    default: return assertNever(s);
  }
}

const values: unknown[] = ['hello', 42, true];

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>{getStatusLabel('success')}</Text>
      <Text>{values.map(formatValue).join(' | ')}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-unknown-never/`
- [ ] Uses `unknown` type for safe values
- [ ] Uses user-defined type guard `x is Type`
- [ ] Uses `never` for exhaustive switch
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path erases `unknown`/`never`/type guards without runtime impact
- [ ] Parity harness passes with 100% match in all 3 environments
