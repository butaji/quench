// useMemo example — demonstrates React useMemo hook for expensive computations.
// Shows memoized values persist across renders without recomputation.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React, { useState, useMemo } from 'react';
import { Box, Text } from 'ink';

export default function UseMemoExample() {
  const [count, setCount] = useState(0);
  const [multiplier, setMultiplier] = useState(1);
  
  // Expensive computation - memoized
  const expensiveResult = useMemo(() => {
    // Simulate expensive calculation
    let result = 0;
    for (let i = 0; i < 1000; i++) {
      result += count * multiplier;
    }
    return result;
  }, [count, multiplier]);
  
  // Simple memoized value
  const doubled = useMemo(() => count * 2, [count]);
  
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">useMemo Demo</Text>
      <Text></Text>
      
      <Text>Count: {count}</Text>
      <Text>Multiplier: {multiplier}</Text>
      <Text></Text>
      
      <Text bold>Memoized Results:</Text>
      <Text>  Expensive ({count} × {multiplier} × 1000): {expensiveResult}</Text>
      <Text>  Doubled ({count} × 2): {doubled}</Text>
      
      <Text></Text>
      <Text dimColor>Changes to multiplier won't trigger expensiveResult recomputation.</Text>
      <Text dimColor>Press q to quit.</Text>
    </Box>
  );
}
