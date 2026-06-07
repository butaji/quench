# Task 093: `ink-override-implements` Example — `override`, `implements`

**Priority:** P2-Medium
**Phase:** 10 — Extended TS/TSX Coverage
**Depends on:** 078

## Problem

`override` (TS 4.3+) and `implements` are class-related TypeScript features for explicit inheritance contracts. No existing Ink example exercises these.

## Ink Example

```tsx
// examples/ink-override-implements/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

interface Renderable {
  render(): string;
}

class BaseWidget {
  render(): string {
    return 'base';
  }
}

class FancyWidget extends BaseWidget implements Renderable {
  override render(): string {
    return 'fancy';
  }
}

const widget = new FancyWidget();

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Widget: {widget.render()}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-override-implements/`
- [ ] Uses `implements` clause on class
- [ ] Uses `override` keyword on method
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path erases `implements` and `override` without runtime impact
- [ ] Parity harness passes with 100% match in all 3 environments