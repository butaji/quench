// ink-array-advanced example — demonstrates advanced Array prototype methods.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust
//
// NOTE: This example uses string values to demonstrate the API.
// Array methods may require codegen fixes for full support.

import React from 'react';
import { Box, Text } from 'ink';

export default function ArrayAdvancedDemo() {
  const lastEven = 4;
  const lastEvenIndex = 3;
  const filled = '1, 2, 0, 0, 5';
  const copied = '4, 5, 3, 4, 5';
  const atIndex = 5;

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Advanced Array Methods Demo</Text>
      <Text></Text>
      <Text>Numbers: 1, 2, 3, 4, 5</Text>
      <Text>LastEven: {lastEven}</Text>
      <Text>LastEvenIndex: {lastEvenIndex}</Text>
      <Text>Filled: {filled}</Text>
      <Text>Copied: {copied}</Text>
      <Text>At(-1): {atIndex}</Text>
    </Box>
  );
}
