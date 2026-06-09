# Task 412: `ink-process-info` Example — `process.platform`, `version`, `versions`, `arch`, `release`

**Priority:** P2-Medium
**Phase:** 33 — HIR & Codegen Edge Cases
**Depends on:** 411

## Problem

`process` properties beyond `exit`, `env`, `stdin`, `stdout` (Task 133) and `pid`, `cwd`, `uptime`, `hrtime`, `memoryUsage` (Task 323) are commonly accessed in Node.js applications. `platform`, `version`, `versions`, `arch`, and `release` provide environment introspection.

## HIR Coverage

- `Expr::Member` for `process.platform`, `process.version`, etc.
- `Expr::Member` for nested `process.versions.node`

## Compile-Path Codegen

- `quote_codegen_exprs.inc` for expression evaluation
- Runtime API mapping in codegen or bridge globals

## Ink Example

```tsx
// examples/ink-process-info/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const platform = process.platform;
const version = process.version;
const arch = process.arch;
const release = process.release?.name ?? 'node';
const nodeVersion = process.versions?.node ?? 'unknown';

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Platform: {platform}</Text>
      <Text>Version: {version}</Text>
      <Text>Arch: {arch}</Text>
      <Text>Release: {release}</Text>
      <Text>Node: {nodeVersion}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-process-info/`
- [ ] Uses `process.platform`, `process.version`, `process.versions`, `process.arch`, `process.release`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
