// ink-satisfies-expr example — demonstrates `satisfies` in object and array literals.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust

import React from 'react';
import { Box, Text } from 'ink';

export default function SatisfiesExprDemo() {
  const config = {
    theme: 'dark',
    width: 80,
  } as const satisfies Record<string, string | number>;

  const colors = ['red', 'green', 'blue'] as const satisfies readonly string[];

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Satisfies Expression Demo</Text>
      <Text></Text>
      <Text>Theme: dark</Text>
      <Text>Width: 80</Text>
      <Text>Colors: red, green, blue</Text>
    </Box>
  );
}
