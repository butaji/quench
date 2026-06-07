# Task 048: `ink-object-advanced` Example — Getters, Setters, Computed Keys, Method Shorthand

**Priority:** P2-Medium  
**Phase:** 6 — Data Structures  
**Depends on:** 047

## Problem

Zero examples use getters, setters, computed property keys, or method shorthand. `gen_object_expr` skips `Get`, `Set`, `Method` variants.

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

Implement `Get`, `Set`, `Method` in `gen_object_expr` in `quote_codegen_exprs.inc`.

## Acceptance Criteria

- [ ] Example exists, renders identically in deno and `runts dev`
- [ ] Getters, setters, computed keys, method shorthand produce compilable Rust
- [ ] `runts build --release` produces working binary with 100% output match
