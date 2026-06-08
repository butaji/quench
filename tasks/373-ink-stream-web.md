# Task 373: `ink-stream-web` Example — `stream/web` (ReadableStream, WritableStream, TransformStream)

**Priority:** P2-Medium
**Phase:** 29 — Web Streams APIs
**Depends on:** 372

## Problem

`stream/web` provides web-standard streams in Node.js (`ReadableStream`, `WritableStream`, `TransformStream`). Task 249 covers `ReadableStream`; no example covers the full web streams surface.

## Ink Example

```tsx
// examples/ink-stream-web/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const stream = new ReadableStream({
    start(controller) {
      controller.enqueue('a');
      controller.enqueue('b');
      controller.close();
    },
  });
  const reader = stream.getReader();
  const chunks: string[] = [];

  async function read() {
    const { done, value } = await reader.read();
    if (!done) {
      chunks.push(value);
      return read();
    }
  }

  read();

  return (
    <Box flexDirection="column">
      <Text>Chunks: {chunks.join(', ')}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-stream-web/`
- [ ] Uses `ReadableStream` / `WritableStream` / `TransformStream`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
