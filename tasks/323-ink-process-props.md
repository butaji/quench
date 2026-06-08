# Task 323: `ink-process-props` Example — `process` Properties (`hrtime`, `memoryUsage`, `versions`, `pid`, `cwd`, `chdir`)

**Priority:** P1-High
**Phase:** 26 — Node.js Runtime Globals
**Depends on:** 322

## Problem

`process` object properties beyond `env`, `exit`, `stdin`, `stdout`, `stderr` are not covered. `hrtime`, `memoryUsage`, `versions`, `pid`, `title`, `cwd`, `chdir`, `uptime` are common runtime introspection APIs.

## Ink Example

```tsx
// examples/ink-process-props/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const pid = typeof process !== 'undefined' ? process.pid : 0;
  const cwd = typeof process !== 'undefined' && process.cwd ? process.cwd() : '/';
  const uptime = typeof process !== 'undefined' && process.uptime ? Math.floor(process.uptime()) : 0;

  return (
    <Box flexDirection="column">
      <Text>PID: {pid}</Text>
      <Text>CWD: {cwd}</Text>
      <Text>Uptime: {uptime}s</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-process-props/`
- [ ] Uses `process.pid`, `process.cwd()`, `process.uptime()`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
