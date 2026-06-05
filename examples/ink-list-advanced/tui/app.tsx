// Advanced List Example — demonstrates list rendering patterns.
// Shows dynamic list rendering with index and keys.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

interface ListItem {
  id: number;
  name: string;
  status: "active" | "inactive" | "pending";
}

function StatusBadge({ status }: { status: ListItem["status"] }) {
  const colorMap = {
    active: "green",
    inactive: "gray",
    pending: "yellow",
  };
  return <Text color={colorMap[status]}>[{status}]</Text>;
}

function ListItemRow({ item, index }: { item: ListItem; index: number }) {
  return (
    <Box key={item.id} justifyContent="space-between" width={50}>
      <Text>
        <Text dimColor>{String(index + 1).padStart(2, "0")}.</Text> {item.name}
      </Text>
      <StatusBadge status={item.status} />
    </Box>
  );
}

export default function ListAdvanced() {
  // Static values for parity testing
  const items: ListItem[] = [
    { id: 1, name: "Project Alpha", status: "active" },
    { id: 2, name: "Project Beta", status: "pending" },
    { id: 3, name: "Project Gamma", status: "inactive" },
  ];

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Project List</Text>
      <Text></Text>
      <Box flexDirection="column" gap={1}>
        {items.map((item, index) => (
          <ListItemRow key={item.id} item={item} index={index} />
        ))}
      </Box>
      <Text></Text>
      <Text dimColor>Total: {items.length} items</Text>
      <Text dimColor>Press q to quit.</Text>
    </Box>
  );
}
