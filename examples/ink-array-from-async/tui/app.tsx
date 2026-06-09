// ink-array-from-async example — demonstrates `Array.fromAsync` (ES2024).
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust
//
// NOTE: Array.fromAsync is an ES2024 feature. It creates an array from
// an async iterable by awaiting each item.

import React from 'react';
import { Box, Text } from 'ink';

export default function ArrayFromAsyncDemo() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Array.fromAsync Demo (ES2024)</Text>
      <Text dimColor>Creates arrays from async iterables</Text>
      <Text></Text>
      <Text>Syntax: await Array.fromAsync(iterable)</Text>
      <Text></Text>
      <Text dimColor>Example usage:</Text>
      <Text>  async function* gen() {'{'} yield 1; {'}'}</Text>
      <Text>  const arr = await Array.fromAsync(gen())</Text>
    </Box>
  );
}
