# Task 042: `ink-control-flow` Example — `for`, `while`, `do-while`, `switch`, `break`, `continue`

**Priority:** P1-High  
**Phase:** 6 — Control Flow  
**Depends on:** 041

## Problem

Zero examples exercise `for`, `while`, `do-while`, or labeled `break`/`continue`. Only 1 example uses `switch`.

## Example

Create `examples/ink-control-flow/`:

```tsx
import { Box, Text, useInput, useState } from 'ink';

export default function App() {
  const [mode, setMode] = useState<'for' | 'while' | 'dowhile' | 'switch'>('for');
  const [count, setCount] = useState(3);
  const lines: string[] = [];

  if (mode === 'for') {
    for (let i = 1; i <= count; i++) {
      if (i === 2) continue;
      if (i === 5) break;
      lines.push(`for line ${i}`);
    }
  } else if (mode === 'while') {
    let w = 1;
    while (w <= count) {
      lines.push(`while line ${w}`);
      w++;
    }
  } else if (mode === 'dowhile') {
    let d = 1;
    do {
      lines.push(`do-while line ${d}`);
      d++;
    } while (d <= count);
  } else {
    for (let i = 0; i <= count; i++) {
      switch (i) {
        case 0: lines.push('zero'); break;
        case 1: lines.push('one'); break;
        case 2: lines.push('two'); break;
        default: lines.push(`many ${i}`); break;
      }
    }
  }

  useInput((input) => {
    if (input === 'q') process.exit(0);
    if (input === 'f') setMode('for');
    if (input === 'w') setMode('while');
    if (input === 'd') setMode('dowhile');
    if (input === 's') setMode('switch');
  });

  return (
    <Box flexDirection="column">
      <Text>Mode: {mode} (f/w/d/s to switch, q to quit)</Text>
      {lines.map((l, idx) => <Text key={idx}>{l}</Text>)}
    </Box>
  );
}
```

## Work

1. Create example files (`tui/app.tsx`, `main.tsx`, `deno.json`, `runts.config.json`)
2. Verify `deno run -A main.tsx` renders correctly
3. Verify `runts dev --once --plugin ratatui` matches deno output exactly
4. Verify `runts build --release --plugin ratatui` produces working binary
5. If codegen fails, fix `gen_for`, `gen_while`, `gen_do_while`, `gen_switch` in `quote_codegen_stmts.inc`

## Acceptance Criteria

- [ ] Example exists and renders identically in deno and `runts dev`
- [ ] `for`, `while`, `do-while`, `switch`, `break`, `continue` all produce compilable Rust
- [ ] Labeled `break`/`continue` produce compilable Rust
- [ ] `runts build --release` produces working binary with output matching deno exactly (100%)
