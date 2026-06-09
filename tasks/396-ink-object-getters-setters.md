# Task 396: `ink-object-getters-setters` Example — Getter and Setter Properties

**Priority:** P1-High
**Phase:** 32 — Core Language + React + Runtime Edge Cases
**Depends on:** 395

## Problem

Object getter/setter properties (`get foo() {}`, `set foo(v) {}`) are a core JavaScript feature. No existing Ink example explicitly exercises this pattern.

## HIR Coverage

- `Property::Get` and `Property::Set` in object literals.
- The parser must capture getter and setter definitions.

## Compile-Path Codegen

- `quote_codegen_exprs.inc` must emit compilable Rust for getter/setter properties.
- Getters map to computed property access, setters to assignment with side effects.

## Ink Example

```tsx
// examples/ink-object-getters-setters/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const counter = {
  _value: 0,
  get value() {
    return this._value;
  },
  set value(v: number) {
    this._value = v;
  },
  get doubled() {
    return this._value * 2;
  }
};

counter.value = 5;

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Value: {counter.value}</Text>
      <Text>Doubled: {counter.doubled}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-object-getters-setters/`
- [ ] Uses `get` and `set` properties in object literals
- [ ] HIR `Property::Get` and `Property::Set` produce compilable Rust
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
