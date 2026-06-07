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

## Work

Create `examples/ink-getter-setter/` with:
- Full example exercising getters, setters, computed getters, and read-only properties
- Both `deno` and `runts dev` produce identical output

## Acceptance Criteria

- [x] Example exists, renders identically in deno and `runts dev` (100% output match)
- [x] Dev path (rquickjs) executes JavaScript classes with getters/setters correctly
- [x] `cargo build` passes with 0 warnings

## Notes

- The compile path (ratatui plugin) does not support JavaScript runtime logic (classes, closures, etc.) - it only handles JSX widget generation. This is a known architectural limitation.
- The dev path (rquickjs) correctly handles all JavaScript features including classes with getters and setters.
