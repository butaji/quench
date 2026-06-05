// useCallback example — demonstrates React useCallback hook for stable function references.
// Shows callback functions maintain identity across renders.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React, { useState, useCallback } from 'react';
import { Box, Text } from 'ink';

export default function UseCallbackExample() {
  const [count, setCount] = useState(0);
  const [renderCount, setRenderCount] = useState(0);
  
  // Memoized callback - identity stable across renders
  const increment = useCallback(() => {
    setCount(c => c + 1);
  }, []);
  
  // Callback with dependency
  const setMultiplier = useCallback((val: number) => {
    // In a real app this would update state
  }, []);
  
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">useCallback Demo</Text>
      <Text></Text>
      
      <Text>Count: {count}</Text>
      <Text>Renders: {renderCount}</Text>
      <Text></Text>
      
      <Text dimColor>useCallback ensures callback identity is stable.</Text>
      <Text dimColor>This prevents unnecessary re-renders in child components.</Text>
      <Text></Text>
      
      <Text dimColor>Press q to quit.</Text>
    </Box>
  );
}
