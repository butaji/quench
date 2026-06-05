// Dynamic children example — demonstrates rendering arrays as children.
// Simplified version for cross-environment parity.
//
// NOTE: Function components and complex dynamic rendering are simplified here
// to ensure parity across deno, runts dev, and runts build.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

const ITEMS = [
  { id: 1, label: 'First item', color: 'green' },
  { id: 2, label: 'Second item', color: 'yellow' },
  { id: 3, label: 'Third item', color: 'red' },
];

export default function DynamicChildrenExample() {
  // Static items for parity testing
  const items = ITEMS;

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Dynamic Children Demo</Text>
      <Text></Text>
      <Text dimColor>Items rendered from array:</Text>
      <Text></Text>
      <Box flexDirection="column" gap={1}>
        {items.map((item) => (
          <Box key={item.id} paddingX={1} borderStyle="round">
            <Text color={item.color as any}>{item.label}</Text>
          </Box>
        ))}
      </Box>
      <Text></Text>
      <Text dimColor>Total items: {items.length}</Text>
    </Box>
  );
}
