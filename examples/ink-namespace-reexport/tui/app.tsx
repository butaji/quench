// ink-namespace-reexport example — demonstrates namespace re-export patterns.
//
// This example shows how to re-export modules using namespace syntax.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust

import React from 'react';
import { Box, Text } from 'ink';
import { Math } from '../utils/index.ts';

export default function App() {
  return (
    <Box flexDirection="column">
      <Text bold color="cyan">Namespace Re-export Demo</Text>
      <Text>2 + 3 = {Math.add(2, 3)}</Text>
      <Text>4 * 5 = {Math.mul(4, 5)}</Text>
      <Text>PI = {Math.PI.toFixed(2)}</Text>
    </Box>
  );
}
