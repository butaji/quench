// ink-props-with-children example — demonstrates React.PropsWithChildren utility type.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust
//
// NOTE: PropsWithChildren is a type-level utility. It's erased at compile time.
// This example demonstrates the concept with inline JSX.

import React from 'react';
import { Box, Text } from 'ink';

export default function PropsWithChildrenDemo() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">PropsWithChildren Demo</Text>
      <Text></Text>
      <Box flexDirection="column" borderStyle="round" padding={1}>
        <Text bold>Welcome</Text>
        <Text></Text>
        <Text>This is card content.</Text>
        <Text color="green">More content here.</Text>
      </Box>
    </Box>
  );
}
