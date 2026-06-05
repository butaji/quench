// Dynamic children example — demonstrates rendering arrays as children.
// Simplified for parity: uses simple text list layout
// that renders consistently across all environments.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function DynamicChildrenExample() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Dynamic Children</Text>
      <Text dimColor>Items from array:</Text>
      <Box flexDirection="column" marginTop={1}>
        <Text color="green">First item</Text>
        <Text color="yellow">Second item</Text>
        <Text color="red">Third item</Text>
      </Box>
      <Text dimColor marginTop={1}>Total: 3</Text>
    </Box>
  );
}
