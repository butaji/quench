// Dynamic children example — demonstrates rendering arrays as children.
// Maps an array of items to Box/Text children.
//
// 1. deno: deno run -A main.tsx
// 2. runts dev: runts dev examples/ink-dynamic-children
// 3. runts compile: runts build examples/ink-dynamic-children --plugin ratatui --release

import React, { useState } from 'react';
import { Box, Text, Spacer, useInput, useApp } from 'ink';

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
  const [items] = useState<Item[]>(INITIAL_ITEMS);
  const [selected, setSelected] = useState(0);
  const { exit } = useApp();

  useInput((input, key) => {
    if (key.upArrow) {
      setSelected((prev) => Math.max(0, prev - 1));
    } else if (key.downArrow) {
      setSelected((prev) => Math.min(items.length - 1, prev + 1));
    } else if (input === 'q' || key.escape) {
      exit();
    }
  });

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
