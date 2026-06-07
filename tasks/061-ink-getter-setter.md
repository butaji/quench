# Task 061: `ink-getter-setter` Example — Getters, Setters, Computed Accessors

**Priority:** P2-Medium  
**Phase:** 6 — Classes & OOP  
**Depends on:** 060

## Problem

Zero examples use getters or setters in classes or objects.

## Example

```tsx
import { Box, Text, useState } from 'ink';

class Counter {
  private _value = 0;

  get value() { return this._value; }
  set value(v: number) { this._value = v; }
  get doubled() { return this._value * 2; }
}

export default function App() {
  const c = new Counter();
  c.value = 5;

  return (
    <Box flexDirection="column">
      <Text>Value: {c.value}</Text>
      <Text>Doubled: {c.doubled}</Text>
    </Box>
  );
}
```

## Work

- Getters → Rust methods with `fn field(&self) -> Type`
- Setters → Rust methods with `fn set_field(&mut self, value: Type)`
- Computed accessors → same pattern

## Acceptance Criteria

- [ ] Example exists, renders identically in deno and `runts dev`
- [ ] Getters and setters produce compilable Rust
- [ ] `runts build --release` produces working binary
- [ ] Parity harness 100%
