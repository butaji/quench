# Task 067: `ink-type-erasure` Example — Generics, Mapped Types, Conditional Types

**Priority:** P3-Low  
**Phase:** 6 — TypeScript Types  
**Depends on:** 066

## Problem

Type-level-only features (generics, mapped types, conditional types, utility types) are erased at runtime but need to be parsed without errors.

## Example

```tsx
import { Box, Text } from 'ink';

type Nullable<T> = T | null;
type Keys<T> = keyof T;
type PickString<T> = T extends string ? T : never;

function identity<T>(x: T): T { return x; }

export default function App() {
  const val = identity<string>('hello');
  const n: Nullable<number> = null;

  return (
    <Box>
      <Text>{val}</Text>
    </Box>
  );
}
```

## Work

Ensure parser handles type-only constructs without producing `Expr::Invalid`. These are erased in codegen.

## Acceptance Criteria

- [ ] Example exists, renders identically in deno and `runts dev`
- [ ] Generic type parameters are erased (no runtime impact)
- [ ] Mapped types and conditional types are erased
- [ ] `runts build --release` produces working binary with 100% output match
