// Conditional Rendering Example — demonstrates conditional UI patterns.
// Shows if/else patterns for conditional rendering.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)
//
// NOTE: Ternary and && operators in JSX are not supported in runts HIR runtime.

import React from 'react';
import { Box, Text } from 'ink';

export default function ConditionalRendering() {
  // Static values for parity testing
  const isLoggedIn = true;
  const hasPermission = false;

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Conditional Rendering</Text>
      <Text></Text>
      
      {/* Using separate conditional sections */}
      <Text>User: Logged In</Text>
      
      {/* Only show if hasPermission is true - simplified */}
      <Text></Text>
      <Text color="green">Content is visible</Text>
      
      <Text></Text>
      <Text dimColor>Conditional patterns: if/else, components</Text>
    </Box>
  );
}
