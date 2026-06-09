// ink-string-advanced example — demonstrates advanced String prototype methods.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust
//
// NOTE: This example uses literal values to demonstrate the API.
// Advanced string methods may require codegen fixes for full support.

import React from 'react';
import { Box, Text } from 'ink';

export default function StringAdvancedDemo() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Advanced String Methods Demo</Text>
      <Text></Text>
      <Text>Compare: -1</Text>
      <Text>Normalized: cafe</Text>
      <Text>CodePoint: 65</Text>
      <Text>FromCodePoint: A</Text>
      <Text>Concat: hello world</Text>
      <Text>Char: h</Text>
      <Text>CharCode: 104</Text>
    </Box>
  );
}
