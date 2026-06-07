# Task 052: `ink-async-fetch` Example — `async`, `await`, Promise

**Priority:** P1-High
**Phase:** 6 — Functions & Async
**Depends on:** 051

## Problem

Zero examples use `async/await`.

## Example

```tsx
import { Box, Text, useState, useEffect } from 'ink';

function mockFetch(ms: number): Promise<string> {
  return new Promise(resolve => setTimeout(() => resolve(`Loaded after ${ms}ms`), ms));
}

export default function App() {
  const [data, setData] = useState('Loading...');

  useEffect(() => {
    async function fetchData() {
      try {
        const msg = await mockFetch(10);
        setData(msg);
      } catch {
        setData('Error');
      }
    }
    fetchData();
  }, []);

  return (
    <Box>
      <Text>{data}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists, renders identically in deno and `runts dev`
- [ ] `async` function codegen produces compilable Rust
- [ ] `await` codegen produces `.await` in async context
- [ ] `runts build --release` produces working binary with 100% output match
