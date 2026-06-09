# Task 394: `ink-block-scope` Example — `let` vs `var` vs `const` Scoping

**Priority:** P1-High
**Phase:** 32 — Core Language + React + Runtime Edge Cases
**Depends on:** 393

## Problem

Block scoping (`let`/`const`) vs function scoping (`var`) is a fundamental JavaScript feature. No existing Ink example explicitly exercises the differences between `let`, `const`, and `var`.

## HIR Coverage

- `Stmt::Let`, `Stmt::Const`, `Stmt::Var` with distinct scoping semantics.
- Block-scoped variables in nested blocks.
- `const` reassignment attempts (should error or be handled).

## Compile-Path Codegen

- `quote_codegen_stmts.inc` must emit compilable Rust respecting block scoping.
- `let` and `const` map to Rust `let` bindings in the appropriate scope.
- `var` hoisting must be handled.

## Ink Example

```tsx
// examples/ink-block-scope/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

function scopeDemo(): string[] {
  const results: string[] = [];
  
  if (true) {
    let x = 'block';
    var y = 'function';
    results.push(x);
    results.push(y);
  }
  
  results.push(y);
  
  const arr = [1, 2, 3];
  arr.forEach((n) => {
    const doubled = n * 2;
    results.push(String(doubled));
  });
  
  return results;
}

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>{scopeDemo().join(', ')}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-block-scope/`
- [ ] Uses `let`, `const`, and `var` with block scoping
- [ ] HIR `Let`/`Const`/`Var` produce compilable Rust
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
