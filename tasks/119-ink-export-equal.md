# Task 119: `ink-export-equal` Example — `export =`, `import =`, `require()`

**Priority:** P3-Low
**Phase:** 11 — Module Pattern Coverage
**Depends on:** 078

## Problem

`export =` and `import = require()` are TypeScript's CommonJS module interop patterns. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-export-equal/lib.ts
function greet(name: string): string {
  return `Hello, ${name}`;
}

export = greet;

// examples/ink-export-equal/tui/app.tsx
import greet = require('../lib.js');
import { Box, Text } from 'ink';

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>{greet('World')}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-export-equal/`
- [ ] Uses `export =` in a module
- [ ] Uses `import = require()` syntax
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path handles `export =` and `import = require()`
- [ ] Parity harness passes with 100% match in all 3 environments
