// useEffect example — demonstrates React useEffect hook with Ink.
// Shows how side effects work in Ink components.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React, { useState, useEffect } from 'react';
import { Box, Text } from 'ink';

export default function UseEffectExample() {
  const [count, setCount] = useState(0);
  const [mounted, setMounted] = useState(false);
  
  // Simulate mount effect
  useEffect(() => {
    setMounted(true);
  }, []);
  
  // Simulate update effect  
  useEffect(() => {
    if (count > 0) {
      // Side effect on count change
    }
  }, [count]);
  
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
