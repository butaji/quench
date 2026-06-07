# Task 051: `ink-compound-bitwise` Example — Compound Assignment + Bitwise Operators

**Priority:** P1-High
**Phase:** 6 — Expressions & Operators
**Depends on:** 050

## Problem

Zero examples use compound assignment (`|=`, `&=`, `^=`, `<<=`, `>>=`, `>>>=`, `**=`) or bitwise operators (`&`, `|`, `^`, `<<`, `>>`, `>>>`, `~`).

## Example

```tsx
import { Box, Text, useState } from 'ink';

export default function App() {
  const [flags, setFlags] = useState(0b0000);

  function toggleBit(pos: number) { setFlags(f => f ^ (1 << pos)); }
  function setBit(pos: number) { setFlags(f => f | (1 << pos)); }
  function clearBit(pos: number) { setFlags(f => f & ~(1 << pos)); }

  const a = 0b1100;
  const b = 0b1010;
  const bits = [];
  for (let i = 0; i < 4; i++) { bits.push((flags >> i) & 1); }

  return (
    <Box flexDirection="column">
      <Text>Flags: {flags.toString(2).padStart(4, '0')}</Text>
      <Text>AND: {(a & b).toString(2)}</Text>
      <Text>OR: {(a | b).toString(2)}</Text>
      <Text>XOR: {(a ^ b).toString(2)}</Text>
      <Text>NOT: {(~a).toString(2)}</Text>
      {bits.map((b, i) => <Text key={i}>Bit {i}: {b ? 'ON' : 'OFF'}</Text>)}
    </Box>
  );
}
```

## Work

Add missing arms in `gen_assign_expr` and `gen_bin_expr` for:
- `ExpAssign`, `BitOrAssign`, `BitAndAssign`, `BitXorAssign`
- `ShlAssign`, `ShrAssign`, `UShrAssign`
- `BitXor`, `BitAnd`, `BitOr`, `Shl`, `Shr`, `UShr` in `bin_logical`

## Acceptance Criteria

- [ ] Example exists, renders identically in deno and `runts dev`
- [ ] All 13 assignment operators (`=` + 12 compound) produce compilable Rust
- [ ] All bitwise operators produce compilable Rust
- [ ] `runts build --release` produces working binary with 100% output match
