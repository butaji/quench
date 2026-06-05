// Dynamic children example — demonstrates rendering arrays as children.
// Uses real array mapping with useState.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React, { useState } from 'react';
import { Box, Text } from 'ink';

export default function DynamicChildrenExample() {
  const [items] = useState([
    { label: 'First item', color: 'green' },
    { label: 'Second item', color: 'yellow' },
    { label: 'Third item', color: 'red' },
  ]);

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Dynamic Children</Text>
      <Text dimColor>Items from array:</Text>
      <Box flexDirection="column" marginTop={1}>
        {items.map((item, i) => (
          <Text key={i} color={item.color as any}>{item.label}</Text>
        ))}
      </Box>
      <Text dimColor marginTop={1}>Total: {items.length}</Text>
    </Box>
  );
}
