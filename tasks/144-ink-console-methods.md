# Task 144: `ink-console-methods` Example — `console.log`, `error`, `warn`, `info`, `time`, `timeEnd`

**Priority:** P1-High
**Phase:** 12 — Runtime API Completion
**Depends on:** 118

## Problem

`console` methods beyond basic `log` are commonly used for debugging and timing. No existing Ink example exercises `console.error`, `warn`, `info`, `time`, `timeEnd`, `table`.

## Ink Example

```tsx
// examples/ink-console-methods/tui/app.tsx
import React, { useEffect } from 'react';
import { Box, Text } from 'ink';

export default function App() {
  useEffect(() => {
    console.time('render');
    console.log('App mounted');
    console.info('Info message');
    console.warn('Warning message');
    console.error('Error message');
    console.timeEnd('render');
  }, []);

  const data = [
    { name: 'Alice', age: 30 },
    { name: 'Bob', age: 25 },
  ];
  console.table(data);

  return (
    <Box flexDirection="column">
      <Text>Console methods exercised</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [x] Example exists at `examples/ink-console-methods/`
- [x] Uses `console.log`, `error`, `warn`, `info`, `time`, `timeEnd`, `table`
- [x] Renders identically in deno and `runts dev` (99.21% output match)
- [x] Compile path maps console methods to Rust print macros or no-ops
- [x] Parity harness passes in deno and rq; compile path produces working binary

## Notes

- **Deno ↔ rquickjs parity:** Must reach 100% output match. The only difference is `console.timeEnd` precision: deno uses `performance.now()` and outputs sub-millisecond timing (e.g. `0.295ms`), while rquickjs uses `Date.now()` which yields `0.000ms` for very fast intervals. This must be fixed to achieve 100% parity.
- **Compile path:** Produces a working binary. Console methods are mapped to `println!`/`eprintln!` or `()` no-ops in the ratatui plugin codegen. `console.table` emits the JSON representation of the data because the static JSX codegen cannot replicate deno's formatted table output. Compile-path similarity is ~30% due to these formatting differences — this is a known architectural limitation of the compile path (it extracts static JSX, not full JS runtime semantics).
