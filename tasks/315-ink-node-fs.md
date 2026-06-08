# Task 315: `ink-node-fs` Example — Node.js `fs` Module

**Priority:** P1-High
**Phase:** 26 — Node.js Standard Library
**Depends on:** 314

## Problem

The Node.js `fs` module (file system operations) is a core part of the Node.js standard library. No existing Ink example exercises `readFile`, `writeFile`, `existsSync`, `mkdirSync`, `readdir`, or other `fs` APIs.

## Ink Example

```tsx
// examples/ink-node-fs/tui/app.tsx
import React, { useState, useEffect } from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const [content, setContent] = useState('');

  useEffect(() => {
    try {
      const data = 'hello from fs';
      setContent(data);
    } catch (e: any) {
      setContent(`Error: ${e.message}`);
    }
  }, []);

  return (
    <Box flexDirection="column">
      <Text>Content: {content}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-node-fs/`
- [ ] References `fs` module patterns
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust for `fs` module calls
- [ ] Parity harness passes with 100% match in all 3 environments
