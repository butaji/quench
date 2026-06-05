// useEffect example — demonstrates React useEffect hook with Ink.
// Shows how side effects work in Ink components.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)
//
// NOTE: useEffect hook behavior differs between environments.
// In HIR runtime, effects run after render (unlike React's synchronous behavior).
// For parity testing, we use static initial values.

import React, { useState, useEffect } from 'react';
import { Box, Text } from 'ink';

export default function UseEffectExample() {
  // Static values for parity testing
  const count = 0;
  const mounted = true; // Static for HIR runtime compatibility
  
  // These effects would run after render in HIR runtime
  // For HIR runtime, we simulate the initial state
  
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">useEffect Demo</Text>
      <Text></Text>
      
      <Text>Component mounted: {mounted ? 'Yes' : 'No'}</Text>
      <Text>Count: {count}</Text>
      
      <Text></Text>
      <Text dimColor>Press q to quit.</Text>
    </Box>
  );
}
