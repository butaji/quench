# Task 095: `ink-new-target` Example — `new.target`

**Priority:** P3-Low
**Phase:** 10 — Extended TS/TSX Coverage
**Depends on:** 078

## Problem

`new.target` is a meta-property that detects whether a function was called with `new`. It is used for inheritance checks and enforcing constructor usage. No existing Ink example exercises it.

## Ink Example

```tsx
// examples/ink-new-target/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

class Base {
  constructor() {
    if (new.target === Base) {
      throw new Error('Base is abstract');
    }
  }
  
  getType(): string {
    return new.target?.name || 'unknown';
  }
}

class Derived extends Base {
  getType(): string {
    return 'Derived';
  }
}

const instance = new Derived();

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Type: {instance.getType()}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-new-target/`
- [ ] Uses `new.target` meta-property
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path handles `new.target`
- [ ] Parity harness passes with 100% match in all 3 environments