// Switch Example — demonstrates toggle/switch UI component.
// Shows switch/toggle patterns for boolean state.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

function Switch({ isOn }: { isOn: boolean }) {
  return (
    <Text color={isOn ? "green" : "gray"}>
      [{isOn ? "●" : "○"}]
    </Text>
  );
}

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
        <Switch isOn={notifications} />
      </Box>
      <Box justifyContent="space-between" width={40}>
        <Text>Dark Mode</Text>
        <Switch isOn={darkMode} />
      </Box>
      <Box justifyContent="space-between" width={40}>
        <Text>Auto-save</Text>
        <Switch isOn={autoSave} />
      </Box>
      <Text></Text>
      <Text dimColor>Press q to quit.</Text>
    </Box>
  );
}
