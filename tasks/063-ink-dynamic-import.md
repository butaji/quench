# Task 063: `ink-dynamic-import` Example — `import()`, `import.meta`

**Priority:** P2-Medium
**Phase:** 6 — Modules
**Depends on:** 062

## Problem

Zero examples use dynamic imports or `import.meta`.

## Example

```tsx
import { Box, Text, useState, useEffect } from 'ink';

export default function App() {
  const [data, setData] = useState('Loading...');

  useEffect(() => {
    async function load() {
      const mod = await import('./data.js');
      setData(mod.default);
    }
    load();
  }, []);

  return (
    <Box>
      <Text>{data}</Text>
      <Text>URL: {import.meta.url || 'unknown'}</Text>
    </Box>
  );
}
```

## Work

**Requires Task 079 (dynamic import in HIR) to be completed first.**

## Acceptance Criteria

- [ ] Example exists, renders identically in deno and `runts dev`
- [ ] `import()` parses into HIR (not Invalid)
- [ ] `import.meta` parses into HIR (not Invalid)
- [ ] `runts build --release` produces working binary
- [ ] Parity harness 100%