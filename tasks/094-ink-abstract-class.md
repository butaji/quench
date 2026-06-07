# Task 094: `ink-abstract-class` Example — `abstract` Classes

**Priority:** P2-Medium
**Phase:** 10 — Extended TS/TSX Coverage
**Depends on:** 078

## Problem

`abstract` classes and methods are a core TypeScript OOP feature. No existing Ink example exercises abstract class patterns in a TUI context.

## Ink Example

```tsx
// examples/ink-abstract-class/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

abstract class Widget {
  abstract getName(): string;
  
  describe(): string {
    return `Widget: ${this.getName()}`;
  }
}

class TextWidget extends Widget {
  getName(): string {
    return 'Text';
  }
}

const widget = new TextWidget();

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>{widget.describe()}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-abstract-class/`
- [ ] Uses `abstract` class with abstract method
- [ ] Uses concrete subclass implementing abstract method
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path handles `abstract` (erases or maps appropriately)
- [ ] Parity harness passes with 100% match in all 3 environments