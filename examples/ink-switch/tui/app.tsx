// Switch component example — demonstrates toggle switches.
// Simplified for cross-environment parity.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function SwitchDemo() {
  // Static display for parity testing
  const enabled = true;
  const disabled = false;

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Switch Component Demo</Text>
      <Text></Text>
      <Box>
        <Text>Option 1: </Text>
        <Text color={enabled ? "green" : "red"}>
          {enabled ? "[x] Enabled" : "[ ] Disabled"}
        </Text>
      </Box>
      <Box>
        <Text>Option 2: </Text>
        <Text color={disabled ? "red" : "green"}>
          {disabled ? "[ ] Disabled" : "[x] Enabled"}
        </Text>
      </Box>
      <Text></Text>
      <Text dimColor>Use space to toggle.</Text>
    </Box>
  );
}
