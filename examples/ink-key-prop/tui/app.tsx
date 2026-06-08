// ink-key-prop example — demonstrates key prop usage in lists and fragments.
//
// The key prop helps React identify which items have changed.
// It should be unique among siblings.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust

import React, { useState } from 'react';
import { Box, Text } from 'ink';

interface Item {
  id: number;
  label: string;
}

interface Category {
  id: string;
  name: string;
  items: string[];
}

export default function App() {
  const [items] = useState<Item[]>([
    { id: 1, label: 'First item' },
    { id: 2, label: 'Second item' },
    { id: 3, label: 'Third item' },
  ]);

  const [categories] = useState<Category[]>([
    { id: 'cat-a', name: 'Category A', items: ['A1', 'A2'] },
    { id: 'cat-b', name: 'Category B', items: ['B1', 'B2', 'B3'] },
  ]);

  const [numbers] = useState<number[]>([10, 20, 30, 40]);

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">key Prop Demo</Text>
      <Text dimColor>key helps React identify changed items</Text>
      <Text></Text>

      {/* Basic key with id */}
      <Text>List with stable key (id):</Text>
      {items.map(item => (
        <Text key={item.id}>{item.id}: {item.label}</Text>
      ))}

      <Text></Text>
      {/* key with template string */}
      <Text>List with template key:</Text>
      {items.map((item, index) => (
        <Text key={`item-${item.id}-${index}`}>{item.id}-{index}: {item.label}</Text>
      ))}

      <Text></Text>
      {/* Nested lists with different keys */}
      <Text>Nested lists (category + item):</Text>
      <Box flexDirection="column" marginLeft={2}>
        {categories.map(cat => (
          <Box key={cat.id} flexDirection="column">
            <Text color="cyan">{cat.name}:</Text>
            {cat.items.map((item, idx) => (
              <Text key={`${cat.id}-${idx}`}>  {item}</Text>
            ))}
          </Box>
        ))}
      </Box>

      <Text></Text>
      {/* key with number index */}
      <Text>List with index key:</Text>
      {numbers.map((num, idx) => (
        <Text key={idx}>{idx}: {num}</Text>
      ))}

      <Text></Text>
      {/* Fragment children with keys */}
      <Text>Fragments with key:</Text>
      {items.map(item => (
        <React.Fragment key={`frag-${item.id}`}>
          <Text>- {item.label}</Text>
          <Text dimColor>  (id: {item.id})</Text>
        </React.Fragment>
      ))}

      <Text></Text>
      {/* Multiple children with shared key context */}
      <Text>Mixed elements with keys:</Text>
      {items.map(item => [
        <Text key={`${item.id}-label`}>{item.label}</Text>,
        <Text key={`${item.id}-meta`} dimColor>id: {item.id}</Text>,
      ])}
    </Box>
  );
}
