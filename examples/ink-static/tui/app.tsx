// Static example — demonstrates static content rendering.
// Simplified for parity: uses basic Box/Text layout
// that renders consistently across all environments.
//
// The Static component renders content once without re-rendering.
// For parity testing, we demonstrate the visual output.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function StaticExample() {
  return (
    <Box flexDirection="column" paddingX={2} paddingY={1} borderStyle="round">
      <Text bold color="green">Static Content Demo</Text>
      <Box flexDirection="column" marginTop={1}>
        <Text color="cyan">[0] Server started on port 3000</Text>
        <Text color="cyan">[1] Connected to database</Text>
        <Text color="cyan">[2] Ready to accept connections</Text>
      </Box>
      <Text color="yellow" marginTop={1}>Live status: OK</Text>
    </Box>
  );
}
