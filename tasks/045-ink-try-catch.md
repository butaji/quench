# Task 045: `ink-try-catch` Example — `try`, `catch`, `finally`, `throw`

**Priority:** P1-High  
**Phase:** 6 — Control Flow  
**Depends on:** 044

## Problem

Zero examples use `try/catch`/`throw`.

## Example

```tsx
import { Box, Text, useState } from 'ink';

export default function App() {
  const [log, setLog] = useState('Ready');

  function risky(n: number): string {
    if (n < 0) throw new Error('Negative');
    if (n > 100) throw new Error('Too large');
    return `OK: ${n}`;
  }

  function runTest(n: number) {
    try {
      const msg = risky(n);
      setLog(msg);
    } catch (e) {
      setLog(`Caught: ${(e as Error).message}`);
    } finally {
      setLog(prev => `${prev} (done)`);
    }
  }

  return (
    <Box flexDirection="column">
      <Text>{log}</Text>
      <Text>Tests: runTest(5), runTest(-1), runTest(200)</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists, renders identically in deno and `runts dev`
- [ ] `try`/`catch`/`finally` produces compilable Rust
- [ ] `throw` produces compilable Rust
- [ ] `runts build --release` produces working binary
- [ ] Parity harness 100%
