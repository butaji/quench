# Task 079: `ink-logical-assign` Example — `||=`, `&&=`, `??=`

**Priority:** P1-High
**Phase:** 10 — Extended TS/TSX Coverage
**Depends on:** 078

## Problem

ES2021 logical assignment operators (`||=`, `&&=`, `??=`) are not exercised by any existing Ink example. These are common in real React/Ink apps for conditional state updates and default assignments.

## Ink Example

```tsx
// examples/ink-logical-assign/tui/app.tsx
import React, { useState } from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const [name, setName] = useState('');
  const [count, setCount] = useState(0);
  const [config, setConfig] = useState<{theme?: string}>({});

  // ||= (logical OR assignment)
  let displayName = name;
  displayName ||= 'Anonymous';

  // &&= (logical AND assignment)  
  let status = count > 0 && 'active';
  let label = 'Status: ';
  label &&= status || 'inactive';

  // ??= (nullish coalescing assignment)
  let theme = config.theme;
  theme ??= 'dark';

  return (
    <Box flexDirection="column">
      <Text>Name: {displayName}</Text>
      <Text>{label}</Text>
      <Text>Theme: {theme}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-logical-assign/`
- [ ] Uses `||=`, `&&=`, and `??=` operators
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
