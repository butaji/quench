// List example — demonstrates list UI with bullet points.
// Uses column layout with consistent spacing.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function ListDemo() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Feature List</Text>
      <Text></Text>
      <Box>
        <Text color="green">• </Text>
        <Text>First item</Text>
      </Box>
      <Box>
        <Text color="yellow">• </Text>
        <Text>Second item</Text>
      </Box>
      <Box>
        <Text color="red">• </Text>
        <Text>Third item</Text>
      </Box>
      <Box>
        <Text color="blue">• </Text>
        <Text>Fourth item</Text>
      </Box>
      <Text></Text>
      <Text dimColor>Press q to quit.</Text>
    </Box>
  );
}
