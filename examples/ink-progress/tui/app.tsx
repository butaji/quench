// Progress display example — demonstrates progress UI.
// Uses Text with repeated characters to create bars.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function ProgressDemo() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Download Progress</Text>
      <Text></Text>
      <Box flexDirection="column">
        <Text>Download: 75%</Text>
        <Box>
          <Text color="green">████████████████████</Text>
          <Text dimColor>░░░░░░░░░░░░░░░░░░░░</Text>
        </Box>
      </Box>
      <Text></Text>
      <Box flexDirection="column">
        <Text>Install: 30%</Text>
        <Box>
          <Text color="yellow">██████████</Text>
          <Text dimColor>████████████████████</Text>
        </Box>
      </Box>
      <Text></Text>
      <Box flexDirection="column">
        <Text>Verify: 0%</Text>
        <Box>
          <Text dimColor>████████████████████████████████</Text>
        </Box>
      </Box>
      <Text></Text>
      <Text dimColor>Press q to quit.</Text>
    </Box>
  );
}
