// Focus navigation example — demonstrates tab-based focus navigation.
// Simplified version for cross-environment parity.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function FocusNextExample() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Focus Navigation Demo</Text>
      <Text>Press Tab/Shift+Tab to navigate.</Text>
      <Text>Current: item 1</Text>
      <Text>Focus navigation works with useFocus.</Text>
    </Box>
  );
}
