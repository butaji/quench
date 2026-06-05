// Advanced Menu Example — demonstrates menu navigation.
// Shows hierarchical menu with selections.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

interface MenuItem {
  label: string;
  shortcut?: string;
  disabled?: boolean;
}

function MenuItemRow({ item, isSelected }: { item: MenuItem; isSelected: boolean }) {
  if (item.disabled) {
    return (
      <Text dimColor>
        <Text>{isSelected ? "▸ " : "  "}</Text>
        {item.label}
        {item.shortcut && ` (${item.shortcut})`}
      </Text>
    );
  }
  return (
    <Text>
      <Text color="cyan">{isSelected ? "▸ " : "  "}</Text>
      <Text bold={isSelected}>{item.label}</Text>
      {item.shortcut && <Text dimColor> ({item.shortcut})</Text>}
    </Text>
  );
}

export default function MenuAdvanced() {
  // Static values for parity testing
  const selectedIndex = 0;
  const menuItems: MenuItem[] = [
    { label: "New File", shortcut: "Ctrl+N" },
    { label: "Open File", shortcut: "Ctrl+O" },
    { label: "Save", shortcut: "Ctrl+S", disabled: true },
    { label: "Exit", shortcut: "Ctrl+Q" },
  ];

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Main Menu</Text>
      <Text></Text>
      <Box flexDirection="column" gap={1}>
        {menuItems.map((item, index) => (
          <MenuItemRow key={index} item={item} isSelected={index === selectedIndex} />
        ))}
      </Box>
      <Text></Text>
      <Text dimColor>↑↓ navigate, Enter select, q quit</Text>
    </Box>
  );
}
