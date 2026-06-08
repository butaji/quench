# Task 305: `ink-commonjs-interop` Example — `module.exports`, `exports`, `require`

**Priority:** P1-High
**Phase:** 25 — Node.js Runtime APIs
**Depends on:** 304

## Problem

CommonJS interop patterns (`module.exports = ...`, `exports.x = ...`, `require('...')`) appear in mixed CJS/ESM codebases. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-commonjs-interop/lib.cjs
exports.greet = function(name: string): string {
  return `Hello, ${name}`;
};

// examples/ink-commonjs-interop/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>CommonJS interop example</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-commonjs-interop/`
- [ ] Uses `exports` or `module.exports` in a `.cjs` file
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path handles CommonJS interop
- [ ] Parity harness passes with 100% match in all 3 environments
