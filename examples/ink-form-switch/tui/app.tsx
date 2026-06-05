// Switch Example — demonstrates toggle/switch UI component.
// Shows switch/toggle patterns for boolean state.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)
//
// NOTE: Custom components and ternary operators are not supported in runts HIR runtime.

import React from 'react';
import { Box, Text } from 'ink';

export default function FormSwitch() {
  // Static values for parity testing
  const notifications = true;
  const darkMode = false;
  const autoSave = true;

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Settings</Text>
      <Text></Text>
      <Box justifyContent="space-between" width={40}>
        <Text>Notifications</Text>
        <Text color="green">[●]</Text>
      </Box>
      <Box justifyContent="space-between" width={40}>
        <Text>Dark Mode</Text>
        <Text color="gray">[○]</Text>
      </Box>
      <Box justifyContent="space-between" width={40}>
        <Text>Auto-save</Text>
        <Text color="green">[●]</Text>
      </Box>
      <Text></Text>
      <Text dimColor>Press q to quit.</Text>
    </Box>
  );
}
