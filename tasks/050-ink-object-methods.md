# Task 050: `ink-object-methods` Example — Getters, Setters, Computed Keys, Method Shorthand

**Priority:** P2-Medium  
**Phase:** 6 — Data Structures  
**Depends on:** 049

## Problem

Zero examples use getters, setters, computed property keys, or method shorthand in object literals.

## Example

```tsx
import { Box, Text } from 'ink';

export default function App() {
  const prefix = 'get';
  const obj = {
    _value: 10,
    get value() { return this._value; },
    set value(v: number) { this._value = v; },
    double() { return this._value * 2; },
    [`${prefix}Label`]: 'computed',
  };

  return (
    <Box flexDirection="column">
      <Text>Value: {obj.value}</Text>
      <Text>Double: {obj.double()}</Text>
      <Text>Label: {obj.getLabel}</Text>
    </Box>
  );
}
```

## Work

`gen_object_expr` currently skips `Get`, `Set`, `Method` variants (comment says `/* Get, Set, Method - skip for now */`). Implement these in codegen.

## Acceptance Criteria

- [ ] Example exists, renders identically in deno and `runts dev`
- [ ] Getters and setters produce compilable Rust
- [ ] Computed property keys produce compilable Rust
- [ ] Method shorthand produces compilable Rust
- [ ] `runts build --release` produces working binary
- [ ] Parity harness 100%
