# Task 043: `ink-try-catch` Example — `try`, `catch`, `finally`, `throw`

**Priority:** P1-High  
**Phase:** 6 — Control Flow  
**Depends on:** 042

## Problem

Zero examples use `try/catch`/`throw`/`finally`.

## Example

```tsx
import { Box, Text, useState } from 'ink';

export default function App() {
  const [log, setLog] = useState<string[]>(['Ready']);

  function risky(n: number): string {
    if (n < 0) throw new Error('Negative');
    if (n > 100) throw new Error('Too large');
    return `OK: ${n}`;
  }

  function runTest(n: number) {
    try {
      const msg = risky(n);
      setLog(prev => [...prev, msg]);
    } catch (e) {
      setLog(prev => [...prev, `Caught: ${(e as Error).message}`]);
    } finally {
      setLog(prev => [...prev, 'done']);
    }
  }

  return (
    <Box flexDirection="column">
      {log.map((line, i) => <Text key={i}>{line}</Text>)}
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists, renders identically in deno and `runts dev`
- [ ] `try`/`catch`/`finally`/`throw` produce compilable Rust
- [ ] `runts build --release` produces working binary with 100% output match
