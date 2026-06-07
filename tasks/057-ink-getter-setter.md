# Task 057: `ink-getter-setter` Example — Getters, Setters, Computed Accessors

**Priority:** P2-Medium  
**Phase:** 6 — Classes & OOP  
**Depends on:** 056

## Problem

Zero examples use getters or setters in classes.

## Example

```tsx
import { Box, Text } from 'ink';

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

## Acceptance Criteria

- [ ] Example exists, renders identically in deno and `runts dev`
- [ ] Getters and setters produce compilable Rust
- [ ] `runts build --release` produces working binary with 100% output match
