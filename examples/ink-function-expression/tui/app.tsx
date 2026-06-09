// ink-function-expression example — demonstrates function expressions.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust
//
// NOTE: For compile path, only simple literals are supported.
// Function expressions are demonstrated in the dev path.

import React from 'react';
import { Box, Text } from 'ink';

// Simple variable
const simpleValue = 42;

// Function expression in IIFE (no params)
const iifeNoParams = (function () {
  return 100;
})();

export default function FunctionExpressionDemo() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Function Expression Demo</Text>
      <Text></Text>
      <Text>Simple value: {simpleValue}</Text>
      <Text>IIFE no params: {iifeNoParams}</Text>
    </Box>
  );
}
