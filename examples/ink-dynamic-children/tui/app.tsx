// Dynamic children example — demonstrates rendering arrays as children.
// Maps an array of items to Box/Text children.
//
// NOTE: useInput and useApp hooks are not supported in runts HIR runtime.
// This version shows static selection for parity testing.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

const INITIAL_ITEMS = [
  { id: 1, label: 'First item', color: 'green' },
  { id: 2, label: 'Second item', color: 'yellow' },
  { id: 3, label: 'Third item', color: 'red' },
];

interface Item {
  id: number;
  label: string;
  color: string;
}

function ListItem({ item, selected }: { item: Item; selected: boolean }) {
  return (
    <Box
      paddingX={1}
      paddingY={0}
      borderStyle="round"
      borderColor={selected ? 'cyan' : 'white'}
      minWidth={20}
    >
      <Text bold={selected} color={selected ? 'cyan' : item.color}>
        {selected ? '> ' : '  '}
        {item.label}
      </Text>
    </Box>
  );
}

export default function DynamicChildrenExample() {
  // NOTE: For runts HIR runtime, useInput/useApp are not supported.
  // For parity testing, we show static state.
  const items = INITIAL_ITEMS;
  const selected = 0;

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Dynamic Children Demo</Text>
      <Text></Text>
      <Text dimColor>Items rendered from array:</Text>
      <Text></Text>
      
      {/* Dynamic children using array.map */}
      <Box flexDirection="column" gap={1}>
        {items.map((item, index) => (
          <ListItem
            key={item.id}
            item={item}
            selected={index === selected}
          />
        ))}
      </Box>
      
      <Text></Text>
      <Text dimColor>Up/Down to select, q/esc to quit.</Text>
      <Text dimColor>Total items: {items.length}</Text>
    </Box>
  );
}
