# Task 395: `ink-shorthand-properties` Example — Object Shorthand `{a}` and Method Shorthand

**Priority:** P1-High
**Phase:** 32 — Core Language + React + Runtime Edge Cases
**Depends on:** 394

## Problem

Object shorthand (`{a}` instead of `{a: a}`) and method shorthand (`{foo() {}}` instead of `{foo: function() {}}`) are common JavaScript patterns. No existing Ink example explicitly exercises these syntaxes.

## HIR Coverage

- `Property::Shorthand` for `{a}` syntax.
- `Property::Method` for `{foo() {}}` syntax.
- Both variants must not produce `Expr::Invalid`.

## Compile-Path Codegen

- `quote_codegen_exprs.inc` must emit compilable Rust for shorthand properties.
- Method shorthand must map to function values in object literals.

## Ink Example

```tsx
// examples/ink-shorthand-properties/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const name = 'Alice';
const age = 30;

const person = {
  name,
  age,
  greet() {
    return `Hello, I'm ${this.name}`;
  },
  doubleAge() {
    return this.age * 2;
  }
};

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Name: {person.name}</Text>
      <Text>Age: {person.age}</Text>
      <Text>{person.greet()}</Text>
      <Text>Double: {person.doubleAge()}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-shorthand-properties/`
- [ ] Uses object shorthand `{name}` and method shorthand `{greet() {}}`
- [ ] HIR `Property::Shorthand` and `Property::Method` produce compilable Rust
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
