# Task 054: `ink-compound-calc` Example — All Compound Assignment Operators

**Priority:** P1-High  
**Phase:** 6 — Expressions & Operators  
**Depends on:** 053

## Problem

Zero examples use compound assignment (`|=`, `&=`, `^=`, `<<=`, `>>=`, `>>>=`, `**=`).

## Example

```tsx
import { Box, Text, useState } from 'ink';

export default function App() {
  const [flags, setFlags] = useState(0b0000);

  function toggleBit(pos: number) { setFlags(f => f ^ (1 << pos)); }
  function setBit(pos: number) { setFlags(f => f | (1 << pos)); }
  function clearBit(pos: number) { setFlags(f => f & ~(1 << pos)); }

  const bits = [];
  for (let i = 0; i < 4; i++) { bits.push((flags >> i) & 1); }

  return (
    <Box flexDirection="column">
      <Text>Flags: {flags.toString(2).padStart(4, '0')}</Text>
      {bits.map((b, i) => <Text key={i}>Bit {i}: {b ? 'ON' : 'OFF'}</Text>)}
    </Box>
  );
}
```

## Work

Add missing arms in `gen_assign_expr`:
```rust
A::ExpAssign    => pow
A::BitOrAssign  => |
A::BitAndAssign => &
A::BitXorAssign => ^
A::ShlAssign    => <<
A::ShrAssign    => >>
A::UShrAssign   => >>> (unsigned)
```

## Acceptance Criteria

- [ ] Example exists, renders identically in deno and `runts dev`
- [ ] All 13 assignment operators (`=` + 12 compound) produce compilable Rust
- [ ] `runts build --release` produces working binary
- [ ] Parity harness 100%
