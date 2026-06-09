# Task 441: `ink-class-generic` Example — Generic Class Declarations with Usage

**Priority:** P1-High
**Phase:** 34 — HIR & Codegen Edge Cases
**Depends on:** 440

## Problem

Generic class declarations (`class Container<T> { ... }`) are a core TypeScript feature. Task 336 covers generic function components, but generic classes are not explicitly exercised. This tests HIR class declaration handling with type parameters.

## HIR Coverage

- `Stmt::Class` with generic type parameters
- `Expr::New` with type arguments (`new Container<string>()`)
- `Expr::Member` for generic class instance methods

## Compile-Path Codegen

- `quote_codegen.rs` for class declaration codegen
- Type parameters are erased; class is emitted without generics

## Ink Example

```tsx
// examples/ink-class-generic/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

class Container<T> {
  private value: T;

  constructor(value: T) {
    this.value = value;
  }

  get(): T {
    return this.value;
  }

  set(value: T): void {
    this.value = value;
  }
}

const numContainer = new Container<number>(42);
const strContainer = new Container<string>('hello');

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Num: {numContainer.get()}</Text>
      <Text>Str: {strContainer.get()}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-class-generic/`
- [ ] Uses generic class declaration with `new Container<T>()`
- [ ] HIR `Stmt::Class` with type parameters produces compilable Rust
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
