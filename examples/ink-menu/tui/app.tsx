// Menu example — demonstrates menu-style UI with navigation hints.
// Uses borders and spacing for visual hierarchy.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React, { useState } from 'react';
import { Box, Text, useInput } from 'ink';

const menuItems = [
  { label: 'New File', key: 'n' },
  { label: 'Open File', key: 'o' },
  { label: 'Save', key: 's' },
  { label: 'Settings', key: ',' },
  { label: 'Help', key: '?' },
];

export default function Menu() {
  const [selected, setSelected] = useState(0);

  useInput((input, key) => {
    if (key.upArrow) {
      setSelected(i => Math.max(0, i - 1));
    }
    if (key.downArrow) {
      setSelected(i => Math.min(menuItems.length - 1, i + 1));
    }
    if (input === 'q') {
      process.exit(0);
    }
  });

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Main Menu</Text>
      <Text></Text>
      <Box flexDirection="column" borderStyle="round" padding={1}>
        {menuItems.map((item, i) => (
          <Box key={i} justifyContent="space-between" width={30}>
            <Text color={i === selected ? 'green' : undefined} bold={i === selected}>
              {item.label}
            </Text>
            <Text dimColor>[{item.key}]</Text>
          </Box>
        ))}
      </Box>
      <Text></Text>
      <Text dimColor>Press q to quit.</Text>
    </Box>
  );
}
