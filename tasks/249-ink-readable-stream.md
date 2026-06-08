# Task 249: `ink-readable-stream` Example — `ReadableStream`

**Priority:** P3-Low
**Phase:** 21 — Runtime API Deep Coverage
**Depends on:** 248

## Problem

`ReadableStream` is the standard Web Streams API for reading data. No existing Ink example exercises it.

## Ink Example

```tsx
// examples/ink-readable-stream/tui/app.tsx
import React, { useState, useEffect } from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const [chunks, setChunks] = useState<string[]>([]);

  useEffect(() => {
    const stream = new ReadableStream({
      start(controller) {
        controller.enqueue('a');
        controller.enqueue('b');
        controller.enqueue('c');
        controller.close();
      },
    });

    const reader = stream.getReader();
    const result: string[] = [];

    function read(): Promise<void> {
      return reader.read().then(({ done, value }) => {
        if (done) {
          setChunks(result);
          return;
        }
        result.push(value as string);
        return read();
      });
    }

    read();
  }, []);

  return (
    <Box flexDirection="column">
      <Text>Chunks: {chunks.join(', ')}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-readable-stream/`
- [ ] Uses `ReadableStream` constructor and `getReader()`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust for ReadableStream
- [ ] Parity harness passes with 100% match in all 3 environments
