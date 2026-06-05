// Focus example — demonstrates focus state display.
// Simplified for parity: uses simple text layout
// that renders consistently across all environments.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function Focus() {
  // Static focus state for parity testing
  const activeIndex = 0;

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Focus Demo</Text>
      <Box flexDirection="row" marginTop={1}>
        <Text color="green" bold>[1]</Text>
        <Text> A  </Text>
        <Text>[2]</Text>
        <Text> B  </Text>
        <Text>[3]</Text>
        <Text> C</Text>
      </Box>
    </Box>
  );
}
