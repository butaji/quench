// Menu example — demonstrates menu-style UI with navigation hints.
// Uses borders and spacing for visual hierarchy.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function Menu() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Main Menu</Text>
      <Text></Text>
      <Box flexDirection="column" borderStyle="round" padding={1}>
        <Box justifyContent="space-between" width={30}>
          <Text>New File</Text>
          <Text dimColor>[n]</Text>
        </Box>
        <Box justifyContent="space-between" width={30}>
          <Text>Open File</Text>
          <Text dimColor>[o]</Text>
        </Box>
        <Box justifyContent="space-between" width={30}>
          <Text>Save</Text>
          <Text dimColor>[s]</Text>
        </Box>
        <Text></Text>
        <Box justifyContent="space-between" width={30}>
          <Text>Settings</Text>
          <Text dimColor>[,]</Text>
        </Box>
        <Box justifyContent="space-between" width={30}>
          <Text>Help</Text>
          <Text dimColor>[?]</Text>
        </Box>
      </Box>
      <Text></Text>
      <Text dimColor>Press q to quit.</Text>
    </Box>
  );
}
