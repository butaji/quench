# Task 058: `ink-module-exports` Example — Named/Default/Re-exports, Namespace Imports

**Priority:** P1-High  
**Phase:** 6 — Modules  
**Depends on:** 057

## Problem

Module system is partially tested but lacks an Ink example with varied export patterns.

## Example

Create a multi-file example:

```tsx
// tui/utils.ts
export const VERSION = '1.0';
export function format(n: number) { return `#${n}`; }
export default function greet(name: string) { return `Hi ${name}`; }
```

```tsx
// tui/app.tsx
import { Box, Text } from 'ink';
import greet, { VERSION, format } from './utils';
import * as utils from './utils';

export { VERSION };
export { format as fmt };
export default function App() {
  return (
    <Box flexDirection="column">
      <Text>{greet('World')}</Text>
      <Text>v{VERSION}</Text>
      <Text>{format(42)}</Text>
      <Text>{utils.format(99)}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Multi-file example exists with named exports, default export, re-exports, namespace import
- [ ] Renders identically in deno and `runts dev`
- [ ] All module patterns produce compilable Rust
- [ ] `runts build --release` produces working binary with 100% output match
