// Conditional Rendering Example — demonstrates conditional UI patterns.
// Shows if/else, ternary, && patterns for conditional rendering.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

function ConditionalContent({ show }: { show: boolean }) {
  if (!show) {
    return <Text dimColor>Content is hidden</Text>;
  }
  return <Text color="green">Content is visible</Text>;
}

export default function ConditionalRendering() {
  // Static values for parity testing
  const isLoggedIn = true;
  const hasPermission = false;
  const items = ["Item A", "Item B"];

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Conditional Rendering</Text>
      <Text></Text>
      
      {/* Ternary operator */}
      <Text>User: {isLoggedIn ? "Logged In" : "Guest"}</Text>
      
      {/* && operator */}
      {hasPermission && <Text color="yellow">⚠ Admin Access</Text>}
      
      {/* Component with conditional */}
      <ConditionalContent show={isLoggedIn} />
      
      <Text></Text>
      <Text dimColor>Conditional patterns: ternary, &&, component</Text>
    </Box>
  );
}
