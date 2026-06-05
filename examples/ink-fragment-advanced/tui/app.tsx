// Advanced Fragment Example — demonstrates React Fragment patterns.
// Shows fragment usage for grouping without extra DOM nodes.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

// Fragment to group related items
function MetaInfo({ label, value }: { label: string; value: string }) {
  return (
    <>
      <Text dimColor>{label}: </Text>
      <Text>{value}</Text>
      <Text>  </Text>
    </>
  );
}

// Fragment with key for lists
function ListItem({ name, status }: { name: string; status: string }) {
  return (
    <Text>
      • {name} <Text dimColor>({status})</Text>
    </Text>
  );
}

export default function FragmentAdvanced() {
  // Static values for parity testing
  const user = { name: "Admin", role: "administrator", loggedIn: true };
  const items = [
    { name: "Dashboard", status: "active" },
    { name: "Settings", status: "pending" },
    { name: "Profile", status: "active" },
  ];

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Fragment Demo</Text>
      <Text></Text>
      
      {/* Inline fragment */}
      <Text>
        User: <Text bold>{user.name}</Text>
      </Text>
      
      {/* Fragment as group */}
      <Box>
        <MetaInfo label="Role" value={user.role} />
        <MetaInfo label="Status" value={user.loggedIn ? "online" : "offline"} />
      </Box>
      
      <Text></Text>
      
      {/* Fragment for list */}
      <Text bold>Items:</Text>
      {items.map((item, i) => (
        <ListItem key={i} name={item.name} status={item.status} />
      ))}
      
      <Text></Text>
      <Text dimColor>Press q to quit.</Text>
    </Box>
  );
}
