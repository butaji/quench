# Task 250: `ink-compression-stream` Example — `CompressionStream` / `DecompressionStream`

**Priority:** P3-Low
**Phase:** 21 — Runtime API Deep Coverage
**Depends on:** 249

## Problem

`CompressionStream` and `DecompressionStream` compress/decompress data using gzip/deflate. No existing Ink example exercises these Web APIs.

## Ink Example

```tsx
// examples/ink-compression-stream/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

async function compress(input: string): Promise<number> {
  const encoder = new TextEncoder();
  const stream = new CompressionStream('gzip');
  const writer = stream.writable.getWriter();
  await writer.write(encoder.encode(input));
  await writer.close();

  const reader = stream.readable.getReader();
  let size = 0;
  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    size += value.length;
  }
  return size;
}

export default function App() {
  const [size, setSize] = React.useState<number | null>(null);

  React.useEffect(() => {
    compress('hello world hello world').then(setSize);
  }, []);

  return (
    <Box flexDirection="column">
      <Text>Compressed size: {size !== null ? size : '...'}</Text>
    </Box>
  );
}
```


## HIR Coverage

- Standard `Expr::Call` + `Expr::Member` for runtime API access

## Compile-Path Codegen

- Runtime API mapping in codegen or bridge globals

## Acceptance Criteria

- [ ] Example exists at `examples/ink-compression-stream/`
- [ ] Uses `CompressionStream` with `gzip`
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust for CompressionStream
- [ ] Parity harness passes with 100% match in all 3 environments
