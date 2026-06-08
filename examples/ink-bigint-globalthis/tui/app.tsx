// ink-bigint-globalthis example — BigInt, numeric separators, globalThis
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust

import React from 'react';
import { Box, Text } from 'ink';

const big = 9007199254740993n;
const formatted = 1_000_000_000;
const isNode = typeof globalThis.process !== 'undefined';
const isDeno = typeof Deno !== 'undefined';
const platform = isNode ? 'Node.js' : (isDeno ? 'Deno' : 'Browser');

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>BigInt: {String(big)}</Text>
      <Text>Numeric separator: {formatted}</Text>
      <Text>Platform: {platform}</Text>
      <Text>Has globalThis: {typeof globalThis === 'object' ? 'yes' : 'no'}</Text>
    </Box>
  );
}
