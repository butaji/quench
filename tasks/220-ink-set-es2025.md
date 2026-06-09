# Task 220: `ink-set-es2025` Example — `Set` Methods (intersection, union, difference, symmetricDifference, isSubsetOf, isSupersetOf, isDisjointFrom)

**Priority:** P3-Low
**Phase:** 19 — ES2025 Features
**Depends on:** 219

## Problem

New `Set` prototype methods (`intersection`, `union`, `difference`, `symmetricDifference`, `isSubsetOf`, `isSupersetOf`, `isDisjointFrom`) are part of ES2025. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-set-es2025/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const a = new Set([1, 2, 3]);
  const b = new Set([2, 3, 4]);

  // Use methods if available, otherwise compute manually
  const intersection = a.intersection ? Array.from(a.intersection(b)) : Array.from(a).filter(x => b.has(x));
  const union = a.union ? Array.from(a.union(b)) : Array.from(new Set([...Array.from(a), ...Array.from(b)]));
  const isSubset = a.isSubsetOf ? a.isSubsetOf(b) : false;

  return (
    <Box flexDirection="column">
      <Text>Intersection: {intersection.join(', ')}</Text>
      <Text>Union: {union.join(', ')}</Text>
      <Text>Is Subset: {String(isSubset)}</Text>
    </Box>
  );
}
```


## HIR Coverage

- `ClassMember` and `Class` variants

## Compile-Path Codegen

- `quote_codegen.rs` for class declaration codegen

## Acceptance Criteria

- [ ] Example exists at `examples/ink-set-es2025/`
- [ ] Uses `Set.prototype.intersection`, `union`, `isSubsetOf`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust for ES2025 Set methods
- [ ] Parity harness passes with 100% match in all 3 environments
