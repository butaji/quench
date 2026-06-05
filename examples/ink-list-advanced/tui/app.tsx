// Advanced List Example — demonstrates list rendering patterns.
// Shows dynamic list rendering with index and keys.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)
//
// NOTE: Custom components are not supported in runts HIR runtime.

import React from 'react';
import { Box, Text } from 'ink';

function getStatusColor(status: string): string {
  if (status === "active") return "green";
  if (status === "inactive") return "gray";
  return "yellow";
}

export default function App() {
  const items = [
    { id: 1, name: "Alpha", status: "active" },
    { id: 2, name: "Beta", status: "pending" },
    { id: 3, name: "Gamma", status: "inactive" },
  ];
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Project List</Text>
      <Box flexDirection="column">
        {items.map(item => {
          const color = getStatusColor(item.status);
          return (
            <Box key={item.id} justifyContent="space-between" width={50}>
              <Text>{item.name}</Text>
              <Text color={color}>[{item.status}]</Text>
            </Box>
          );
        })}
      </Box>
      <Text dimColor>Total: {items.length} items</Text>
      <Text dimColor>Press q to quit.</Text>
    </Box>
  );
}
