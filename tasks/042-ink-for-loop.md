# Task 042: `ink-for-loop` Example — `for`, `break`, `continue`, `++`/`--`

**Priority:** P1-High  
**Phase:** 6 — Control Flow  
**Depends on:** 041

## Problem

Zero examples use `for` loops, `break`, `continue`, or `++`/`--`. Compile-path codegen for `for` exists (`gen_for`) but lacks end-to-end validation with a real Ink example.

## Example

Create `examples/ink-for-loop/`:

```tsx
import { Box, Text, useInput, useState } from 'ink';

export default function App() {
  const [count, setCount] = useState(5);
  const rows: string[] = [];

  for (let i = 1; i <= count; i++) {
    if (i === 3) continue;
    if (i === 5) break;
    rows.push(`Row ${i}`);
  }

  useInput((input) => {
    if (input === 'q') process.exit(0);
    if (input === '+') setCount(c => c + 1);
    if (input === '-') setCount(c => Math.max(0, c - 1));
  });

  return (
    <Box flexDirection="column">
      <Text>Count: {count} (+/- to change, q to quit)</Text>
      {rows.map((r, idx) => <Text key={idx}>{r}</Text>)}
    </Box>
  );
}
```

## Work

1. Create example files (`tui/app.tsx`, `main.tsx`, `deno.json`, `runts.config.json`)
2. Verify `deno run -A main.tsx` renders correctly
3. Verify `runts dev --once --plugin ratatui` matches deno
4. Verify `runts build --release --plugin ratatui` produces working binary
5. If codegen fails, fix `gen_for` in `quote_codegen_stmts.inc`

## Acceptance Criteria

- [ ] Example exists and renders identically in deno and `runts dev`
- [ ] `for` loop with `break` and `continue` produces compilable Rust
- [ ] `++`/`--` operators produce compilable Rust
- [ ] `runts build --release` produces working binary
- [ ] Parity harness 100% across all 3 environments
