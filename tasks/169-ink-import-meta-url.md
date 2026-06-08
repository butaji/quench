# Task 169: `ink-import-meta-url` Example — `import.meta.url`, `import.meta.resolve`

**Priority:** P1-High
**Phase:** 16 — Module Meta Completion
**Depends on:** 059

## Problem

`import.meta.url` and `import.meta.resolve` are standard ECMAScript module meta-properties. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-import-meta-url/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const url = import.meta.url;
const base = url.slice(0, url.lastIndexOf('/') + 1);

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>URL: {url.slice(0, 30)}...</Text>
      <Text>Base: {base}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-import-meta-url/`
- [ ] Uses `import.meta.url`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
