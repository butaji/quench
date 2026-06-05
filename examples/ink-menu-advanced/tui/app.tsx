// Advanced Menu Example — demonstrates menu navigation.
// Shows hierarchical menu with selections.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)
//
// NOTE: Custom components and ternary operators are not supported in runts HIR runtime.

import React from 'react';
import { Box, Text } from 'ink';

export default function MenuAdvanced() {
  // Static values for parity testing
  const selectedIndex = 0;
  const menuItems = [
    { label: "New File", shortcut: "Ctrl+N", disabled: false },
    { label: "Open File", shortcut: "Ctrl+O", disabled: false },
    { label: "Save", shortcut: "Ctrl+S", disabled: true },
    { label: "Exit", shortcut: "Ctrl+Q", disabled: false },
  ];

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Main Menu</Text>
      <Text></Text>
      <Box flexDirection="column">
        <Text><Text color="cyan">▸ </Text><Text bold>New File</Text><Text dimColor> (Ctrl+N)</Text></Text>
        <Text>  Open File<Text dimColor> (Ctrl+O)</Text></Text>
        <Text dimColor>  Save<Text dimColor> (Ctrl+S)</Text></Text>
        <Text>  Exit<Text dimColor> (Ctrl+Q)</Text></Text>
      </Box>
      <Text></Text>
      <Text dimColor>↑↓ navigate, Enter select, q quit</Text>
    </Box>
  );
}
