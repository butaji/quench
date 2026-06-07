# Task 055: `ink-bitwise-calc` Example — Bitwise Operators

**Priority:** P2-Medium  
**Phase:** 6 — Expressions & Operators  
**Depends on:** 054

## Problem

Zero examples use bitwise operators (`&`, `|`, `^`, `<<`, `>>`, `>>>`).

## Example

```tsx
import { Box, Text } from 'ink';

export default function App() {
  const a = 0b1100;
  const b = 0b1010;

  return (
    <Box flexDirection="column">
      <Text>AND:  {(a & b).toString(2)} = {(a & b)}</Text>
      <Text>OR:   {(a | b).toString(2)} = {(a | b)}</Text>
      <Text>XOR:  {(a ^ b).toString(2)} = {(a ^ b)}</Text>
      <Text>SHL:  {(a << 1).toString(2)} = {(a << 1)}</Text>
      <Text>SHR:  {(a >> 1).toString(2)} = {(a >> 1)}</Text>
      <Text>USHR: {(a >>> 1).toString(2)} = {(a >>> 1)}</Text>
      <Text>NOT:  {(~a).toString(2)} = {~a}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists, renders identically in deno and `runts dev`
- [ ] All bitwise operators produce compilable Rust
- [ ] `runts build --release` produces working binary
- [ ] Parity harness 100%
