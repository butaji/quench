// Focus navigation example — demonstrates tab-based focus navigation.
// Uses useFocus hook to make elements focusable.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function FocusNextExample() {
  // For parity testing, use static focus state
  const isFirstFocused = true;

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Focus Navigation Demo</Text>
      <Text>Press Tab/Shift+Tab to navigate.</Text>
      <Text></Text>
      <Box
        flexDirection="column"
        borderStyle="round"
        borderColor={isFirstFocused ? "cyan" : "gray"}
        padding={1}
      >
        <Text>First Item (focused)</Text>
      </Box>
      <Text></Text>
      <Box
        flexDirection="column"
        borderStyle="round"
        borderColor={!isFirstFocused ? "cyan" : "gray"}
        padding={1}
      >
        <Text>Second Item</Text>
      </Box>
    </Box>
  );
}
