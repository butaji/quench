// Custom render example — demonstrates render options and custom components.
// NOTE: This example is simplified for runts HIR runtime compatibility.
// Custom function components and fragments are not yet supported.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text, Spacer } from 'ink';

export default function CustomRenderExample() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Custom Render Demo</Text>
      <Spacer />
      <Text dimColor>Static content is rendered once for performance.</Text>
      <Spacer />
      <Box flexDirection="column">
        <Text>Dynamic count: 42</Text>
        <Text dimColor>This content updates with state.</Text>
      </Box>
      <Spacer />
      <Text italic dimColor>
        Render options control stdout, exit, and debug modes.
      </Text>
    </Box>
  );
}
