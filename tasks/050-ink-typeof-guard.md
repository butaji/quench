# Task 050: `ink-typeof-guard` Example — `typeof`, `instanceof`, `delete`, `void`

**Priority:** P1-High
**Phase:** 6 — Expressions & Operators
**Depends on:** 049

## Problem

Zero examples use `typeof`, `instanceof`, or `void`. Only 1 uses `delete`.

## Example

```tsx
import { Box, Text } from 'ink';

export default function App({ value }: { value: unknown }) {
  let label: string;
  if (typeof value === 'string') {
    label = `String: ${value.toUpperCase()}`;
  } else if (typeof value === 'number') {
    label = `Number: ${value * 2}`;
  } else if (value instanceof Date) {
    label = `Date: ${value.toISOString()}`;
  } else {
    label = `Other: ${String(value)}`;
  }

  const obj: any = { a: 1 };
  delete obj.a;
  const unused = void 0;

  return (
    <Box flexDirection="column">
      <Text>{label}</Text>
      <Text>Deleted: {obj.a === undefined ? 'yes' : 'no'}</Text>
    </Box>
  );
}
```

## Work

Add constant folding for `typeof`:
- `typeof "str"` → `"string"`
- `typeof 42` → `"number"`
- `typeof true` → `"boolean"`
- `typeof undefined` → `"undefined"`
- `typeof null` → `"object"`

## Acceptance Criteria

- [ ] Example exists, renders identically in deno and `runts dev`
- [ ] `typeof` on literals constant-folds to type string
- [ ] `typeof` on non-literals produces compilable Rust
- [ ] `instanceof`, `delete`, `void` produce compilable Rust
- [ ] `runts build --release` produces working binary with 100% output match
