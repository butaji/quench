# Task 145: `ink-uri-encoding` Example — `encodeURI`, `decodeURI`, `encodeURIComponent`, `decodeURIComponent`

**Priority:** P1-High
**Phase:** 12 — Runtime API Completion
**Depends on:** 144

## Problem

URI encoding/decoding functions (`encodeURI`, `decodeURI`, `encodeURIComponent`, `decodeURIComponent`) are standard JavaScript globals for URL-safe string handling. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-uri-encoding/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const text = 'hello world & foo=bar';
const encoded = encodeURIComponent(text);
const decoded = decodeURIComponent(encoded);
const uri = 'https://example.com/path?query=hello world';
const encodedUri = encodeURI(uri);

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Original: {text}</Text>
      <Text>Encoded: {encoded}</Text>
      <Text>Decoded: {decoded}</Text>
      <Text>URI: {encodedUri}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-uri-encoding/`
- [ ] Uses `encodeURI`, `decodeURI`, `encodeURIComponent`, `decodeURIComponent`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust for all URI functions
- [ ] Parity harness passes with 100% match in all 3 environments
