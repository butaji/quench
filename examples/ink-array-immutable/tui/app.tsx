// ink-array-immutable example — demonstrates ES2023 immutable array methods.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust

import React from 'react';
import { Box, Text } from 'ink';

const original = [1, 2, 3, 4, 5];
const spliced = original.toSpliced(1, 2, 'a', 'b');
const replaced = original.with(2, 'X');

export default function App() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Immutable Array Methods Demo</Text>
      <Text dimColor>ES2023 toSpliced and with</Text>
      <Text></Text>
      <Text>Original: {original.join(', ')}</Text>
      <Text>Spliced: {spliced.join(', ')}</Text>
      <Text>Replaced: {replaced.join(', ')}</Text>
      <Text>Unchanged: {original.join(', ')}</Text>
    </Box>
  );
}
