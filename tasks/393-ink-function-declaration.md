# Task 393: `ink-function-declaration` Example — Named Function Declarations

**Priority:** P1-High
**Phase:** 32 — Core Language + React + Runtime Edge Cases
**Depends on:** 392

## Problem

Named function declarations (`function foo() {}`) differ from function expressions in hoisting behavior and HIR representation. No existing Ink example explicitly exercises standalone named function declarations.

## HIR Coverage

- `Stmt::FunctionDecl` with a non-empty name.
- Hoisting semantics (function available before declaration in source).

## Compile-Path Codegen

- `quote_codegen_stmts.inc` must emit compilable Rust for named function declarations.
- Function hoisting must be handled in generated code order.

## Ink Example

```tsx
// examples/ink-function-declaration/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

function greet(name: string): string {
  return `Hello, ${name}`;
}

function add(a: number, b: number): number {
  return a + b;
}

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>{greet('World')}</Text>
      <Text>Sum: {add(3, 4)}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-function-declaration/`
- [ ] Uses named `function` declarations
- [ ] HIR `Stmt::FunctionDecl` produces compilable Rust
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
