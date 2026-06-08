# Task 121: `ink-json-api` Example — `JSON.stringify`, `JSON.parse`

**Priority:** P1-High
**Phase:** 12 — Runtime API Completion
**Depends on:** 120

## Problem

`JSON.stringify` and `JSON.parse` are among the most commonly used JavaScript APIs. No existing Ink example explicitly exercises them in a TUI context.

## Ink Example

```tsx
// examples/ink-json-api/tui/app.tsx
import React, { useState } from 'react';
import { Box, Text } from 'ink';

const data = { name: 'App', version: 1, features: ['ui', 'api'] };
const json = JSON.stringify(data, null, 2);
const parsed = JSON.parse(json);

export default function App() {
  const [input, setInput] = useState('{"key":"value"}');
  let obj: Record<string, string> = {};
  try {
    obj = JSON.parse(input);
  } catch {
    obj = { error: 'invalid' };
  }

  return (
    <Box flexDirection="column">
      <Text>Stringified: {json.slice(0, 30)}...</Text>
      <Text>Parsed name: {parsed.name}</Text>
      <Text>Input key: {obj.key ?? obj.error}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-json-api/`
- [ ] Uses `JSON.stringify` with formatting
- [ ] Uses `JSON.parse` with error handling
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
