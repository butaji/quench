// Advanced Fragment Example — demonstrates React Fragment patterns with Ink.
// Shows fragment usage for grouping without extra DOM nodes.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)
//
// NOTE: Fragment syntax and && operator are not supported in runts HIR runtime.

import React from 'react';
import { Box, Text } from 'ink';

type Status = "active" | "inactive" | "pending";

function getStatusColor(status: Status): string {
  if (status === "active") return "green";
  if (status === "inactive") return "gray";
  return "yellow";
}

export default function FragmentAdvanced() {
  // Static values for parity testing
  const user = { name: "Admin", role: "administrator", status: "online" as Status };
  const items = [
    { name: "Dashboard", status: "active" as Status },
    { name: "Settings", status: "pending" as Status },
    { name: "Profile", status: "active" as Status },
  ];

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Fragment Demo</Text>
      <Text></Text>
      
      <Text>User: <Text bold>{user.name}</Text></Text>
      
      <Box>
        <Text dimColor>Role: </Text>
        <Text>{user.role}</Text>
        <Text>  </Text>
        <Text dimColor>Status: </Text>
        <Text>{user.status}</Text>
      </Box>
      
      <Text></Text>
      
      <Text bold>Items:</Text>
      {items.map((item, i) => {
        const color = getStatusColor(item.status);
        return (
          <Box key={i}>
            <Text color="dimColor">• </Text>
            <Text>{item.name}</Text>
            <Text color={color}> ({item.status})</Text>
          </Box>
        );
      })}
      
      <Text></Text>
      <Text dimColor>Press q to quit.</Text>
    </Box>
  );
}
